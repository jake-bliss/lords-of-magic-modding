#!/usr/bin/env python3
"""Local review page: pick the best upscale option for each overlay image.

    python3 tools/hd-review/serve.py [--renders artifacts/hd-review] [--port 8765]
                                     [--choices FILE] [--seed FILE]

Then open http://127.0.0.1:8765. Players reach it through `lomhd_setup.py --review`, which renders
their own game's art and passes --choices my-upscale-choices.json, seeded from the shipped picks. Renders come from tools/hd-review/render_variants.py and stay on
this machine -- they are the game's art, so the page is served locally and never published.

Picks are written to --choices (by default release/hd-overlay/upscale-choices.json, the shipped
defaults) after every click: member names and option names only, no art, so the file is safe to
commit. If --choices does not exist yet it starts as a copy of --seed. Setup reads it to run the
chosen model per image.
"""
from __future__ import annotations

import argparse
import http.server
import json
import os
import pathlib
import tempfile
import threading
import urllib.parse

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
CHOICES = ROOT / "release" / "hd-overlay" / "upscale-choices.json"
OPTIONS = ["ultrasharp", "ultrasharp-tta", "anime2x", "anime4x"]
# "approved": the original palette pipeline (despeckle, UltraSharp, remap to the image's own 256
# colours) -- kept for character portraits, where it was reviewed and approved on 2026-09-22.
VALID = set(OPTIONS) | {"original", "approved"}
SAVING = threading.Lock()        # the server is threaded; two quick picks must not interleave


def load_choices() -> dict[str, str]:
    try:
        return json.loads(Handler.choices.read_text())["choices"]
    except (OSError, ValueError, KeyError):
        return {}


def save_choices(choices: dict[str, str]) -> None:
    body = json.dumps({"version": 1, "options": OPTIONS, "choices": dict(sorted(choices.items()))},
                      indent=1) + "\n"
    target = Handler.choices
    fd, part = tempfile.mkstemp(dir=target.parent, prefix=target.name, suffix=".part")
    with os.fdopen(fd, "w") as f:
        f.write(body)
    os.replace(part, target)


class Handler(http.server.BaseHTTPRequestHandler):
    renders: pathlib.Path
    choices: pathlib.Path = CHOICES

    def options(self) -> list[str]:
        """The approved palette pipeline is an option only where it was rendered: setup renders
        it for character portraits, the repo's review never did."""
        return OPTIONS + (["approved"] if (self.renders / "approved").is_dir() else [])

    def log_message(self, *args) -> None:  # quiet
        pass

    def send(self, code: int, body: bytes, kind: str) -> None:
        self.send_response(code)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "max-age=3600" if kind == "image/png" else "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        path = urllib.parse.urlparse(self.path).path
        if path in ("/", "/index.html"):
            return self.send(200, (HERE / "review.html").read_bytes(), "text/html; charset=utf-8")
        if path == "/api/state":
            originals = sorted((self.renders / "original").glob("*.png"))
            options, images = self.options(), []
            for png in originals:
                key = png.stem
                images.append({"key": key, "group": key.split("__", 1)[0],
                               "ready": [o for o in options if (self.renders / o / png.name).exists()]})
            state = {"options": options, "images": images, "choices": load_choices()}
            return self.send(200, json.dumps(state).encode(), "application/json")
        if path.startswith("/img/"):
            rel = pathlib.PurePosixPath(urllib.parse.unquote(path[5:]))
            if (len(rel.parts) != 2 or rel.parts[0] not in VALID or rel.suffix != ".png"
                    or "\\" in rel.parts[1] or rel.parts[1].startswith(".")):
                return self.send(404, b"", "text/plain")
            file = (self.renders / rel.parts[0] / rel.parts[1]).resolve()
            if file.is_relative_to(self.renders) and file.is_file():
                return self.send(200, file.read_bytes(), "image/png")
        self.send(404, b"not found", "text/plain")

    def do_POST(self) -> None:
        if urllib.parse.urlparse(self.path).path != "/api/choose":
            return self.send(404, b"", "text/plain")
        # Only this page may pick. Another site open in the browser can post to localhost; a
        # JSON content type forces a CORS preflight it cannot pass, and the Origin must be ours.
        origin = self.headers.get("Origin")
        if (self.headers.get("Content-Type", "").split(";")[0] != "application/json"
                or origin not in (None, *(f"http://{host}:{self.server.server_port}"
                                          for host in ("127.0.0.1", "localhost")))):
            return self.send(403, b"forbidden", "text/plain")
        known = {png.stem for png in (self.renders / "original").glob("*.png")}
        try:
            body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
            keys, choice = body["keys"], body["choice"]
            assert isinstance(keys, list) and all(isinstance(k, str) and k in known for k in keys)
            assert choice in VALID or choice is None
        except (ValueError, KeyError, TypeError, AssertionError):
            return self.send(400, b"bad request", "text/plain")
        with SAVING:
            choices = load_choices()
            for key in keys:
                if choice is None:
                    choices.pop(key, None)
                else:
                    choices[key] = choice
            save_choices(choices)
        self.send(200, json.dumps({"picked": len(choices)}).encode(), "application/json")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--renders", type=pathlib.Path, default=ROOT / "artifacts" / "hd-review")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--choices", type=pathlib.Path, default=CHOICES,
                        help="where picks are saved (default: the shipped defaults in the repo)")
    parser.add_argument("--seed", type=pathlib.Path,
                        help="copied to --choices first when --choices does not exist yet")
    args = parser.parse_args()
    Handler.renders = args.renders.resolve()
    Handler.choices = args.choices.resolve()
    if not Handler.choices.exists() and args.seed:
        Handler.choices.write_text(args.seed.read_text())
    server = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"http://127.0.0.1:{args.port}  (picks saved to {Handler.choices})", flush=True)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
