"""An updated stable snippet must invalidate its importing module and HTML URL."""
import base64
import hashlib
import json
import importlib.util
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

if __name__ == "__main__":
    unittest.main()
