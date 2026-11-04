// Real WASM/egui editor workflows in Chromium with software WebGL2.
import {after, before, test} from "node:test";
import assert from "node:assert/strict";
import {createServer} from "node:http";
import {readFile, mkdir, rm, writeFile} from "node:fs/promises";
import {resolve, extname, sep} from "node:path";
import {fileURLToPath} from "node:url";
const {chromium} = await import(process.env.RYSMO_PLAYWRIGHT_MODULE || "playwright");
const root = resolve(fileURLToPath(new URL("../../", import.meta.url)));
const dist = resolve(process.env.RYSMO_WEB_DIST || resolve(root, "crates/rysmo_app/dist"));
const artifacts = resolve(root, "target/web-tests");
const mime = {".html":"text/html", ".js":"text/javascript", ".mjs":"text/javascript", ".wasm":"application/wasm", ".svg":"image/svg+xml", ".png":"image/png", ".ico":"image/x-icon"};
let server, base, downloads = 0;
before(async () => {
  await readFile(resolve(dist, "index.html"));
  await rm(artifacts, {recursive:true, force:true});
  await mkdir(artifacts, {recursive:true});
  server = createServer(async (req, res) => {
    try {
      const path = resolve(dist, "." + decodeURIComponent(new URL(req.url, "http://localhost").pathname.replace(/\/$/, "/index.html")));
      if (!path.startsWith(dist + sep)) { res.writeHead(403).end(); return; }
      const bytes = await readFile(path);
      res.writeHead(200, {"Content-Type":mime[extname(path)] || "application/octet-stream", "Cache-Control":"no-store"}).end(bytes);
    } catch { res.writeHead(404).end(); }
  });
  await new Promise((ok, fail) => { server.once("error", fail); server.listen(0, "127.0.0.1", ok); });
  base = `http://127.0.0.1:${server.address().port}`;
});
after(async () => {
  if (server) await new Promise(ok => server.close(ok));
});
async function editor(t, query="demo&diagnostics") {
  // Isolate software-GPU resources as well as browser storage between scenarios.
  const browser = await chromium.launch({headless:true, executablePath:process.env.RYSMO_CHROMIUM_EXECUTABLE, args:["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"]});
  let page;
  const errors = [];
  t.after(async () => {
    if (page) await page.screenshot({path:resolve(artifacts, t.name.replace(/\W+/g,"-") + ".png")}).catch(()=>{});
    await browser.close();
    assert.deepEqual(errors, [], "The editor must not raise browser exceptions");
  });
  const context = await browser.newContext({viewport:{width:1280,height:800}, deviceScaleFactor:1, acceptDownloads:true});
  page = await context.newPage();
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  // Exercise the file input/download fallback used by browsers without native pickers.
  await page.addInitScript(() => { window.showOpenFilePicker = undefined; window.showSaveFilePicker = undefined; });
  await page.goto(`${base}/?${query}`, {timeout:120000});
  await page.waitForFunction(() => !document.getElementById("loading"), undefined, {timeout:120000});
  await page.locator("#rysmo").focus();
  await state(page, /Camera synced: true/);
  return page;
}
async function state(page, pattern) {
  await page.waitForFunction(source => new RegExp(source).test(document.getElementById("web-diagnostics")?.dataset.editor || ""), pattern.source, {timeout:30000});
}
async function open(page, text, name="browser-test.rysmo") {
  const chooser = page.waitForEvent("filechooser");
  await page.keyboard.press("Control+o");
  await (await chooser).setFiles({name, mimeType:"application/json", buffer:Buffer.from(text)});
  await page.locator("#rysmo").focus();
}
async function click(page, x, y) {
  await page.mouse.click(x, y);
  // Let Bevy consume this gesture before sending the next one.
  await page.evaluate(() => new Promise(ok => requestAnimationFrame(() => requestAnimationFrame(ok))));
}
async function save(page) {
  const pending = page.waitForEvent("download", {timeout:30000});
  await page.keyboard.press("Control+Shift+s");
  const download = await pending;
  assert.equal(await download.failure(), null);
  await download.saveAs(resolve(artifacts, `download-${++downloads}.rysmo`));
  const text = await readFile(await download.path(), "utf8");
  const doc = JSON.parse(text);
  assert.equal(doc.format, "rysmo");
  return doc;
}
async function recoverySnapshot(page) {
  return page.evaluate(async () => {
    const request = indexedDB.open("rysmo", 1);
    const db = await new Promise((ok, fail) => { request.onsuccess = () => ok(request.result); request.onerror = () => fail(request.error); });
    try {
      const store = db.transaction("state").objectStore("state");
      const [current, workspace] = await Promise.all(["current", "workspace"].map(key => new Promise((ok, fail) => {
        const get = store.get(key); get.onsuccess = () => ok(get.result); get.onerror = () => fail(get.error);
      })));
      return {current, workspace, diagnostics:document.getElementById("web-diagnostics")?.dataset.editor};
    } finally { db.close(); }
  });
}
const empty = JSON.stringify({format:"rysmo",version:2,length_unit:"m",display_units:"m"});

test("demo boots with WebGL2 and a synchronized 3D camera", {timeout:180000}, async t => {
  const page = await editor(t);
  const canvas = await page.locator("#rysmo").boundingBox();
  assert.equal(canvas.width, 1280);
  assert.equal(canvas.height, 800);
  await state(page, /mesh entities: [1-9]/);
  assert.equal((await save(page)).format, "rysmo");
});

test("open draw undo redo save and reopen preserve actual geometry", {timeout:180000}, async t => {
  const page = await editor(t);
  await open(page, empty);
  await state(page, /Model: 0 faces \/ 0 edges/);
  await page.mouse.move(620, 440);
  await page.keyboard.press("r");
  await state(page, /Tool: Rectangle/);
  await page.keyboard.press("ArrowUp");
  await click(page, 620, 440);
  await page.mouse.move(820, 560, {steps:8});
  await page.keyboard.type("2,1", {delay:80});
  await page.keyboard.press("Enter");
  await state(page, /Model: 1 faces \/ 4 edges/);
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+z");
  await state(page, /Model: 0 faces \/ 0 edges/);
  await page.keyboard.press("Control+Shift+z");
  await state(page, /Model: 1 faces \/ 4 edges/);
  const saved = await save(page);
  assert.equal(saved.faces.length, 1);
  assert.equal(saved.edges.length, 4);
  assert.equal(saved.vertices.length, 4);
  const vertices = new Map(saved.vertices.map(vertex => [vertex.id, vertex.p]));
  const lengths = saved.edges.map(edge => {
    const a = vertices.get(edge.v[0]), b = vertices.get(edge.v[1]);
    return Math.hypot(...a.map((value, index) => value - b[index]));
  }).sort((a,b)=>a-b);
  for (const [index, expected] of [1,1,2,2].entries()) assert.ok(Math.abs(lengths[index] - expected) < 1e-6);
  await open(page, JSON.stringify(saved), "roundtrip.rysmo");
  await state(page, /Model: 1 faces \/ 4 edges/);
  const reopened = await save(page);
  for (const part of ["vertices","edges","faces"]) assert.deepEqual(reopened[part], saved[part]);
});

test("dirty document survives reload through IndexedDB recovery", {timeout:300000}, async t => {
  const page = await editor(t);
  await open(page, empty);
  await state(page, /Model: 0 faces \/ 0 edges/);
  await page.mouse.move(620, 440);
  await page.keyboard.press("l");
  await state(page, /Tool: Line/);
  await click(page, 620, 440);
  await click(page, 820, 520);
  await page.keyboard.press("Escape");
  await state(page, /Model: 0 faces \/ 1 edges/);
  // IndexedDB reads are asynchronous. Poll their awaited result on the Node side;
  // a Promise itself is truthy and cannot be used as a synchronous DOM predicate.
  const deadline = Date.now() + 90000;
  let committed, snapshot;
  while (Date.now() < deadline) {
    snapshot = await recoverySnapshot(page);
    const active = snapshot.workspace?.documents.find(doc => doc.id === snapshot.workspace.active);
    if (snapshot.current?.dirty && snapshot.current.name === "browser-test.rysmo"
        && JSON.parse(snapshot.current.text).edges?.length === 1
        && active?.dirty && active.name === "browser-test.rysmo"
        && JSON.parse(active.text).edges?.length === 1) {
      committed = snapshot;
      break;
    }
    await new Promise(ok => setTimeout(ok, 250));
  }
  await writeFile(resolve(artifacts, "recovery-before.json"), JSON.stringify(snapshot, null, 2));
  assert.ok(committed, "Recovery must commit the dirty model and active document before reload");
  page.on("dialog", dialog => dialog.accept());
  await page.goto(`${base}/?diagnostics`, {timeout:120000});
  await page.waitForFunction(() => !document.getElementById("loading"), undefined, {timeout:120000});
  await writeFile(resolve(artifacts, "recovery-after.json"), JSON.stringify(await recoverySnapshot(page), null, 2));
  await state(page, /Model: 0 faces \/ 1 edges/);
  await state(page, /dirty true/);
  await page.locator("#rysmo").focus();
  const restored = await save(page);
  assert.equal(restored.edges.length, 1);
});
