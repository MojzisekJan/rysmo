#!/usr/bin/env python3
"""Stage an isolated app and deterministic fixtures from an explicitly selected build."""
import argparse
import json
from pathlib import Path
import shutil

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--dist', type=Path, required=True)
args = parser.parse_args()
dist = args.dist.resolve()
out = root / 'target/web-profile-probe'
if out.exists(): shutil.rmtree(out)
assets = out / 'assets'
assets.mkdir(parents=True)
for path in dist.iterdir():
    if path.is_dir() and path.name in ('snippets', 'brand'):
        shutil.copytree(path, assets / path.name)
    elif path.suffix in ('.js', '.mjs', '.wasm', '.css', '.png', '.svg', '.woff2'):
        shutil.copy2(path, assets / path.name)
modules = list(assets.glob('rysmo-*.js'))
wasms = list(assets.glob('*.wasm'))
assert len(modules) == len(wasms) == 1, 'Use a clean isolated build directory'
files = list(assets.glob('snippets/*/src/web_files.js'))
assert len(files) == 1
# Runtime imports keep the same ABI; use current diagnostic JS in the isolated probe.
source = (root / "crates/rysmo_app/src/web_files.js").read_text()
source = source.replace('export async function restore() {', '''export async function restore() {
  const name = new URLSearchParams(location.search).get("case") || "profile-flat";
  if (!/^profile-(flat|textured|raw)$/.test(name)) throw Error("Unknown performance fixture");
  const response = await fetch(`./fixtures/${name}.rysmo`);
  if (!response.ok) throw Error(`Fixture HTTP ${response.status}`);
  return {current:{name:name+".rysmo",text:await response.text(),dirty:false},recent:[]};
}
async function unusedRestore() {''', 1)
files[0].write_text(source)
fixtures = out / 'fixtures'
fixtures.mkdir()
for case, original in [('flat','grid-flat-100.rysmo'), ('textured','grid-textured-100.rysmo')]:
    model = json.loads((root / 'target/web-test' / original).read_text())
    if case == 'flat':
        for material in model['materials']: material['color'] = [0.85,0.15,0.65,1]
    (fixtures / f'profile-{case}.rysmo').write_text(json.dumps(model,separators=(',',':')))
model = json.loads((root / 'target/persistence-benchmark/raw.rysmo').read_text())
# Persistence cubes are only 0.2 m wide with 2 m spacing. Enlarge them for
# viewport confirmation while retaining the same 72,000-face root topology.
for vertex in model['vertices']:
    cell = (vertex['id'] - 1000) // 100
    origin = [(cell % 100) * 2, (cell // 100) * 2, 0]
    vertex['p'] = [o + (p-o) * 7.5 for p,o in zip(vertex['p'], origin)]
model['materials'] = [dict(id=30_000_000,name='Visible surface marker',color=[0.85,0.15,0.65,1])]
for face in model['faces']: face['material'] = 30_000_000
model['style'] = dict(lighting='flat',anti_aliasing='fxaa',render_scale=1.0,edges=False)
(fixtures / 'profile-raw.rysmo').write_text(json.dumps(model,separators=(',',':')))
html = '''<!doctype html><html lang="en"><meta charset="utf-8"><title>Rysmo performance probe</title><link rel="icon" href="data:,">
<style>html,body{margin:0;height:100%;overflow:hidden;background:#d6e3f0}#rysmo{display:block;width:100%;height:100%;touch-action:none}#loading{position:fixed;inset:0;display:grid;place-items:center;background:#d6e3f0;z-index:30}</style>
<canvas id="rysmo" tabindex="0" role="application"></canvas><div id="loading">Starting isolated performance probe…</div>
<script type="module">import init from MODULE;import './assets/web_diagnostics.mjs';
try{const wasm=await init({module_or_path:WASM});dispatchEvent(new CustomEvent('TrunkApplicationStarted',{detail:{wasm}}));}
catch(error){document.getElementById('loading').textContent=String(error.stack||error);}</script></html>'''
html = html.replace('MODULE', json.dumps('./assets/'+modules[0].name)).replace('WASM',json.dumps('./assets/'+wasms[0].name))
(out / 'index.html').write_text(html)
print(out)
