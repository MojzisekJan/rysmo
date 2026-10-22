// The web build's files (crates/rysmo_app/src/web.rs).
//
// - The open document is autosaved into IndexedDB, so a reload or a closed tab does not
//   lose work, and the recent files are kept there too, each with a copy of its text.
// - Where the browser has the File System Access API (Chromium), Open and Save work on
//   the real file: Save writes it in place. Elsewhere a file comes in through a file
//   input and goes out as a download.
//
// The structure follows Heron Brush's web_files.js.

let database = null;
// The handle of the file the open document came from or was saved to, if any.
let current = null;
let session = 0;
const pendingFiles = new Map();
let nextFile = 0;
function stage_file(handle) {
  const token = ++nextFile;
  pendingFiles.set(token, handle);
  return token;
}
const RECENT_MAX = 8;

function open() {
  database ??= new Promise((resolve, reject) => {
    const request = indexedDB.open("rysmo", 1);
    request.onupgradeneeded = () => {
      const db = request.result;
      db.createObjectStore("state");   // "current" -> {name, text, dirty}; "recent" -> [names]
      db.createObjectStore("files");   // name -> text (a copy, for Open Recent)
      db.createObjectStore("handles"); // name -> FileSystemFileHandle
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  }).catch(error => {
    database = null;
    throw error;
  });
  return database;
}

async function get(store, key) {
  const db = await open();
  return new Promise((resolve, reject) => {
    const request = db.transaction(store).objectStore(store).get(key);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function put(store, key, value) {
  const db = await open();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction(store, "readwrite");
    if (value === undefined) transaction.objectStore(store).delete(key);
    else transaction.objectStore(store).put(value, key);
    transaction.oncomplete = () => resolve();
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error || new Error("storage transaction aborted"));
  });
}

function warn(what, error) {
  console.warn(`Rysmo: ${what}`, error);
}

// Puts a file at the top of the recent list, keeping a copy of its text and its handle.
async function remember(name, text, handle) {
  try {
    await put("files", name, text);
    // A handle that cannot be stored (some browsers refuse) only costs writing in place
    // after a reload.
    await put("handles", name, handle || undefined).catch((error) => warn("could not keep the file's handle", error));
    const recent = ((await get("state", "recent")) || []).filter((n) => n !== name);
    recent.unshift(name);
    for (const dropped of recent.splice(RECENT_MAX)) {
      await put("files", dropped, undefined);
      await put("handles", dropped, undefined);
    }
    await put("state", "recent", recent);
  } catch (error) {
    warn("could not update the recent files", error);
  }
}

// {current: {name, text, dirty} or null, recent: [names]}, with the current file's
// handle staged until Rust accepts the parsed model.
export async function restore() {
  const started = session;
  try {
    const doc = (await get("state", "current")) || null;
    const recent = (await get("state", "recent")) || [];
    const handle = doc ? (await get("handles", doc.name)) || null : null;
    const token = doc && session === started ? stage_file(handle) : null;
    return { current: doc, recent, token };
  } catch (error) {
    warn("the browser's storage is not available", error);
    return { current: null, recent: [] };
  }
}

export async function recent_names() {
  try {
    return (await get("state", "recent")) || [];
  } catch {
    return [];
  }
}

export async function autosave(name, text, dirty) {
  try {
    await put("state", "current", { name, text, dirty });
    return true;
  } catch (error) {
    warn("could not keep the model", error);
    return false;
  }
}

// A new, empty document: no file behind it.
export function forget_file() {
  session++;
  current = null;
  pendingFiles.clear();
}

// The editor calls this only after parsing and replacing the document successfully.
export function accept_open_file(token) {
  if (!pendingFiles.has(token)) return;
  current = pendingFiles.get(token);
  session++;
  pendingFiles.delete(token);
}
export function reject_open_file(token) {
  pendingFiles.delete(token);
}

const types = [{ description: "Rysmo model", accept: { "application/json": [".rysmo"] } }];

// [name, text] of a file the user picks, or null when they cancel.
export async function pick_open() {
  if (window.top === window && typeof window.showOpenFilePicker === "function") {
    try {
      const [handle] = await window.showOpenFilePicker({ types });
      const text = await (await handle.getFile()).text();
      const token = stage_file(handle);
      await remember(handle.name, text, handle);
      return [handle.name, text, token];
    } catch (error) {
      if (error?.name === "AbortError") return null;
      warn("could not open through the file picker, choosing another way", error);
    }
  }
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".rysmo,application/json";
    input.hidden = true;
    document.body.appendChild(input);
    const finish = value => { input.remove(); resolve(value); };
    input.onchange = async () => {
      const file = input.files[0];
      if (!file) return finish(null);
      try {
        const text = await file.text();
        const token = stage_file(null);
        await remember(file.name, text, null);
        finish([file.name, text, token]);
      } catch (error) {
        warn("could not read the selected file", error);
        finish(null);
      }
    };
    input.oncancel = () => finish(null);
    input.click();
  });
}

// [name, text] of a recent file, from its stored copy.
export async function open_recent(name) {
  try {
    const text = await get("files", name);
    if (text === undefined) return null;
    const handle = (await get("handles", name)) || null;
    const token = stage_file(handle);
    await remember(name, text, handle);
    return [name, text, token];
  } catch (error) {
    warn("could not open the recent file", error);
    return null;
  }
}

async function write(handle, text) {
  const mode = { mode: "readwrite" };
  if ((await handle.queryPermission(mode)) !== "granted" && (await handle.requestPermission(mode)) !== "granted") {
    throw new Error("permission refused");
  }
  const writable = await handle.createWritable();
  await writable.write(text);
  await writable.close();
}

// Saves the document. Returns [name, how] with how "file" (written in place), "picked"
// (written to a newly chosen file) or "download", or null when the user cancels.
export async function save(name, text, save_as) {
  const started = session;
  const file = current;
  if (file && !save_as) {
    try {
      await write(file, text);
      await remember(file.name, text, file);
      return [file.name, "file"];
    } catch (error) {
      warn("could not write the file, choosing another way", error);
    }
  }
  if (window.top === window && typeof window.showSaveFilePicker === "function") {
    try {
      const handle = await window.showSaveFilePicker({ suggestedName: name, types });
      await write(handle, text);
      if (session === started) current = handle;
      await remember(handle.name, text, handle);
      return [handle.name, "picked"];
    } catch (error) {
      if (error?.name === "AbortError") return null;
      warn("could not save through the file picker, downloading instead", error);
    }
  }
  download(name, new TextEncoder().encode(text), "application/json");
  if (session === started) current = null;
  await remember(name, text, null);
  return [name, "download"];
}

// Hands the browser a file to save in its downloads.
export function download(name, bytes, mime) {
  const url = URL.createObjectURL(new Blob([bytes.slice()], { type: mime }));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
}

// Image picking leaves the open model's file handle and recent-file list intact.
export function pick_texture_image() {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/png,image/jpeg,.png,.jpg,.jpeg";
    input.onchange = async () => {
      const file = input.files[0];
      if (!file) return resolve(null);
      try { resolve([file.name, new Uint8Array(await file.arrayBuffer())]); }
      catch (error) { warn("could not read texture image", error); resolve(null); }
    };
    input.oncancel = () => resolve(null);
    input.click();
  });
}

// Keep editor shortcuts from opening browser bookmarks or file dialogs while the
// canvas has focus. Copy/cut/paste events remain available to egui text fields.
let shortcutsInstalled = false;
export function install_shortcuts() {
  if (shortcutsInstalled) return;
  shortcutsInstalled = true;
  window.addEventListener("keydown", event => {
    if (event.target?.tagName !== "CANVAS") return;
    if (event.key === "F1" && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault();
      return;
    }
    if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
    if (["a", "d", "s", "o", "n", "z", "y"].includes(event.key.toLowerCase())) {
      event.preventDefault();
    }
  }, {capture: true});
}

// Keep this independent of the delayed IndexedDB autosave.
let unsavedChanges = false;
export function set_unsaved_changes(dirty) { unsavedChanges = dirty; }
window.addEventListener("beforeunload", (event) => {
  if (!unsavedChanges) return;
  event.preventDefault();
  event.returnValue = "";
});

// Opt-in QA output, consumed by web_diagnostics.mjs. No storage or network traffic.
export function set_render_diagnostics(text) {
  const panel = document.getElementById("web-diagnostics");
  if (panel) {
    panel.dataset.editor = text;
    panel.dataset.editorUpdated = String(Date.now());
  }
}

// Application preferences, independent of the model and its undo history.
export function get_autosave_settings() {
  try {
    const value = JSON.parse(window.localStorage.getItem("rysmo.autosave"));
    if (typeof value?.enabled === "boolean" && Number.isInteger(value.minutes) && value.minutes >= 1 && value.minutes <= 120) {
      return [value.enabled, value.minutes];
    }
  } catch { /* Storage can be blocked in embedded/private browsing. */ }
  return [true, 1];
}

export function set_autosave_settings(enabled, minutes) {
  try {
    window.localStorage.setItem("rysmo.autosave", JSON.stringify({ enabled, minutes }));
  } catch { /* The current session can still use these preferences. */ }
}

// Device preferences are independent of model recovery and its save interval.
export function load_preferences() {
  try { return window.localStorage.getItem("rysmo.preferences.v1"); }
  catch { return null; }
}
export function save_preferences(text) {
  try { window.localStorage.setItem("rysmo.preferences.v1", text); return true; }
  catch { return false; }
}
