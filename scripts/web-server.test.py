"""Regression for generated HTML whose source mtime outlives its asset names."""
import http.client
import importlib.util
import os
import sys
import tempfile
import threading
import unittest
from functools import partial
from http.server import ThreadingHTTPServer
from pathlib import Path

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("serve_web", Path(__file__).with_name("serve-web.py"))
serve_web = importlib.util.module_from_spec(spec)
spec.loader.exec_module(serve_web)

class QuietHandler(serve_web.DevelopmentHandler):
    def log_message(self, *_):
        pass

class GeneratedAssetsTest(unittest.TestCase):
    def test_html_changes_are_not_hidden_by_an_unchanged_mtime(self):
        with tempfile.TemporaryDirectory() as root:
            page = Path(root) / "index.html"
            page.write_text('<script src="old.js"></script>')
            timestamp = page.stat().st_mtime
            server = ThreadingHTTPServer(("127.0.0.1", 0), partial(QuietHandler, directory=root))
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                conn = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=3)
                conn.request("GET", "/index.html")
                initial = conn.getresponse()
                modified = initial.getheader("Last-Modified")
                initial.read()
                page.write_text('<script src="new.js"></script>')
                os.utime(page, (timestamp, timestamp))
                for method in ("GET", "HEAD"):
                    conn.request(method, "/index.html", headers={"If-Modified-Since": modified})
                    response = conn.getresponse()
                    self.assertEqual(response.status, 200)
                    self.assertEqual(response.getheader("Cache-Control"), "no-store")
                    body = response.read()
                    if method == "GET":
                        self.assertIn(b"new.js", body)
                conn.close()
            finally:
                server.shutdown()
                server.server_close()
                thread.join()

if __name__ == "__main__":
    unittest.main()
