#!/usr/bin/env python3
"""Stage an isolated renderer test app; never modify the distributable web build."""
import json
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "crates/rysmo_app/dist"
OUT = ROOT / "target/web-test/render-probe"
ASSETS = OUT / "assets"
ASSETS.mkdir(parents=True, exist_ok=True)
# Only runtime assets, excluding release metadata and the source archive.
for path in DIST.iterdir():
    if path.name in {"brand", "snippets"}:
        shutil.copytree(path, ASSETS / path.name, dirs_exist_ok=True)
    elif path.suffix in {".js", ".mjs", ".wasm"}:
        shutil.copy2(path, ASSETS / path.name)
modules = list(DIST.glob("rysmo-*.js"))
wasms = list(DIST.glob("*.wasm"))
assert len(modules) == len(wasms) == 1, "Use a fresh output directory for a different build."
files = [ASSETS / p.relative_to(DIST) for p in DIST.glob("snippets/*/src/web_files.js")]
assert len(files) == 1
source = files[0].read_text()
marker = "export async function restore() {"
assert marker in source
source = source.replace(marker, '''export async function restore() {
  // Test-only startup fixture. Uses the production Rust open/replace path.
  const model = new URLSearchParams(location.search).get("case") || "grid-flat-100";
  if (!/^grid-(flat|studio|textured)-(50|100|150)(-msaa4)?$/.test(model) && model !== "grid-section-100") throw Error("Unknown render case");
  const response = await fetch(`./fixtures/${model}.rysmo`);
  if (!response.ok) throw Error(`Fixture HTTP ${response.status}`);
  return {current:{name:model+".rysmo",text:await response.text(),dirty:false},recent:[]};
}
async function unusedBrowserRestore() {''', 1)
# Isolate repeatable renderer measurements from users' browser autosave/recent files.
source = source.replace("export function autosave(name, text, dirty) {", "export function autosave(name, text, dirty) { return;\n", 1)
files[0].write_text(source)
(OUT / "fixtures").mkdir(exist_ok=True)
fixtures = list((ROOT / "target/web-test").glob("grid-*.rysmo"))
assert fixtures, "Run scripts/create-web-benchmark.py first."
for path in fixtures:
    shutil.copy2(path, OUT / "fixtures" / path.name)
    if not path.stem.endswith("-msaa4"):
        document = json.loads(path.read_text())
        document["style"]["anti_aliasing"] = "msaa4"
        (OUT / "fixtures" / (path.stem + "-msaa4.rysmo")).write_text(json.dumps(document))
section = json.loads((OUT / "fixtures/grid-textured-100.rysmo").read_text())
section["section_planes"] = [{"id": 9000, "name": "Optimization regression", "origin": [12.2, 0, 0], "normal": [1, 0, 0], "active": True}]
(OUT / "fixtures/grid-section-100.rysmo").write_text(json.dumps(section))
html = '''<!doctype html><html lang="en"><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Rysmo renderer verification</title><style>
html,body{margin:0;height:100%;overflow:hidden;background:#d6e3f0}
#rysmo{display:block;width:100%;height:100%;outline:none;touch-action:none}
#loading{position:fixed;inset:0;display:grid;place-items:center;font:16px sans-serif;z-index:30;background:#d6e3f0}
</style><canvas id="rysmo" tabindex="0" role="application" aria-label="Rysmo 3D editor"></canvas>
<div id="loading">Starting renderer verification...</div><script type="module">
import init from MODULE;
import "./assets/web_diagnostics.mjs";
try {
  const wasm = await init({module_or_path: WASM});
  dispatchEvent(new CustomEvent("TrunkApplicationStarted",{detail:{wasm}}));
} catch(error) {
  document.getElementById("loading").textContent=String(error.stack||error);
}
</script></html>'''
html = html.replace("MODULE", json.dumps("./assets/" + modules[0].name)).replace("WASM", json.dumps("./assets/" + wasms[0].name))
(OUT / "index.html").write_text(html)
harness = (ROOT / "target/web-test/index.html").read_text().replace("http://localhost:8098/index.html?diagnostics", "./index.html?case=grid-textured-100&diagnostics")
(OUT / "embed.html").write_text(harness)
print(f"Staged isolated renderer probe in {OUT}")
print("Serve target/web-test with scripts/serve-web.py; open /render-probe/index.html?case=grid-textured-100&diagnostics")
