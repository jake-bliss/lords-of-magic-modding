#!/usr/bin/env python3
"""Serve the side-by-side image review page and persist verdicts to disk.

    python3 tools/review-server.py DATA_DIR --page tools/portrait-review [--port N]
    python3 tools/review-server.py artifacts/sprite-review --page tools/sprite-review --port 8778

DATA_DIR must contain `manifest.json` plus an `images/` directory. `--page` is the directory holding
the `index.html` to serve; the two review pages share this server because both want the same three
things -- a manifest, images, and verdicts that survive the tab closing.

The code lives under `tools/` and the data under `artifacts/` on purpose: `artifacts/` is gitignored,
so the game's own images never reach the repository, while the pages and this server -- which are
ours -- stay version controlled.

**Why a server rather than opening the .html directly.** On a `file://` origin browsers give the
page an opaque origin, so `localStorage` either throws or silently isolates per load. A review of
several hundred images would be lost with no warning. Served over http://127.0.0.1 the storage is
real, and every verdict is ALSO written straight to `verdicts.json`, so closing the tab cannot lose
work and the results can be read back with no export step.

Bound to 127.0.0.1. These are the game's own assets; nothing here should be reachable off the
machine, which is also why the review page is not published as a hosted artifact.
"""
from __future__ import annotations

import argparse
import http.server
import json
import os
import pathlib
import socketserver
import threading

HERE = pathlib.Path(__file__).resolve().parent
LOCK = threading.Lock()


class Handler(http.server.SimpleHTTPRequestHandler):
    data_dir: pathlib.Path = HERE
    page_dir: pathlib.Path = HERE

    def __init__(self, *a, **kw):
        # The page comes from page_dir; images/manifest/verdicts come from data_dir, routed
        # explicitly below.
        super().__init__(*a, directory=str(type(self).page_dir), **kw)

    def log_message(self, *a):
        pass

    @property
    def verdict_file(self) -> pathlib.Path:
        return self.data_dir / "verdicts.json"

    def load(self) -> dict:
        path = self.verdict_file
        if path.exists():
            try:
                return json.loads(path.read_text())
            except json.JSONDecodeError:
                # A half-written file must never wipe a review in progress. Keep it aside.
                path.rename(path.with_suffix(".corrupt.json"))
        return {}

    def send_json(self, payload, status=200):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def send_file(self, path: pathlib.Path, content_type: str):
        if not path.is_file():
            self.send_error(404)
            return
        body = path.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/verdicts":
            self.send_json(self.load())
        elif self.path == "/manifest.json":
            self.send_file(self.data_dir / "manifest.json", "application/json")
        elif self.path.startswith("/images/"):
            name = pathlib.PurePosixPath(self.path).name          # no traversal out of images/
            self.send_file(self.data_dir / "images" / name, "image/png")
        else:
            super().do_GET()

    def do_POST(self):
        if self.path != "/verdict":
            self.send_error(404)
            return
        try:
            payload = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        except (json.JSONDecodeError, ValueError):
            self.send_error(400)
            return
        with LOCK:
            data = self.load()
            pid = payload.get("id")
            if payload.get("verdict") is None:
                data.pop(pid, None)
            else:
                data[pid] = payload["verdict"]
            # Atomic against a process crash via os.replace, and fsync'd so a power loss cannot
            # leave the rename visible with the contents missing. The reviewer may have hundreds of
            # verdicts in here; the cost of two fsyncs per keystroke is irrelevant next to that.
            tmp = self.verdict_file.with_suffix(".tmp")
            with open(tmp, "w") as handle:
                json.dump(data, handle, indent=1, sort_keys=True)
                handle.flush()
                os.fsync(handle.fileno())
            tmp.replace(self.verdict_file)
            directory = os.open(self.data_dir, os.O_RDONLY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        self.send_response(204)
        self.end_headers()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("data_dir")
    parser.add_argument("--page", type=pathlib.Path, default=HERE / "portrait-review",
                        help="directory holding the index.html to serve")
    parser.add_argument("--port", type=int, default=8777)
    args = parser.parse_args()

    data_dir = pathlib.Path(args.data_dir).resolve()
    manifest = data_dir / "manifest.json"
    if not manifest.is_file():
        parser.error(f"no manifest.json in {data_dir}")
    count = len(json.loads(manifest.read_text()))

    page_dir = args.page.resolve()
    if not (page_dir / "index.html").is_file():
        parser.error(f"no index.html in {page_dir}")
    Handler.data_dir = data_dir
    Handler.page_dir = page_dir
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("127.0.0.1", args.port), Handler) as httpd:
        print(f"review {count} pairs at http://127.0.0.1:{args.port}/")
        print(f"verdicts persist to {data_dir / 'verdicts.json'}")
        httpd.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
