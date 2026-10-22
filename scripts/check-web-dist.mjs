// Validate a built package without starting the editor or simulating a browser.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import vm from "node:vm";

const root = path.resolve(process.argv[2] || "crates/rysmo_app/dist");
const info = JSON.parse(await readFile(path.join(root, "build-info.json"), "utf8"));
for (const [name, expected] of Object.entries(info.assets)) {
  const file = path.resolve(root, name);
  assert.ok(file.startsWith(root + path.sep), `Asset escapes package: ${name}`);
  const bytes = await readFile(file);
  assert.equal(bytes.length, expected.bytes, `Wrong size: ${name}`);
  assert.equal(createHash("sha256").update(bytes).digest("hex"), expected.sha256, `Wrong hash: ${name}`);
}
const html = await readFile(path.join(root, "index.html"), "utf8");
const entry = Object.keys(info.assets).filter(name => /^rysmo-[a-f0-9]+\.js$/.test(name));
const wasmNames = Object.keys(info.assets).filter(name => name.endsWith(".wasm"));
assert.equal(entry.length, 1, "Expected one generated JavaScript entry point");
assert.equal(wasmNames.length, 1, "Expected one WASM module");
assert.ok(html.includes(entry[0]), "HTML does not reference the generated JavaScript");
assert.ok(html.includes(`content="${info.assets[wasmNames[0]].bytes}"`), "Loader size does not match WASM");
const modules = new Map();
const context = vm.createContext({});
async function moduleAt(url) {
  assert.equal(url.protocol, "file:", "Package must use local JavaScript dependencies");
  const file = fileURLToPath(url);
  assert.ok(file.startsWith(root + path.sep), `Import escapes package: ${url}`);
  const text = await readFile(file, "utf8");
  const fingerprint = url.searchParams.get("v");
  if (fingerprint) {
    assert.equal(createHash("sha256").update(text).digest("hex").slice(0, 16), fingerprint, `Stale import: ${url}`);
  }
  if (!modules.has(url.href)) {
    modules.set(url.href, new vm.SourceTextModule(text, {context, identifier:url.href}));
  }
  return modules.get(url.href);
}
const main = await moduleAt(pathToFileURL(path.join(root, entry[0])));
// Link every generated import to the real packaged exports, without evaluating it.
await main.link((specifier, referring) => moduleAt(new URL(specifier, referring.identifier)));
const binary = await WebAssembly.compile(await readFile(path.join(root, wasmNames[0])));
let directImports = 0;
for (const dependency of WebAssembly.Module.imports(binary)) {
  if (!dependency.module.startsWith("./snippets/")) continue;
  const file = fileURLToPath(new URL(dependency.module, main.identifier));
  const linked = [...modules.values()].find(module => fileURLToPath(new URL(module.identifier)) === file);
  assert.ok(linked, `WASM imports a module absent from JavaScript: ${dependency.module}`);
  assert.ok(Reflect.ownKeys(linked.namespace).includes(dependency.name), `Missing WASM import: ${dependency.module}:${dependency.name}`);
  directImports++;
}
console.log(JSON.stringify({
  revision: info.revision,
  tracked_changes: info.tracked_changes,
  assets: Object.keys(info.assets).length,
  linked_js_modules: modules.size,
  wasm_bytes: info.assets[wasmNames[0]].bytes,
  wasm_imports: WebAssembly.Module.imports(binary).length,
  direct_wasm_js_imports: directImports,
  wasm_exports: WebAssembly.Module.exports(binary).length,
  note: "Static package validation only; editor startup, WebGL and browser interactions are not exercised.",
}, null, 2));
