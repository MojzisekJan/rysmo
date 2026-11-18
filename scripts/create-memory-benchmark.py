#!/usr/bin/env python3
"""Add six deterministic 1024x1024 RGBA textures to the native benchmark fixture."""
import base64
import json
from pathlib import Path
import random
import struct
import zlib

root = Path(__file__).resolve().parents[1]
out = root / 'target/persistence-benchmark'
model = json.loads((out / 'unique.rysmo').read_text())
rng = random.Random(381)
pixels = rng.randbytes(1024 * 1024 * 4)
def chunk(kind, data):
    return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data) & 0xffffffff)
png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!IIBBBBB', 1024, 1024, 8, 6, 0, 0, 0))
png += chunk(b'IDAT', zlib.compress(b''.join(b'\0' + pixels[y*4096:(y+1)*4096] for y in range(1024)))) + chunk(b'IEND', b'')
model['images'] = [dict(id=20_000_000+i, name=f'Texture {i}', mime='image/png', data=base64.b64encode(png).decode()) for i in range(6)]
model['materials'] = [dict(id=21_000_000+i, name=f'Material {i}', color=[1,1,1,1], texture=dict(image=20_000_000+i, size=[1,1])) for i in range(6)]
for i, face in enumerate(model['faces']): face['material'] = 21_000_000+i%6
path = out / 'textured.rysmo'
path.write_text(json.dumps(model, separators=(',', ':')))
print(path, path.stat().st_size, 'bytes; six 4 MiB decoded RGBA images')
