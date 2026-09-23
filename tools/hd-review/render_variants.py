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
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "portrait-upscale"))
import lbm_png  # noqa: E402

GROUPS = {                       # group -> directory in pic.mpq, and the size every member has
    "portrait": ("portrait", (70, 67)),
    "building": ("lbm/building", None),
}
OPTIONS = {                      # name -> (model, scale, extra esrgan args)
    "ultrasharp": ("ultrasharp-4x", 4, []),
    "anime2x": ("realesr-animevideov3-x2", 2, []),
    "anime4x": ("realesr-animevideov3-x4", 4, []),
    "ultrasharp-tta": ("ultrasharp-4x", 4, ["-x"]),     # last: about 8x slower than the rest
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


def render(option: str, keys: list[str], out: pathlib.Path, esrgan: pathlib.Path,
           models: pathlib.Path) -> int:
    model, scale, extra = OPTIONS[option]
    dest = out / option
    dest.mkdir(exist_ok=True)
    todo = [k for k in keys if not (dest / f"{k}.png").exists()]
    if not todo:
        return 0
    with tempfile.TemporaryDirectory() as tmp:
        stage, raw = pathlib.Path(tmp) / "in", pathlib.Path(tmp) / "out"
        stage.mkdir(); raw.mkdir()
        for k in todo:
            shutil.copy(out / "original" / f"{k}.png", stage / f"{k}.png")
        subprocess.run([str(esrgan), "-i", str(stage), "-o", str(raw), "-n", model, "-m", str(models),
                        "-s", str(scale), "-f", "png", *extra], check=True, capture_output=True)
        for k in todo:
            got = raw / f"{k}.png"
            if not got.exists():
                raise SystemExit(f"{option}: no output for {k}")
            if scale == 4:
                # Exactly 2x the original, whatever rounding the model applied.
                w, h = png_size(out / "original" / f"{k}.png")
                subprocess.run(["magick", str(got), "-filter", "MagicKernelSharp2021",
                                "-resize", f"{w * 2}x{h * 2}!", str(dest / f"{k}.png")], check=True)
            else:
                shutil.move(str(got), dest / f"{k}.png")
    return len(todo)


def png_size(path: pathlib.Path) -> tuple[int, int]:
    data = path.read_bytes()[16:24]
    return int.from_bytes(data[:4], "big"), int.from_bytes(data[4:], "big")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("src", type=pathlib.Path)
    parser.add_argument("out", type=pathlib.Path)
    parser.add_argument("--esrgan", type=pathlib.Path, required=True)
    parser.add_argument("--models", type=pathlib.Path, required=True)
    parser.add_argument("--only", choices=sorted(OPTIONS), action="append")
    args = parser.parse_args()

    keys = write_originals(args.src, args.out)
    print(f"{len(keys)} images", flush=True)
    for option in args.only or OPTIONS:
        n = render(option, keys, args.out, args.esrgan, args.models)
        print(f"{option}: {n} rendered", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
