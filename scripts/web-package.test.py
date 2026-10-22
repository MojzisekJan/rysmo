"""An updated stable snippet must invalidate its importing module and HTML URL."""
import base64
import hashlib
import json
import importlib.util
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("package_web", Path(__file__).with_name("package-web.py"))
package_web = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package_web)

class ModuleCacheTest(unittest.TestCase):
    def test_unpacked_source_can_be_packaged_without_git_and_detects_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text("[workspace]\n")
            digest = hashlib.sha256((root / "Cargo.toml").read_bytes()).hexdigest()
            (root / "SOURCE-MANIFEST.json").write_text(json.dumps({"revision":"abc", "files":{"Cargo.toml":digest}}))
            self.assertEqual(package_web.source_inputs(root), (["Cargo.toml"], "abc", False))
            (root / "Cargo.toml").write_text("changed")
            self.assertEqual(package_web.source_inputs(root), (["Cargo.toml"], "abc", True))
            (root / "SOURCE-MANIFEST.json").write_text(json.dumps({"revision":"abc", "files":{"../outside":"x"}}))
            with self.assertRaises(SystemExit):
                package_web.source_inputs(root)

    def test_source_archive_keeps_build_assets_and_test_models(self):
        for name in ["Cargo.toml", "Cargo.lock", "crates/rysmo_model/fonts/DejaVuSans.ttf", "crates/rysmo_app/brand/logo.png", "testdata/text-preview.rysmo", "docs/file-format.md", "scripts/build-web.sh", "licenses/font.txt"]:
            self.assertTrue(package_web.include_source_file(name), name)
        for name in ["docs/reference/external-manual.pdf", "docs/verification/web-render-textured-100.jpg", "docs/verification/texture-preview.png"]:
            self.assertFalse(package_web.include_source_file(name), name)

    def test_snippet_changes_update_the_full_import_chain_and_are_idempotent(self):
        with tempfile.TemporaryDirectory() as root:
            dist = Path(root)
            snippet = dist / "snippets/app/src/web_files.js"
            snippet.parent.mkdir(parents=True)
            snippet.write_text("export function save() { return 1; }\n")
            (dist / "rysmo-original.js").write_text("import { save } from './snippets/app/src/web_files.js';\nconst imports = {'./snippets/app/src/web_files.js': save};\n")
            index = dist / "index.html"
            index.write_text('<script type="module" src="./rysmo-original.js"></script><link rel="modulepreload" href="./rysmo-original.js" integrity="sha384-old">')
            previous_name = None
            for value in [1, 2]:
                snippet.write_text(f"export function save() {{ return {value}; }}\n")
                package_web.fingerprint_local_modules(dist)
                scripts = list(dist.glob("rysmo-*.js"))
                self.assertEqual(len(scripts), 1)
                script = scripts[0]
                expected = hashlib.sha256(snippet.read_bytes()).hexdigest()[:16]
                self.assertIn("web_files.js?v=" + expected, script.read_text())
                self.assertIn("{'./snippets/app/src/web_files.js': save}", script.read_text())
                self.assertIn(script.name, index.read_text())
                sri = base64.b64encode(hashlib.sha384(script.read_bytes()).digest()).decode()
                self.assertIn("sha384-" + sri, index.read_text())
                self.assertNotEqual(previous_name, script.name)
                previous_name = script.name
                snapshot = (script.read_bytes(), index.read_bytes())
                package_web.fingerprint_local_modules(dist)
                self.assertEqual(snapshot, (script.read_bytes(), index.read_bytes()))
            self.assertNotIn("original.js", index.read_text())

class BuiltModuleTest(unittest.TestCase):
    @unittest.skipUnless(os.environ.get("NODE") or shutil.which("node"), "Node.js is required for module validation")
    def test_built_module_checks_detect_missing_exports_and_corrupt_assets(self):
        node = os.environ.get("NODE") or shutil.which("node")
        checker = Path(__file__).with_name("check-web-dist.mjs").resolve()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "snippets").mkdir()
            snippet = b"export function good() {}\n"
            digest = hashlib.sha256(snippet).hexdigest()[:16]
            empty_wasm = b"\0asm\x01\0\0\0"
            def prepare(imports, wasm=empty_wasm):
                (root / "snippets/a.js").write_bytes(snippet)
                (root / "rysmo-abc.js").write_text(imports + f" from './snippets/a.js?v={digest}';\n")
                (root / "app.wasm").write_bytes(wasm)
                (root / "index.html").write_text(f'<meta content="{len(wasm)}"><script src="rysmo-abc.js"></script>')
                assets = {p.relative_to(root).as_posix(): {"bytes": p.stat().st_size, "sha256": hashlib.sha256(p.read_bytes()).hexdigest()}
                          for p in root.rglob("*") if p.is_file() and p.name != "build-info.json"}
                (root / "build-info.json").write_text(json.dumps({"revision": "fixture", "tracked_changes": False, "assets": assets}))
            def check():
                return subprocess.run([node, "--experimental-vm-modules", str(checker), str(root)], text=True, capture_output=True, timeout=20)
            prepare("import { good }")
            result = check()
            self.assertEqual(result.returncode, 0, result.stderr)
            prepare("import { missing }")
            result = check()
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("does not provide an export", result.stderr)
            # A direct WASM namespace import bypasses named JavaScript import checks.
            module, name = b"./snippets/a.js", b"missing"
            payload = b"\x01" + bytes([len(module)]) + module + bytes([len(name)]) + name + b"\0\0"
            wasm = empty_wasm + b"\x01\x04\x01\x60\0\0" + b"\x02" + bytes([len(payload)]) + payload
            prepare("import * as namespace", wasm)
            result = check()
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Missing WASM import", result.stderr)
            prepare("import { good }")
            (root / "snippets/a.js").write_text("corrupted")
            result = check()
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Wrong size", result.stderr)

if __name__ == "__main__":
    unittest.main()
