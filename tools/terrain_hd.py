#!/usr/bin/env python3
"""2x terrain atlases: every tile upscaled on its own, then quantized back to its atlas's palette.

    python3 tools/terrain_hd.py SRC_DIR OUT_DIR --esrgan PATH --models DIR [--work DIR]

SRC_DIR holds the game's `til\\*.lbm` and `til\\*.til` members. OUT_DIR receives the 2x atlases and
the .til files with TILESIZE doubled -- what `mods/terrain-hd-art` stages into pic.mpq.

**Per tile, not per sheet.** An atlas is a grid of independent Wang tiles: each `TILE=` line gives a
cell's corner and edge terrain types, and the map places cells by those types, never by where they
sit on the sheet. Upscaling the whole sheet lets the model blend each tile into whatever tile is
stored beside it, and the first staging (2026-09-23) did exactly that: 8-12% of the pixels in a
2-px ring around every tile took a colour that exists only in the NEIGHBOURING cell. On the map
that is a faint grid on every tile edge. Here each tile is padded by repeating its own edge pixels,
upscaled alone, and cropped, so nothing outside the tile can reach it.

**Quantized index-safely.** Texels go through the light tables by PALETTE INDEX, so the atlas must
stay 8-bit in its own palette. A 2x pixel may only take an index that occurs within one source
pixel of it AND inside the same tile, so the key, colour-cycling ranges and special indices never
spread to pixels that did not have them, and no index crosses a tile edge.

**Not every atlas.** `thite01`/`ttype01` are data maps read by coordinate (height and terrain type),
not textures; doubling them would double the map's lookups rather than its detail.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE / "portrait-upscale"))

import hd_upscale  # noqa: E402
import lbm_png  # noqa: E402

NOT_TEXTURES = {"thite01", "ttype01"}
PAD = 8
DEFAULT_TILE = 32
CHOICES = HERE.parent / "release" / "hd-overlay" / "upscale-choices.json"


def read_til(path: pathlib.Path) -> tuple[str, int]:
    """(atlas member name, tile size) from a .til. Only square tiles exist; anything else refuses."""
    text = path.read_bytes().decode("latin-1")
    lbm = re.search(r"^LBM=\s*(\S+)", text, re.M | re.I)
    size = re.search(r"TILESIZE=\s*(\d+),\s*(\d+)", text)
    if not lbm or not size:
        raise SystemExit(f"{path.name}: no LBM= or TILESIZE=")
    if size[1] != size[2]:
        raise SystemExit(f"{path.name}: TILESIZE {size[1]},{size[2]} is not square")
    return lbm[1].strip().lower(), int(size[1])


def doubled_til(data: bytes) -> bytes:
    new, n = re.subn(rb"TILESIZE=\s*(\d+),\s*(\d+)",
                     lambda m: b"TILESIZE= %d, %d" % (2 * int(m[1]), 2 * int(m[2])), data)
    if n != 1:
        raise SystemExit(f"expected one TILESIZE line, found {n}")
    return new


def tile_sizes(src: pathlib.Path) -> dict[str, int]:
    """Tile size per atlas, from every .til that names it. Two .til files disagreeing refuses."""
    sizes: dict[str, int] = {}
    for til in sorted(src.glob("*.til")):
        name, size = read_til(til)
        if sizes.setdefault(name, size) != size:
            raise SystemExit(f"{name}: tile size {size} in {til.name}, {sizes[name]} elsewhere")
    return sizes


def padded_tile(idx: bytes, w: int, pal, tx: int, ty: int, t: int, pad: int):
    """RGB rows of one tile with its own edge pixels repeated `pad` times outward."""
    rows = []
    for y in range(-pad, t + pad):
        sy = ty * t + min(max(y, 0), t - 1)
        rows.append([pal[idx[sy * w + tx * t + min(max(x, 0), t - 1)]] for x in range(-pad, t + pad)])
    return rows


def quantize(idx: bytes, w: int, h: int, pal, t: int, rgb: bytes) -> tuple[bytes, float]:
    """Each 2x pixel -> the nearest palette colour among indices within one source pixel of it,
    clamped to its own tile. Returns the indices and how often a block's top-left keeps its source
    index (a sanity reading: high, and not 100%)."""
    W = 2 * w
    out = bytearray(4 * w * h)
    for y in range(2 * h):
        sy = y // 2
        y0 = (sy // t) * t
        ys = range(max(y0, sy - 1), min(y0 + t, sy + 2))
        for x in range(W):
            sx = x // 2
            x0 = (sx // t) * t
            cand = {idx[yy * w + xx] for yy in ys for xx in range(max(x0, sx - 1), min(x0 + t, sx + 2))}
            p = (y * W + x) * 3
            r, g, b = rgb[p], rgb[p + 1], rgb[p + 2]
            out[y * W + x] = min(cand, key=lambda i: (pal[i][0] - r) ** 2 + (pal[i][1] - g) ** 2 + (pal[i][2] - b) ** 2)
    kept = sum(out[2 * y * W + 2 * x] == idx[y * w + x] for y in range(h) for x in range(w))
    return bytes(out), kept / (w * h)


def foreign_edge_pixels(idx: bytes, w: int, h: int, t: int, out: bytes) -> int:
    """2x pixels holding an index their own tile's source neighbourhood does not have. The per-tile
    pipeline makes this 0 by construction; it is measured anyway, because it is the defect."""
    W, bad = 2 * w, 0
    for y in range(2 * h):
        sy = y // 2
        y0 = (sy // t) * t
        for x in range(W):
            sx = x // 2
            x0 = (sx // t) * t
            own = {idx[yy * w + xx] for yy in range(max(y0, sy - 1), min(y0 + t, sy + 2))
                   for xx in range(max(x0, sx - 1), min(x0 + t, sx + 2))}
            bad += out[y * W + x] not in own
    return bad


def build(src: pathlib.Path, out: pathlib.Path, esrgan: pathlib.Path, models: pathlib.Path,
          work: pathlib.Path, choices: dict[str, str]) -> list[str]:
    out.mkdir(parents=True, exist_ok=True)
    sizes = tile_sizes(src)
    report = []
    for lbm in sorted(src.glob("*.lbm")):
        name = lbm.stem.lower()
        if name in NOT_TEXTURES:
            report.append(f"{name}: skipped, a data map, not a texture")
            continue
        option = choices.get(f"terrain__{name}")
        if option not in hd_upscale.OPTIONS:
            raise SystemExit(f"{name}: no reviewed full-colour choice ({option!r})")
        w, h, idx, pal, chunks = lbm_png.decode(lbm)
        pal = [tuple(c) for c in pal]
        t = sizes.get(f"{name}.lbm", DEFAULT_TILE)
        if w % t or h % t:
            raise SystemExit(f"{name}: {w}x{h} is not a whole number of {t}px tiles")
        tiles_dir = work / "tiles" / name
        tiles_dir.mkdir(parents=True, exist_ok=True)
        inputs = {}
        for ty in range(h // t):
            for tx in range(w // t):
                key = f"{name}_{tx:02d}_{ty:02d}"
                path = tiles_dir / f"{key}.png"
                if not path.exists():
                    lbm_png.write_png(path, t + 2 * PAD, t + 2 * PAD, padded_tile(idx, w, pal, tx, ty, t, PAD))
                inputs[key] = path
        rendered = work / "rendered" / option
        hd_upscale.render(option, inputs, rendered, esrgan, models)
        # Crop each tile's centre and lay it back on the 2x sheet.
        with tempfile.TemporaryDirectory() as tmp:
            sheet = pathlib.Path(tmp) / "sheet.png"
            args = ["magick", "-size", f"{2 * w}x{2 * h}", "xc:black"]
            for key in inputs:
                tx, ty = int(key[-5:-3]), int(key[-2:])
                args += ["(", str(rendered / f"{key}.png"), "-crop", f"{2 * t}x{2 * t}+{2 * PAD}+{2 * PAD}",
                         "+repage", ")", "-geometry", f"+{2 * t * tx}+{2 * t * ty}", "-composite"]
            args.append(str(sheet))
            subprocess.run(args, check=True)
            rgb = subprocess.check_output(["magick", str(sheet), "-depth", "8", "rgb:-"])
        if len(rgb) != 4 * w * h * 3:
            raise SystemExit(f"{name}: assembled sheet is {len(rgb)} bytes, not {12 * w * h}")
        indices, kept = quantize(idx, w, h, pal, t, rgb)
        foreign = foreign_edge_pixels(idx, w, h, t, indices)
        if foreign:
            raise SystemExit(f"{name}: {foreign} pixels took an index from another tile")
        dest = out / f"{name}.lbm"
        lbm_png.encode(dest, 2 * w, 2 * h, indices, pal, chunks)
        w2, h2, idx2, pal2, _ = lbm_png.decode(dest)
        if (w2, h2) != (2 * w, 2 * h) or bytes(idx2) != indices or [tuple(c) for c in pal2] != pal:
            raise SystemExit(f"{name}: written LBM does not read back")
        (work / "preview").mkdir(parents=True, exist_ok=True)
        lbm_png.write_png(work / "preview" / f"{name}.png", 2 * w, 2 * h,
                          [[pal[v] for v in indices[y * 2 * w:(y + 1) * 2 * w]] for y in range(2 * h)])
        report.append(f"{name}: {w}x{h} -> {2 * w}x{2 * h}, {t}px tiles, {option}; "
                      f"top-left keeps source index {kept:.0%}; foreign-tile pixels 0")
    for til in sorted(src.glob("*.til")):
        (out / til.name.lower()).write_bytes(doubled_til(til.read_bytes()))
        report.append(f"{til.name.lower()}: TILESIZE doubled")
    return report


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("src", type=pathlib.Path)
    ap.add_argument("out", type=pathlib.Path)
    ap.add_argument("--esrgan", type=pathlib.Path, required=True)
    ap.add_argument("--models", type=pathlib.Path, required=True)
    ap.add_argument("--work", type=pathlib.Path, help="tile PNGs and renders; reused on a rerun")
    ap.add_argument("--choices", type=pathlib.Path, default=CHOICES)
    args = ap.parse_args(argv)
    choices = json.loads(args.choices.read_text())["choices"]
    work = args.work or args.out.parent / (args.out.name + ".work")
    print("\n".join(build(args.src, args.out, args.esrgan, args.models, work, choices)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
