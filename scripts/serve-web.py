#!/usr/bin/env python3
"""Serve a local web build without retaining outdated generated assets in cache."""
import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

class DevelopmentHandler(SimpleHTTPRequestHandler):
    def send_head(self):
        # Trunk can retain the source HTML mtime while its generated URLs change.
        for name in ("If-Modified-Since", "If-None-Match"):
            if name in self.headers:
                del self.headers[name]
        return super().send_head()

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", nargs="?", type=Path, default=Path(__file__).resolve().parents[1] / "crates/rysmo_app/dist")
    parser.add_argument("--port", type=int, default=8098)
    args = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", args.port), partial(DevelopmentHandler, directory=str(args.directory.resolve())))
    print(f"Serving {args.directory.resolve()} at http://127.0.0.1:{args.port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
