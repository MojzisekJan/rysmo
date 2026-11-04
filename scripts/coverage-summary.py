#!/usr/bin/env python3
"""Summarize cargo-llvm-cov JSON without treating unmeasured branches as covered."""
import argparse
import json
from collections import defaultdict
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("report", type=Path)
parser.add_argument("--snapshot", type=Path)
args = parser.parse_args()
data = json.loads(args.report.read_text())["data"][0]
crates = defaultdict(lambda: {"lines": {"covered": 0, "count": 0}, "functions": {"covered": 0, "count": 0}})
for entry in data["files"]:
    name = entry["filename"].replace("\\", "/")
    if "/crates/" not in name:
        continue
    crate = name.split("/crates/", 1)[1].split("/", 1)[0]
    for metric in ("lines", "functions"):
        for field in ("covered", "count"):
            crates[crate][metric][field] += entry["summary"][metric][field]

def percent(metric):
    return f'{100 * metric["covered"] / metric["count"]:.2f}%' if metric["count"] else "not measured"

lines = ["# Native Rust coverage", "", "| Crate | Lines | Functions |", "|---|---:|---:|"]
for crate, metrics in sorted(crates.items()):
    lines.append(f'| {crate} | {percent(metrics["lines"])} | {percent(metrics["functions"])} |')
lines.extend([f'| **Total** | **{percent(data["totals"]["lines"])}** | **{percent(data["totals"]["functions"])}** |', "",
              "Native tests only. Browser-only Rust, GPU execution and other OS targets are outside this report.",
              "Separate test files, examples and vendored code are excluded; inline test modules remain included.",
              "Branch coverage is not measured by this stable-toolchain run.", ""])
text = "\n".join(lines)
print(text)
args.report.with_name("summary.md").write_text(text)
if args.snapshot:
    args.snapshot.write_text(json.dumps({"tool": "cargo-llvm-cov 0.9.1", "scope": sorted(crates),
        "native_only": True, "inline_tests_included": True, "branches_measured": False,
        "totals": data["totals"], "crates": dict(sorted(crates.items()))}, indent=2) + "\n")
