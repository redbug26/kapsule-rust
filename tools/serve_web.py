#!/usr/bin/env python3
"""Local static preview with the publication CSP and cross-origin-isolation headers."""
import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import shlex

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--port", type=int, default=8080)
parser.add_argument("--directory", type=Path, default=ROOT / "dist/web")
args = parser.parse_args()
headers = {}
for line in (args.directory / ".htaccess").read_text().splitlines():
    words = shlex.split(line)
    if words[:3] == ["Header", "always", "set"] and len(words) == 5:
        headers[words[3]] = words[4]


class Handler(SimpleHTTPRequestHandler):
    def end_headers(self):
        for name, value in headers.items():
            self.send_header(name, value)
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()


server = ThreadingHTTPServer(("127.0.0.1", args.port), partial(Handler, directory=str(args.directory)))
print(f"Kapsule preview: http://localhost:{args.port}", flush=True)
try:
    server.serve_forever()
except KeyboardInterrupt:
    pass
finally:
    server.server_close()
