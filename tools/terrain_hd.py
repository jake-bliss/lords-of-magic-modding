#!/usr/bin/env python3
"""2x terrain atlases: every tile upscaled on its own, then quantized back to its atlas's palette.

    python3 tools/terrain_hd.py SRC_DIR OUT_DIR --esrgan PATH --models DIR [--work DIR]

SRC_DIR holds the game's `til\\*.lbm` and `til\\*.til` members. OUT_DIR receives the 2x atlases and
the .til files with TILESIZE doubled -- what `mods/terrain-hd-art` stages into pic.mpq.

**Per tile, not per sheet.** An atlas is a grid of Wang tiles: each `TILE=` line (tilenum = atlas
cell, then the cell's own terrain type and the types on its n, ne, e, se, s, sw, w, nw sides) says
what may sit next to it, and the map places cells by those types, never by where they sit on the
sheet. Upscaling the whole sheet lets the model blend each tile into whatever tile is stored beside
it: the first staging (2026-09-23) put a colour that exists only in the NEIGHBOURING cell into
8-12% of every tile's edge ring.

**Padded with a neighbour the map could place there.** Each side is padded with a plain tile of the
terrain type that side borders (texture top = map north, right = east -- measured: compatible
neighbours' edges match about 20% better than random pairs that way round). The model then sees the
right terrain continuing past every edge. Repeating the tile's own edge instead left a visible step
at every tile edge on the map (rung 2, 2026-09-24).

**Edges pulled to their terrain's colour.** Adjacent tiles are not drawn pixel-continuous, and the
model smooths each tile's interior, so a leftover step between two tiles reads as a line where the
noisy 1x art hid it. In the outer NORM_PX of each tile, the local average colour (a blur of the tile
itself) is shifted to the average colour of the terrain type on that side, with the detail kept on
top. Any two tiles of one type then meet at the same base colour, whichever two the map picks.
The detail right at the edge is damped a little too: it is what does not continue across. Mosaic
of random plain meadow tiles, seam step / interior step: edge padding + blur 1.66, neighbour padding
+ colour only 1.51, + damping 0.97 (see NORM_*).

**Quantized index-safely.** Texels go through the light tables by PALETTE INDEX, so the atlas must
stay 8-bit in its own palette. A 2x pixel may only take an index that occurs within one source
pixel of it AND inside the same tile (two, in the NORM_PX band at a tile's edge), so no index
reaches a pixel more than two source pixels from where the original had it, and none crosses a
tile edge. (An isolated index CAN grow into its neighbouring 2x pixels. None of the 20 atlases has an active CRNG range, so no cycling colour can
spread; a key or cycling index added later would want excluding from its neighbours' candidates.)

**Not every atlas.** `thite01`/`ttype01` are data maps read by coordinate (height and terrain type),
not textures; doubling them would double the map's lookups rather than its detail.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
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
NORM_PX = 4          # 2x pixels from a tile edge over which colour is pulled to the terrain's
NORM_SIGMA = 2.0     # blur (2x pixels) that defines a pixel's "local average colour"
NORM_DAMP = 0.4      # how much of the fine detail is damped at the very edge
# Chosen on mosaics of random plain meadow and plains tiles by two readings that pull opposite ways:
# the step across a seam, and the detail left in the edge band, each over the interior's. Colour
# alone (8px, sigma 4) left seams at 1.51-1.57x; wider bands or broader blurs were WORSE (the leftover
# is fine detail that does not continue across the edge, not shading). Damping fixes the step but
# flattens the band into a visible "grout" lattice past about half. 4px / sigma 2 / 0.4: seam
# 0.97-1.01x, band detail 0.95-1.01x -- neither reads as a line.
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


SIDES = ("n", "e", "s", "w")


def _types(field: str) -> set[int] | None:
    """`6`, `6|9`, `~6|9` -> {6, 9}; `*` (any) -> None. `~` is kept as the types it names."""
    field = field.strip().lstrip("~")
    return None if field == "*" else {int(v) for v in field.split("|") if v}


def tile_defs(src: pathlib.Path) -> dict[str, dict[int, dict]]:
    """Per atlas, cell -> {"self": type, "n"/"e"/"s"/"w": the type on that side, "pure": bool}.
    The cell is the TILE= line's FIRST field (tile 392 is plain water and cell 392 is blue); the
    last field is something else. Where two .til files describe one cell (tilesa01.til and
    tilesb01.til both draw from tilesb01.lbm), the first, in name order, is kept."""
    defs: dict[str, dict[int, dict]] = {}
    for til in sorted(src.glob("*.til")):
        name, _ = read_til(til)
        cells = defs.setdefault(name, {})
        for line in til.read_bytes().decode("latin-1").replace("\r", "\n").splitlines():
            if not line.startswith("TILE="):
                continue
            f = line[5:].split(",")
            if len(f) < 10:
                continue
            cell, own = int(f[0]), int(f[1])
            ring = [_types(v) for v in f[2:10]]
            sides = dict(zip(("n", "ne", "e", "se", "s", "sw", "w", "nw"), ring))
            entry = {"self": own, "pure": all(r == {own} for r in ring)}
            for side in SIDES:
                v = sides[side]
                entry[side] = own if v is None or own in v else min(v)
            cells.setdefault(cell, entry)
    return defs


def neighbours(defs: dict[int, dict], cell: int) -> dict[str, int | None]:
    """For each side, a plain tile of the terrain on that side (deterministic), or None."""
    d = defs.get(cell)
    if d is None:
        return dict.fromkeys(SIDES)
    by_type: dict[int, list[int]] = {}
    for c, e in sorted(defs.items()):
        if e["pure"]:
            by_type.setdefault(e["self"], []).append(c)
    out = {}
    for side in SIDES:
        pool = [c for c in by_type.get(d[side], []) if c != cell] or by_type.get(d[side], [])
        out[side] = pool[(cell * 7) % len(pool)] if pool else None
    return out


def padded_tile(idx: bytes, w: int, pal, tx: int, ty: int, t: int, pad: int,
                nbrs: dict[str, int | None] | None = None):
    """RGB rows of one tile padded `pad` pixels outward. Each side comes from its neighbour cell
    (`nbrs`, cell numbers on this sheet) where there is one -- the neighbour's pixels as they would
    continue past the edge -- and otherwise repeats the tile's own edge. Corners repeat the tile's
    own corner pixel. Never the cell stored beside it on the sheet."""
    nbrs = nbrs or {}
    per = w // t

    def at(cell: int, x: int, y: int):
        return pal[idx[((cell // per) * t + y) * w + (cell % per) * t + x]]

    own = ty * per + tx
    clamp = lambda v: min(max(v, 0), t - 1)  # noqa: E731
    rows = []
    for y in range(-pad, t + pad):
        row = []
        for x in range(-pad, t + pad):
            inside_x, inside_y = 0 <= x < t, 0 <= y < t
            side = None
            if inside_x and not inside_y:
                side = "n" if y < 0 else "s"
            elif inside_y and not inside_x:
                side = "w" if x < 0 else "e"
            n = nbrs.get(side) if side else None
            if n is not None:
                row.append(at(n, x % t, y % t))
            else:
                row.append(at(own, clamp(x), clamp(y)))
        rows.append(row)
    return rows


def type_means(idx: bytes, w: int, pal, t: int, defs: dict[int, dict]) -> dict[int, tuple[float, ...]]:
    """Average 1x colour of each terrain type, over its plain tiles on this sheet."""
    per, acc = w // t, {}
    for cell, d in defs.items():
        if not d["pure"]:
            continue
        s = acc.setdefault(d["self"], [0, 0, 0, 0])
        for y in range(t):
            for x in range(t):
                c = pal[idx[((cell // per) * t + y) * w + (cell % per) * t + x]]
                s[0] += c[0]; s[1] += c[1]; s[2] += c[2]; s[3] += 1
    return {k: (v[0] / v[3], v[1] / v[3], v[2] / v[3]) for k, v in acc.items()}


def normalize_edges(rgb: bytes, low: bytes, w2: int, h2: int, t2: int, defs: dict[int, dict],
                    means: dict[int, tuple[float, ...]], band: int, damp: float = NORM_DAMP) -> bytes:
    """Shift each tile's local average colour (`low`, a per-tile blur) toward its side's terrain
    average across the outer `band` pixels, and damp the detail (`rgb - low`) by up to `damp` at
    the very edge. Cells with no TILE= line,
    or sides whose terrain has no plain tile on this sheet, are left alone."""
    out = bytearray(rgb)
    per = w2 // t2
    for y in range(h2):
        ty, iy = divmod(y, t2)
        for x in range(w2):
            tx, ix = divmod(x, t2)
            d = defs.get(ty * per + tx)
            if d is None:
                continue
            dist = {"w": ix, "e": t2 - 1 - ix, "n": iy, "s": t2 - 1 - iy}
            side = min(dist, key=dist.get)
            a = 1 - dist[side] / band
            target = means.get(d[side])
            if a <= 0 or target is None:
                continue
            p = (y * w2 + x) * 3
            for q in range(3):
                detail = (rgb[p + q] - low[p + q]) * (1 - damp * a)
                out[p + q] = min(255, max(0, round(low[p + q] + detail + a * (target[q] - low[p + q]))))
    return bytes(out)


def quantize(idx: bytes, w: int, h: int, pal, t: int, rgb: bytes, band: int = 0) -> tuple[bytes, float]:
    """Each 2x pixel -> the nearest palette colour among indices within one source pixel of it,
    clamped to its own tile. Inside the normalized `band` (2x pixels from a tile edge) the reach is
    two source pixels: nine candidates snap a blended edge colour straight back to a hard one and
    undo most of the softening. Returns the indices and how often a block's top-left keeps its
    source index (a sanity reading: high, and not 100%)."""
    W, T = 2 * w, 2 * t
    out = bytearray(4 * w * h)
    for y in range(2 * h):
        sy = y // 2
        y0 = (sy // t) * t
        for x in range(W):
            sx = x // 2
            x0 = (sx // t) * t
            r_ = 2 if min(x % T, y % T, T - 1 - x % T, T - 1 - y % T) < band else 1
            cand = {idx[yy * w + xx] for yy in range(max(y0, sy - r_), min(y0 + t, sy + r_ + 1))
                    for xx in range(max(x0, sx - r_), min(x0 + t, sx + r_ + 1))}
            p = (y * W + x) * 3
            r, g, b = rgb[p], rgb[p + 1], rgb[p + 2]
            # Ties go to the source pixel's own index, then to the lowest. Palettes repeat colours
            # (ruins01: indices 1 and 89 are the same RGB) and the light tables work by INDEX, so
            # an equal-looking swap is not a harmless one.
            src = idx[sy * w + sx]
            out[y * W + x] = min(cand, key=lambda i: ((pal[i][0] - r) ** 2 + (pal[i][1] - g) ** 2
                                                      + (pal[i][2] - b) ** 2, i != src, i))
    kept = sum(out[2 * y * W + 2 * x] == idx[y * w + x] for y in range(h) for x in range(w))
    return bytes(out), kept / (w * h)


def foreign_edge_pixels(idx: bytes, w: int, h: int, t: int, out: bytes) -> int:
    """2x pixels holding an index their own TILE does not have within two source pixels. The
    per-tile pipeline makes this 0 by construction; it is measured anyway, because it is the defect."""
    W, bad = 2 * w, 0
    for y in range(2 * h):
        sy = y // 2
        y0 = (sy // t) * t
        for x in range(W):
            sx = x // 2
            x0 = (sx // t) * t
            own = {idx[yy * w + xx] for yy in range(max(y0, sy - 2), min(y0 + t, sy + 3))
                   for xx in range(max(x0, sx - 2), min(x0 + t, sx + 3))}
            bad += out[y * W + x] not in own
    return bad


def build(src: pathlib.Path, out: pathlib.Path, esrgan: pathlib.Path, models: pathlib.Path,
          work: pathlib.Path, choices: dict[str, str], norm: int = NORM_PX) -> list[str]:
    out.mkdir(parents=True, exist_ok=True)
    sizes = tile_sizes(src)
    all_defs = tile_defs(src)
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
        # Tiles and renders are reused on a rerun, so their names carry everything they were made
        # from: the source atlas's bytes, the tile size, the padding and every tile's neighbours.
        # A re-extracted source, a new PAD or an edited .til gets fresh tiles instead of silently
        # compositing the old ones.
        defs = all_defs.get(f"{name}.lbm", {})
        per_row = w // t
        nbrs = {c: neighbours(defs, c) for c in range(per_row * (h // t))}
        digest = hashlib.sha256(lbm.read_bytes() + b"%d/%d/" % (t, PAD)
                                + json.dumps(nbrs, sort_keys=True).encode()).hexdigest()[:12]
        tiles_dir = work / "tiles" / f"{name}-{digest}"
        tiles_dir.mkdir(parents=True, exist_ok=True)
        inputs = {}
        for ty in range(h // t):
            for tx in range(w // t):
                key = f"{name}-{digest}_{tx:02d}_{ty:02d}"
                path = tiles_dir / f"{key}.png"
                if not path.exists():
                    # Reused whenever it exists, so it only appears whole: a run stopped mid-write
                    # leaves a .part, never a truncated tile the next run would take as done.
                    part = path.with_name(path.name + ".part")
                    lbm_png.write_png(part, t + 2 * PAD, t + 2 * PAD,
                                      padded_tile(idx, w, pal, tx, ty, t, PAD, nbrs[ty * per_row + tx]))
                    os.replace(part, path)
                inputs[key] = path
        rendered = work / "rendered" / option
        hd_upscale.render(option, inputs, rendered, esrgan, models)
        # Crop each tile's centre and lay it back on the 2x sheet.
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            sheet = tmp / "sheet.png"

            def assemble(dest: pathlib.Path, sigma: float = 0) -> None:
                args = ["magick", "-size", f"{2 * w}x{2 * h}", "xc:black"]
                for key in inputs:
                    tx, ty = int(key[-5:-3]), int(key[-2:])
                    # Blurred AFTER the crop, against the tile's own repeated edge, so the local
                    # average never reaches a neighbouring cell either.
                    extra = ["-virtual-pixel", "edge", "-blur", f"0x{sigma}"] if sigma else []
                    args += ["(", str(rendered / f"{key}.png"), "-crop", f"{2 * t}x{2 * t}+{2 * PAD}+{2 * PAD}",
                             "+repage", *extra, ")", "-geometry", f"+{2 * t * tx}+{2 * t * ty}", "-composite"]
                args.append(str(dest))
                subprocess.run(args, check=True)

            def pixels(path: pathlib.Path) -> bytes:
                return subprocess.check_output(["magick", str(path), "-depth", "8", "rgb:-"])

            assemble(sheet)
            rgb = pixels(sheet)
            if norm and defs:
                low = tmp / "low.png"
                assemble(low, NORM_SIGMA)
                rgb = normalize_edges(rgb, pixels(low), 2 * w, 2 * h, 2 * t, defs,
                                      type_means(idx, w, pal, t, defs), norm)
        if len(rgb) != 4 * w * h * 3:
            raise SystemExit(f"{name}: assembled sheet is {len(rgb)} bytes, not {12 * w * h}")
        indices, kept = quantize(idx, w, h, pal, t, rgb, band=norm)
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
        padded = sum(v is not None for n in nbrs.values() for v in n.values())
        report.append(f"{name}: {w}x{h} -> {2 * w}x{2 * h}, {t}px tiles, {option}, {len(defs)} defined, "
                      f"{padded} sides neighbour-padded; "
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
