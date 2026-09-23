#!/usr/bin/env python3
"""Render every upscale option for every overlay image, for review side by side.

    python3 tools/hd-review/render_variants.py SRC_DIR OUT_DIR --esrgan PATH --models DIR

SRC_DIR is an extracted pic.mpq tree. OUT_DIR gets, per image, `original/` plus one folder per
option, all as PNG named `<group>__<member>.png`. Everything written is derived from the game's
art: keep OUT_DIR under the gitignored `artifacts/`.

The options, chosen 2026-09-22 from side-by-side sheets: the palette-constrained pipeline in
tools/portrait-upscale lost detail twice over (despeckle before, a dithered 256-colour remap after),
and the overlay draws full colour, so none of these do either:

  ultrasharp      4x-UltraSharp, shrunk to 2x
  ultrasharp-tta  the same with test-time augmentation (8 flipped/rotated passes averaged)
  anime2x         realesr-animevideov3 at its native 2x
  anime4x         realesr-animevideov3 at 4x, shrunk to 2x

Resumable: only images without an output are rendered. Each model runs once per batch over a
folder, not once per image.
"""
from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys

TOOLS = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(TOOLS))
sys.path.insert(0, str(TOOLS / "portrait-upscale"))
import hd_upscale  # noqa: E402  -- the options themselves, shared with the player's setup
import lbm_png  # noqa: E402

GROUPS = {                       # group -> directory in pic.mpq, and the size every member has
    "portrait": ("portrait", (70, 67)),
    "building": ("lbm/building", None),
}
def find_dir(root: pathlib.Path, rel: str) -> pathlib.Path | None:
    """pic.mpq spells directories in both cases (PORTRAIT\\ and portrait\\)."""
    here = root
    for part in rel.split("/"):
        match = [p for p in here.iterdir() if p.is_dir() and p.name.lower() == part]
        if not match:
            return None
        here = match[0]
    return here


def write_originals(src: pathlib.Path, out: pathlib.Path) -> list[str]:
    originals = out / "original"
    originals.mkdir(parents=True, exist_ok=True)
    keys = []
    for group, (rel, size) in GROUPS.items():
        folder = find_dir(src, rel)
        if folder is None:
            continue
        for lbm in sorted(folder.iterdir(), key=lambda p: p.name.lower()):
            if lbm.suffix.lower() != ".lbm":
                continue
            w, h, px, pal, _ = lbm_png.decode(lbm)
            if size and (w, h) != size:
                continue
            key = f"{group}__{lbm.stem.lower()}"
            if key in keys:
                continue                       # the same member under both spellings
            keys.append(key)
            png = originals / f"{key}.png"
            if not png.exists():
                ppm = png.with_suffix(".ppm")
                ppm.write_bytes(f"P6 {w} {h} 255\n".encode() + b"".join(bytes(pal[i]) for i in px))
                subprocess.run(["magick", str(ppm), str(png)], check=True)
                ppm.unlink()
    return keys


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("src", type=pathlib.Path)
    parser.add_argument("out", type=pathlib.Path)
    parser.add_argument("--esrgan", type=pathlib.Path, required=True)
    parser.add_argument("--models", type=pathlib.Path, required=True)
    parser.add_argument("--only", choices=sorted(hd_upscale.OPTIONS), action="append")
    args = parser.parse_args()

    keys = write_originals(args.src, args.out)
    print(f"{len(keys)} images", flush=True)
    inputs = {k: args.out / "original" / f"{k}.png" for k in keys}
    for option in args.only or hd_upscale.OPTIONS:
        n = hd_upscale.render(option, inputs, args.out / option, args.esrgan, args.models)
        print(f"{option}: {n} rendered", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
