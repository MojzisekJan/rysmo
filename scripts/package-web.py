#!/usr/bin/env python3
"""Validate and package the current Trunk output for an itch.io HTML5 upload."""
import base64
import gzip
import hashlib
import json
import io
import re
import subprocess
import tarfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "crates/rysmo_app/dist"
OUT = ROOT / "target/web"

def fingerprint_local_modules(dist):
    """Bind wasm-bindgen's stable snippet URLs to their actual source contents."""
    scripts = list(dist.glob("rysmo-*.js"))
    if len(scripts) != 1:
        raise SystemExit("Expected exactly one generated JavaScript module.")
    script = scripts[0]
    original = script.read_text()
    pattern = r"(?P<quote>['\"])(?P<path>\./snippets/[^'\"?]+\.js)(?:\?v=[0-9a-f]+)?(?P=quote)"

    def version(match):
        module = dist / match.group("path")
        if not module.is_file():
            raise SystemExit(f"Missing imported JavaScript module: {module}")
        digest = hashlib.sha256(module.read_bytes()).hexdigest()[:16]
        quote = match.group("quote")
        return f"{quote}{match.group('path')}?v={digest}{quote}"

    # The import-object keys are part of the WASM ABI and must stay unchanged.
    normalized = re.sub(pattern, lambda m: m.group("quote") + m.group("path") + m.group("quote"), original)
    rewritten = re.sub(r"(?m)^(import[^\n]*?from\s+)([^\n]+)",
                       lambda m: m.group(1) + re.sub(pattern, version, m.group(2)), normalized)
    name = "rysmo-" + hashlib.sha256(rewritten.encode()).hexdigest()[:16] + ".js"
    if name != script.name:
        (dist / name).write_text(rewritten)
        index = dist / "index.html"
        index.write_text(index.read_text().replace(script.name, name))
        script.unlink()
    elif rewritten != original:
        script.write_text(rewritten)
    index = dist / "index.html"
    html = re.sub(pattern, version, index.read_text())
    def integrity(match):
        tag = match.group(0)
        href = re.search(r'href="([^"?]+)(?:\?[^" ]*)?"', tag)
        if href:
            asset = dist / href.group(1)
            if asset.is_file():
                digest = base64.b64encode(hashlib.sha384(asset.read_bytes()).digest()).decode()
                tag = re.sub(r'integrity="[^"]*"', 'integrity="sha384-' + digest + '"', tag)
        return tag
    html = re.sub(r'<link\b[^>]*>', integrity, html)
    index.write_text(html)


def include_source_file(name):
    """Keep build/test sources and assets; omit reference PDFs and QA media."""
    path = Path(name)
    if path.parts[:2] == ("docs", "reference") and path.suffix.lower() == ".pdf":
        return False
    if path.parts[:2] == ("docs", "verification") and path.suffix.lower() in {".png", ".jpg", ".jpeg", ".webp", ".gif", ".mp4"}:
        return False
    return True


def published_files():
    return [p for p in sorted(DIST.rglob("*")) if p.is_file() and not any(part.startswith(".") for part in p.relative_to(DIST).parts)]

def source_inputs(root=ROOT):
    manifest = root / "SOURCE-MANIFEST.json"
    if not (root / ".git").exists() and manifest.is_file():
        data = json.loads(manifest.read_text())
        files = sorted(data["files"])
        for name in files:
            path = Path(name)
            if path.is_absolute() or ".." in path.parts:
                raise SystemExit("Invalid source manifest path")
        dirty = any(not (root / name).is_file() or hashlib.sha256((root / name).read_bytes()).hexdigest() != data["files"][name] for name in files)
        return files, data["revision"], dirty
    files = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).decode().split("\0")
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    dirty = subprocess.run(["git", "diff", "--quiet", "HEAD", "--"], cwd=root).returncode != 0
    return sorted(filter(None, files)), revision, dirty


def main():
    index = DIST / "index.html"
    if not index.is_file():
        raise SystemExit("Missing dist/index.html; run scripts/build-web.sh first.")
    fingerprint_local_modules(DIST)
    html = index.read_text()
    for url in re.findall(r'(?:src|href)=["\']([^"\']+)', html):
        if url.startswith(("https:", "http:", "data:", "#")):
            continue
        if url.startswith("/"):
            raise SystemExit(f"Absolute asset path is unsafe for an itch.io subdirectory: {url}")
        path = DIST / url.split("?", 1)[0]
        if not path.is_file():
            raise SystemExit(f"Missing page asset: {url}")
    wasm = list(DIST.glob("*.wasm"))
    if len(wasm) != 1:
        raise SystemExit("Expected exactly one generated WASM module.")
    with wasm[0].open("rb") as stream:
        if stream.read(8) != b"\0asm\x01\0\0\0":
            raise SystemExit("Invalid WASM header in build output.")
    # The loader can show the optimized module size without a separate HEAD request.
    size_meta = f'<meta name="rysmo-wasm-bytes" content="{wasm[0].stat().st_size}">'
    html = re.sub(r'<meta name="rysmo-wasm-bytes" content="[0-9]+">', '', html)
    index.write_text(html.replace('</head>', size_meta + '</head>'))
    OUT.mkdir(parents=True, exist_ok=True)
    files, revision, dirty = source_inputs()
    files = [name for name in files if include_source_file(name) and (ROOT / name).is_file()]
    source_manifest = {"revision": revision, "files": {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in files}}
    source = DIST / "source.tar.gz"
    with tarfile.open(source, "w:gz") as archive:
        for name in files:
            path = ROOT / name
            if path.is_file() and include_source_file(name):
                archive.add(path, arcname=f"rysmo/{name}", recursive=False)
        manifest_bytes = json.dumps(source_manifest, indent=2).encode() + b"\n"
        info = tarfile.TarInfo("rysmo/SOURCE-MANIFEST.json")
        info.size = len(manifest_bytes)
        archive.addfile(info, io.BytesIO(manifest_bytes))
    (DIST / "LICENSE").write_bytes((ROOT / "LICENSE").read_bytes())
    (DIST / "SOURCE.txt").write_text("Rysmo source code and build instructions are in source.tar.gz.\nLicense: GPL-3.0-or-later; see LICENSE.\nReference PDFs and QA screenshots are omitted; build assets and test models are included.\n")
    assets = {str(p.relative_to(DIST)): {"bytes": p.stat().st_size, "sha256": hashlib.sha256(p.read_bytes()).hexdigest()} for p in published_files() if p.name != "build-info.json"}
    manifest = {"revision": revision, "tracked_changes": dirty, "assets": assets}
    (DIST / "build-info.json").write_text(json.dumps(manifest, indent=2) + "\n")
    target = OUT / "rysmo-itch.zip"
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for path in published_files():
            archive.write(path, path.relative_to(DIST).as_posix())
    with zipfile.ZipFile(target) as archive:
        assert "index.html" in archive.namelist()
        assert archive.testzip() is None
    report = {"zip": str(target), "zip_bytes": target.stat().st_size, "source_archive_bytes": source.stat().st_size, "wasm_bytes": wasm[0].stat().st_size, "wasm_gzip_bytes": len(gzip.compress(wasm[0].read_bytes(), compresslevel=9)), "revision": revision, "tracked_changes": dirty}
    (OUT / "build-report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))

if __name__ == "__main__":
    main()
