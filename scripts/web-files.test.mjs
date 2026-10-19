import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
const source = await readFile(new URL("../crates/rysmo_app/src/web_files.js", import.meta.url), "utf8");
async function fixture({ embedded=false, pickerError=null, readError=false, cancelInput=false, diagnosticsPanel=null, storage=null }={}) {
  const calls={open:0,save:0,inputs:0,downloads:[],writes:[],attached:0,removed:0};
  const window={addEventListener(){},showOpenFilePicker:async()=>{calls.open++;if(pickerError)throw pickerError;return [{name:"picked.rysmo",getFile:async()=>({text:async()=>"model"})}];},showSaveFilePicker:async()=>{calls.save++;return {name:"saved.rysmo",queryPermission:async()=>"granted",createWritable:async()=>({write:async text=>calls.writes.push(text),close:async()=>{}})};}};
  window.top=embedded?{}:window;
  if (storage) window.localStorage = storage;
  const document={getElementById:()=>diagnosticsPanel,body:{appendChild(node){node.attached=true;calls.attached++;}},createElement(type){
    if(type==="input")return {files:[{name:"fixture.rysmo",text:async()=>{if(readError)throw Error("read failed");return "fixture model";}}],remove(){calls.removed++;},click(){assert.equal(this.attached,true);calls.inputs++;queueMicrotask(()=>cancelInput?this.oncancel():this.onchange());}};
    if(type==="a")return {click(){calls.downloads.push(this.download);},remove(){}};
    throw Error(`Unexpected element ${type}`);
  }};
  const context=vm.createContext({window,document,indexedDB:{open(){throw Error("storage unavailable");}},console:{warn(){}},URL:{createObjectURL(){return "blob:test";},revokeObjectURL(){}},Blob,TextEncoder,Uint8Array,setTimeout(){},queueMicrotask});
  const module=new vm.SourceTextModule(source,{context});await module.link(()=>{});await module.evaluate();
  return {api:module.namespace,calls};
}
test("cross-origin embed opens via input even when storage is unavailable",async()=>{
  const {api,calls}=await fixture({embedded:true});
  assert.equal(JSON.stringify(await api.pick_open()),JSON.stringify(["fixture.rysmo","fixture model"]));
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
