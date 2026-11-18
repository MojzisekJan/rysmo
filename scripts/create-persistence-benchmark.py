#!/usr/bin/env python3
"""Generate CPU persistence fixtures from the documented native cube example."""
import argparse
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--count", type=int, default=12_000)
parser.add_argument("--output", type=Path, default=ROOT / "target/persistence-benchmark")
parser.add_argument("--raw", action="store_true", help="also create one root mesh for face-preparation timings")
args = parser.parse_args()
if not 1 <= args.count <= 60_000:
    parser.error("count must be between 1 and 60000")
args.output.mkdir(parents=True, exist_ok=True)
example = json.loads(re.search(r"```json\n(.*?)\n```", (ROOT / "docs/file-format.md").read_text(), re.S)[1])
template = {kind: [item for item in example[kind] if item.get("in") == 20]
            for kind in ("vertices", "edges", "faces")}
for shared in (True, False):
    model = dict(format="rysmo", version=20, length_unit="m", display_units="m",
                 definitions=[], instances=[], vertices=[], edges=[], faces=[])
    for i in range(args.count):
        offset = 1000 + (0 if shared else i) * 100
        definition = offset + 20
        model["instances"].append(dict(id=10_000_000 + i, **{"def": definition},
                                       o=[(i % 100) * 2, (i // 100) * 2, 0],
                                       x=[1, 0, 0], y=[0, 1, 0], z=[0, 0, 1]))
        if shared and i:
            continue
        model["definitions"].append(dict(id=definition, name="Cube"))
        for kind, entries in template.items():
            for original in entries:
                entry = dict(original, id=offset + original["id"], **{"in": definition})
                # Use unpainted geometry so the fixture needs no material/image data.
                entry.pop("material", None)
                entry.pop("back_material", None)
                for key in ("outer", "v"):
                    if key in entry:
                        entry[key] = [offset + vertex for vertex in entry[key]]
                model[kind].append(entry)
    path = args.output / ("shared.rysmo" if shared else "unique.rysmo")
    path.write_text(json.dumps(model, separators=(",", ":")), encoding="utf-8")
    print(f"{path}: {path.stat().st_size} bytes, {args.count} instances")

if args.raw:
    model = dict(format="rysmo", version=20, length_unit="m", display_units="m",
                 definitions=[], instances=[], vertices=[], edges=[], faces=[])
    for i in range(args.count):
        offset = 1000 + i * 100
        for kind, entries in template.items():
            for original in entries:
                entry = dict(original, id=offset + original["id"])
                for key in ("in", "material", "back_material"):
                    entry.pop(key, None)
                for key in ("outer", "v"):
                    if key in entry:
                        entry[key] = [offset + vertex for vertex in entry[key]]
                if kind == "vertices":
                    x, y, z = entry["p"]
                    entry["p"] = [x + (i % 100) * 2, y + (i // 100) * 2, z]
                model[kind].append(entry)
    path = args.output / "raw.rysmo"
    path.write_text(json.dumps(model, separators=(",", ":")), encoding="utf-8")
    print(f"{path}: {path.stat().st_size} bytes, {args.count * len(template['faces'])} root faces")
