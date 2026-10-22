import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
const source = await readFile(new URL("../crates/rysmo_app/src/web_files.js", import.meta.url), "utf8");
async function fixture({ embedded=false, pickerError=null, readError=false, cancelInput=false, diagnosticsPanel=null, storage=null, indexedDB=null }={}) {
  const calls={open:0,save:0,inputs:0,downloads:[],writes:[],attached:0,removed:0};
  const listeners = new Map();
  const window={addEventListener(type, callback){listeners.set(type, callback);},showOpenFilePicker:async()=>{calls.open++;if(pickerError)throw pickerError;return [{name:"picked.rysmo",getFile:async()=>({text:async()=>"model"})}];},showSaveFilePicker:async()=>{calls.save++;return {name:"saved.rysmo",queryPermission:async()=>"granted",createWritable:async()=>({write:async text=>calls.writes.push(text),close:async()=>{}})};}};
  window.top=embedded?{}:window;
  if (storage) window.localStorage = storage;
  const document={getElementById:()=>diagnosticsPanel,body:{appendChild(node){node.attached=true;calls.attached++;}},createElement(type){
    if(type==="input")return {files:[{name:"fixture.rysmo",text:async()=>{if(readError)throw Error("read failed");return "fixture model";}}],remove(){calls.removed++;},click(){assert.equal(this.attached,true);calls.inputs++;queueMicrotask(()=>cancelInput?this.oncancel():this.onchange());}};
    if(type==="a")return {click(){calls.downloads.push(this.download);},remove(){}};
    throw Error(`Unexpected element ${type}`);
  }};
  const context=vm.createContext({window,document,indexedDB:indexedDB || {open(){throw Error("storage unavailable");}},console:{warn(){}},URL:{createObjectURL(){return "blob:test";},revokeObjectURL(){}},Blob,TextEncoder,Uint8Array,setTimeout(){},queueMicrotask});
  const module=new vm.SourceTextModule(source,{context});await module.link(()=>{});await module.evaluate();
  return {api:module.namespace,calls,window,listeners};
}
test("cross-origin embed opens via input even when storage is unavailable",async()=>{
  const {api,calls}=await fixture({embedded:true});
  assert.equal(JSON.stringify((await api.pick_open()).slice(0, 2)),JSON.stringify(["fixture.rysmo","fixture model"]));
  assert.equal(calls.open,0);assert.equal(calls.inputs,1);assert.equal(calls.attached,1);assert.equal(calls.removed,1);
});
test("blocked top-level picker falls back; user cancellation does not reopen it",async()=>{
  const blocked=await fixture({pickerError:Object.assign(Error("blocked"),{name:"SecurityError"})});
  assert.equal((await blocked.api.pick_open())[0],"fixture.rysmo");assert.equal(blocked.calls.inputs,1);
  const cancelled=await fixture({pickerError:Object.assign(Error("cancelled"),{name:"AbortError"})});
  assert.equal(await cancelled.api.pick_open(),null);assert.equal(cancelled.calls.inputs,0);
});
test("iframe Save downloads; top-level Save can still write the chosen file",async()=>{
  const embedded=await fixture({embedded:true});
  assert.equal((await embedded.api.save("model.rysmo","model",false))[1],"download");
  assert.equal(embedded.calls.save,0);assert.deepEqual(embedded.calls.downloads,["model.rysmo"]);
  const top=await fixture();assert.equal((await top.api.save("model.rysmo","model",false))[1],"picked");
  assert.deepEqual(top.calls.writes,["model"]);
});
test("failed input read completes without leaving an unresolved open operation",async()=>{
  const {api}=await fixture({embedded:true,readError:true});assert.equal(await api.pick_open(),null);
});

test("cancelled input removes the temporary picker",async()=>{
  const {api,calls}=await fixture({embedded:true,cancelInput:true});
  assert.equal(await api.pick_open(),null);
  assert.equal(calls.attached,1);assert.equal(calls.removed,1);
});
test("loader never displays more downloaded bytes than the served WASM size",async()=>{
  const status={textContent:""},fill={style:{}};
  const loading={classList:{remove(){}},querySelector:s=>s===".status"?status:fill};
  const loaderSource=await readFile(new URL("../crates/rysmo_app/loader.mjs",import.meta.url),"utf8");
  const context=vm.createContext({document:{getElementById:()=>loading,querySelector:()=>({href:"test.wasm"})},fetch:async()=>({headers:{get:()=>String(1048576)}})});
  const module=new vm.SourceTextModule(loaderSource,{context});await module.link(()=>{});await module.evaluate();
  const loader=module.namespace.default();loader.onStart();
  await new Promise(resolve=>setImmediate(resolve));
  loader.onProgress({current:2097152});
  assert.equal(status.textContent,"Downloading 1.0 of 1.0 MB");
  assert.equal(fill.style.width,"100%");
  loader.onSuccess();assert.equal(status.textContent,"Starting...");
});

test("renderer diagnostics update only the opt-in DOM panel",async()=>{
  const panel={dataset:{}};
  const enabled=await fixture({diagnosticsPanel:panel});
  enabled.api.set_render_diagnostics("Editor updates: 60 FPS");
  assert.equal(panel.dataset.editor,"Editor updates: 60 FPS");
  assert.ok(Number(panel.dataset.editorUpdated)>0);
  assert.equal(enabled.calls.open,0);assert.equal(enabled.calls.inputs,0);
  const disabled=await fixture();
  assert.doesNotThrow(()=>disabled.api.set_render_diagnostics("ignored"));
});

test("auto-save preferences persist, validate stored values, and tolerate blocked storage", async()=>{
  let stored = null;
  const storage = {getItem:()=>stored,setItem:(key,value)=>{assert.equal(key,"rysmo.autosave");stored=value;}};
  const {api} = await fixture({storage});
  assert.equal(JSON.stringify(api.get_autosave_settings()), "[true,1]");
  api.set_autosave_settings(false, 5);
  const reloaded = await fixture({storage});
  assert.equal(JSON.stringify(reloaded.api.get_autosave_settings()), "[false,5]");
  for (const invalid of ['oops', '{"enabled":true,"minutes":0}', '{"enabled":false,"minutes":999}', '{"enabled":true,"minutes":1.5}', '{"enabled":"false","minutes":5}']) {
    stored = invalid;
    assert.equal(JSON.stringify(api.get_autosave_settings()), "[true,1]");
  }
  const blocked = await fixture();
  assert.equal(JSON.stringify(blocked.api.get_autosave_settings()), "[true,1]");
  assert.doesNotThrow(()=>blocked.api.set_autosave_settings(true, 2));
});

test("packaged loader uses the optimized WASM size without another network request", async()=>{
  const status={textContent:""},fill={style:{}};
  const loading={classList:{remove(){}},querySelector:s=>s===".status"?status:fill};
  const loaderSource=await readFile(new URL("../crates/rysmo_app/loader.mjs",import.meta.url),"utf8");
  let requests=0;
  const context=vm.createContext({document:{getElementById:()=>loading,querySelector:s=>s.startsWith("meta")?{content:"1048576"}:{href:"test.wasm"}},fetch:async()=>{requests++;throw Error("Unexpected request");}});
  const module=new vm.SourceTextModule(loaderSource,{context});await module.link(()=>{});await module.evaluate();
  const loader=module.namespace.default();loader.onStart();loader.onProgress({current:524288});
  assert.equal(requests,0);
  assert.equal(status.textContent,"Downloading 0.5 of 1.0 MB");
  assert.equal(fill.style.width,"50%");
});

test("workspace preferences survive reload and report unavailable storage", async()=>{
  const values=new Map();
  const storage={getItem:key=>values.get(key)??null,setItem:(key,value)=>values.set(key,value)};
  const first=await fixture({storage});
  assert.equal(first.api.load_preferences(),null);
  const text=JSON.stringify({reverse_zoom:true,windows:{Inspector:[100,150,320,400]}});
  assert.equal(first.api.save_preferences(text),true);
  const second=await fixture({storage});
  assert.equal(second.api.load_preferences(),text);
  assert.equal(second.api.get_autosave_settings()[0],true);
  const blocked=await fixture({storage:{getItem(){throw Error("blocked")},setItem(){throw Error("blocked")}}});
  assert.equal(blocked.api.load_preferences(),null);
  assert.equal(blocked.api.save_preferences(text),false);
});


function recoveryDatabase({ aborts = 0, openFailures = 0 } = {}) {
  const stores = new Map(["state", "files", "handles"].map(name => [name, new Map()]));
  return { stores, open() {
    const request = {};
    queueMicrotask(() => {
      if (openFailures-- > 0) {
        request.error = Error("storage temporarily blocked");
        request.onerror();
        return;
      }
      request.result = {transaction(name, mode) {
        const transaction = {};
        transaction.objectStore = () => ({
          get(key) {
            const get = {};
            queueMicrotask(() => { get.result = stores.get(name).get(key); get.onsuccess(); });
            return get;
          },
          put(value, key) {
            assert.equal(mode, "readwrite");
            queueMicrotask(() => {
              if (aborts-- > 0) { transaction.onabort(); return; }
              stores.get(name).set(key, JSON.parse(JSON.stringify(value)));
              transaction.oncomplete();
            });
          },
        });
        return transaction;
      }};
      request.onsuccess();
    });
    return request;
  }};
}

test("recovery acknowledges commits, restores dirty state and keeps the previous copy on abort", async () => {
  const db = recoveryDatabase();
  const { api } = await fixture({ indexedDB: db });
  assert.equal(await api.autosave("work.rysmo", "first", true), true);
  const restored = await api.restore();
  assert.equal(restored.current.name, "work.rysmo");
  assert.equal(restored.current.text, "first");
  assert.equal(restored.current.dirty, true);
  // This transaction aborts without an error event, as IndexedDB permits.
  const blocked = recoveryDatabase({ aborts: 1 });
  blocked.stores.get("state").set("current", {name: "older.rysmo", text: "old", dirty: true});
  const retry = await fixture({ indexedDB: blocked });
  assert.equal(await retry.api.autosave("work.rysmo", "new", true), false);
  assert.equal((await retry.api.restore()).current.text, "old");
  assert.equal(await retry.api.autosave("work.rysmo", "new", false), true);
  assert.equal((await retry.api.restore()).current.dirty, false);
});

test("a failed database open can recover on the next autosave attempt", async () => {
  const { api } = await fixture({ indexedDB: recoveryDatabase({openFailures: 1}) });
  assert.equal(await api.autosave("work.rysmo", "draft", true), false);
  assert.equal(await api.autosave("work.rysmo", "draft", true), true);
  assert.equal((await api.restore()).current.text, "draft");
});

test("beforeunload warns only while the document has unsaved changes", async () => {
  const { api, listeners } = await fixture();
  let prevented = 0;
  const event = {preventDefault(){prevented++;}};
  const unload = listeners.get("beforeunload");
  unload(event);
  assert.equal(prevented, 0);
  api.set_unsaved_changes(true);
  unload(event);
  assert.equal(prevented, 1);
  assert.equal(event.returnValue, "");
  api.set_unsaved_changes(false);
  unload(event);
  assert.equal(prevented, 1);
});

test("a Save As completing after New cannot attach its handle to the new document", async () => {
  const { api, window, calls } = await fixture();
  let finish;
  const gate = new Promise(resolve => { finish = resolve; });
  const handle = {name:"old.rysmo", queryPermission:async()=>"granted", createWritable:async()=>({write:async text=>calls.writes.push(text), close:async()=>{}})};
  let picks = 0;
  window.showSaveFilePicker = async () => { picks++; await gate; return handle; };
  const pending = api.save("old.rysmo", "old contents", true);
  api.forget_file();
  finish();
  assert.equal((await pending)[0], "old.rysmo");
  await api.save("new.rysmo", "new contents", false);
  assert.equal(picks, 2);
  assert.deepEqual(calls.writes, ["old contents", "new contents"]);
});

test("an in-place save keeps its original handle while another file is opened", async () => {
  const { api, window, calls } = await fixture();
  let release;
  const gate = new Promise(resolve => {release = resolve;});
  let writes = 0;
  const original = {name:"original.rysmo", queryPermission:async()=>"granted", createWritable:async()=>({write:async text=>{writes++; if(writes===2) await gate; calls.writes.push(text);},close:async()=>{}})};
  window.showSaveFilePicker = async () => original;
  await api.save("original.rysmo", "first", true);
  const pending = api.save("original.rysmo", "second", false);
  const opened = await api.pick_open();
  api.accept_open_file(opened[2]);
  release();
  assert.equal((await pending)[0], "original.rysmo");
  assert.equal(calls.downloads.length, 0);
  assert.deepEqual(calls.writes, ["first", "second"]);
});


test("restored handles are used only after the editor accepts the recovery model", async () => {
  let writes = 0;
  const handle = {name:"restored.rysmo", queryPermission:async()=>"granted", createWritable:async()=>({write:async()=>{writes++;},close:async()=>{}})};
  const db = recoveryDatabase();
  db.stores.get("state").set("current", {name:"restored.rysmo", text:"model", dirty:true});
  db.stores.get("handles").set("restored.rysmo", handle);
  const rejected = await fixture({indexedDB:db});
  await rejected.api.restore();
  assert.equal((await rejected.api.save("draft.rysmo", "draft", false))[1], "picked");
  assert.equal(writes, 0);
  const accepted = await fixture({indexedDB:db});
  const restored = await accepted.api.restore();
  accepted.api.accept_open_file(restored.token);
  assert.equal((await accepted.api.save("restored.rysmo", "updated", false))[1], "file");
  assert.equal(writes, 1);
});

test("a rejected Open keeps the previous file handle", async () => {
  const {api,window,calls} = await fixture();
  const handle = {name:"valid.rysmo", queryPermission:async()=>"granted", createWritable:async()=>({write:async text=>calls.writes.push(text),close:async()=>{}})};
  window.showSaveFilePicker = async () => handle;
  await api.save("valid.rysmo", "valid", true);
  const opened = await api.pick_open();
  // Parsing failed, so the editor rejects the staged handle.
  api.reject_open_file(opened[2]);
  assert.equal((await api.save("valid.rysmo", "still valid", false))[0], "valid.rysmo");
  assert.deepEqual(calls.writes, ["valid", "still valid"]);
});

test("overlapping Open results keep separate handles until accepted or rejected", async () => {
  const {api,window} = await fixture();
  const writes = [];
  let next = 0;
  window.showOpenFilePicker = async () => {
    const name = `file-${++next}.rysmo`;
    return [{name, getFile:async()=>({text:async()=>"model"}), queryPermission:async()=>"granted", createWritable:async()=>({write:async text=>writes.push([name,text]),close:async()=>{}})}];
  };
  const first = await api.pick_open();
  const second = await api.pick_open();
  assert.notEqual(first[2], second[2]);
  api.accept_open_file(first[2]);
  api.reject_open_file(second[2]);
  assert.equal((await api.save(first[0], "updated", false))[0], "file-1.rysmo");
  assert.deepEqual(writes, [["file-1.rysmo", "updated"]]);
});
