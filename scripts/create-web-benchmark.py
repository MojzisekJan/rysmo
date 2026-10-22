#!/usr/bin/env python3
"""Generate disposable models and an iframe harness beneath target/web-test."""
import base64
import json
import re
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target/web-test"
OUT.mkdir(parents=True, exist_ok=True)
example = json.loads(re.search(r"```json\n(.*?)\n```", (ROOT / "docs/file-format.md").read_text(), re.S)[1])
base = {"format": "rysmo", "version": 17, "length_unit": "m", "display_units": "m", "definitions": example["definitions"], "materials": example["materials"]}
for kind in ["vertices", "edges", "faces"]:
    base[kind] = [entry for entry in example[kind] if entry.get("in") == 20]
base["instances"] = [{"id": 100+i, "def": 20, "o": [round(1.3*x, 9), round(1.3*y, 9), round(1.3*z, 9)], "x": [5,0,0], "y": [0,5,0], "z": [0,0,5], "material": 10} for i,(x,y,z) in enumerate((x,y,z) for z in range(3) for y in range(20) for x in range(20))]
base["camera"] = {"target": [12.8,12.8,1.5], "distance": 55, "yaw": -0.9, "pitch": 0.7, "fov_y": 0.7, "parallel": False, "eye_base_z": 0}

def chunk(kind, data):
    return struct.pack("!I",len(data)) + kind + data + struct.pack("!I",zlib.crc32(kind+data)&0xffffffff)
pixels = b"".join(b"\0" + b"".join(bytes((205,180,130) if (x//16+y//16)%2 else (70,100,135)) for x in range(1024)) for y in range(1024))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR",struct.pack("!IIBBBBB",1024,1024,8,2,0,0,0)) + chunk(b"IDAT",zlib.compress(pixels)) + chunk(b"IEND",b"")
(OUT / "checker.png").write_bytes(png)
for lighting in ["flat", "studio", "textured"]:
    for percent in [50,100,150]:
        doc = json.loads(json.dumps(base))
        doc["style"] = {"lighting": "studio" if lighting=="textured" else lighting, "studio_environment": "warm", "tone_mapping": "none" if lighting=="flat" else "ag_x", "render_scale": percent/100, "anti_aliasing": "fxaa", "edges": True}
        if lighting=="textured":
            doc["images"] = [{"id":80,"name":"checker.png","mime":"image/png","data":base64.b64encode(png).decode()}]
            doc["materials"][0].update(texture={"image":80,"size":[1,1]},color=[1,1,1,1])
            doc["style"].update(texture_limit=512,texture_anisotropy=8)
        (OUT / f"grid-{lighting}-{percent}.rysmo").write_text(json.dumps(doc))
# The same boxes with independent geometry, to measure snapshot cost as well as instances.
unique = json.loads(json.dumps(base))
unique.update(definitions=[], instances=[], vertices=[], edges=[], faces=[])
for i, instance in enumerate(base["instances"]):
    offset = 1000 + i * 100
    unique["definitions"].append(dict(base["definitions"][0], id=offset+20))
    unique["instances"].append(dict(instance, id=offset+90, **{"def":offset+20}))
    for vertex in base["vertices"]:
        unique["vertices"].append(dict(vertex, id=offset+vertex["id"], **{"in":offset+20}))
    for edge in base["edges"]:
        unique["edges"].append(dict(edge, id=offset+edge["id"], v=[offset+v for v in edge["v"]], **{"in":offset+20}))
    for face in base["faces"]:
        unique["faces"].append(dict(face, id=offset+face["id"], outer=[offset+v for v in face["outer"]], **{"in":offset+20}))
unique["style"] = {"lighting":"flat", "tone_mapping":"none"}
(OUT / "grid-unique-100.rysmo").write_text(json.dumps(unique))

# A small editable model for clipboard and save/restore checks.
small = json.loads(json.dumps(base))
small["instances"] = [dict(base["instances"][0], o=[0,0,0])]
small["camera"] = dict(base["camera"], target=[0.5,0.5,0.5], distance=5)
(OUT / "editable-cube.rysmo").write_text(json.dumps(small))
(OUT / "index.html").write_text('''<!doctype html><html lang="en"><meta charset="utf-8"><title>Rysmo embed verification</title><style>body{margin:0;background:#191d24;color:white;font:14px sans-serif}h1{margin:0;height:40px;line-height:40px;font-size:20px}.bar{display:flex;gap:8px;padding:4px;box-sizing:border-box;height:40px}button{margin:0;padding:6px}iframe{display:block;border:0;}</style><h1>Rysmo cross-origin embed check</h1><div class="bar"><button onclick="size(1280,800)">1280 × 800</button><button onclick="size(800,600)">800 × 600</button><button onclick="size(390,700)">390 × 700</button><button onclick="document.querySelector('iframe').requestFullscreen()">Fullscreen</button></div><iframe title="Rysmo editor" src="http://localhost:8098/index.html?diagnostics" width="800" height="600" allow="fullscreen" allowfullscreen sandbox="allow-scripts allow-same-origin allow-downloads allow-popups allow-pointer-lock"></iframe><script>function size(w,h){let frame=document.querySelector('iframe');frame.width=w;frame.height=h}</script></html>''')
print(f"Generated ten 1200-instance / 7200-face models in {OUT}")
