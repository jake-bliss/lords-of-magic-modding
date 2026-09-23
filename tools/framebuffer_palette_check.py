#!/usr/bin/env python3
"""Check an exported IMP frame's colours against the game's own framebuffer.

    python3 tools/framebuffer_palette_check.py FRAME.raw SPRITE.png X Y

FRAME.raw is a frame dumped by the HD-overlay build of cnc-ddraw (`LOMHDRAW`, then u32 width,
height and bits per pixel, then RGB565 pixels): the game's 16-bit surface itself, with no screenshot
writer in between. SPRITE.png is one frame exported with `lom-asset-viewer --export-imp-frame`, and
X Y is where that frame sits on screen.

Prints, for each of the six orderings of the exported palette's channels, the share of the sprite's
drawn pixels (every index but 0, transparency, and 1, the shadow) whose colour, truncated to RGB565,
equals the framebuffer. The identity ordering winning outright means the viewer decodes IMP palettes
the way the engine draws them. It also prints whether every index maps to a single framebuffer
colour, which separates a channel-order error (it does) from a different palette altogether (it
does not).

This is the instrument that settled the channel order on 2026-09-23 (research log): the engine's
`screencapture` BMPs cannot, because their own byte order was the thing in doubt. Frames and
exports are game art, so they stay under the gitignored `artifacts/`.
"""
from __future__ import annotations

import argparse
import itertools
import pathlib
import struct
import sys
from collections import defaultdict

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from png_index_patch import read_indexed_png  # noqa: E402

TRANSPARENT, SHADOW = 0, 1


def read_frame(data: bytes) -> tuple[int, int, list[int]]:
    """(width, height, RGB565 pixels) from a `LOMHDRAW` dump."""
    if data[:8] != b"LOMHDRAW":
        raise ValueError("not a LOMHDRAW frame")
    width, height, bits = struct.unpack_from("<III", data, 8)
    if bits != 16:
        raise ValueError(f"expected 16 bits per pixel, found {bits}")
    if len(data) < 20 + width * height * 2:
        raise ValueError("frame is truncated")
    return width, height, list(struct.unpack_from(f"<{width * height}H", data, 20))


def rgb565(r: int, g: int, b: int) -> int:
    return ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)


def check(frame: bytes, sprite_png: bytes, x: int, y: int) -> dict:
    width, height, pixels = read_frame(frame)
    sprite = read_indexed_png(sprite_png)
    if x < 0 or y < 0 or x + sprite.width > width or y + sprite.height > height:
        raise ValueError("the sprite does not fit in the frame at that position")
    plte = sprite.palette()
    palette = [tuple(plte[i * 3:i * 3 + 3]) for i in range(len(plte) // 3)]
    drawn = [(sprite.at(sx, sy), pixels[(y + sy) * width + x + sx])
             for sy in range(sprite.height) for sx in range(sprite.width)
             if sprite.at(sx, sy) not in (TRANSPARENT, SHADOW)]
    if not drawn:
        raise ValueError("the sprite has no drawn pixels")
    orderings = {}
    for order in itertools.permutations(range(3)):
        hits = sum(rgb565(*(palette[i][c] for c in order)) == seen for i, seen in drawn)
        orderings["".join("RGB"[c] for c in order)] = hits / len(drawn)
    colours = defaultdict(set)
    for i, seen in drawn:
        colours[i].add(seen)
    return {"pixels": len(drawn), "orderings": orderings,
            "one_colour_per_index": all(len(c) == 1 for c in colours.values())}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("frame", type=pathlib.Path)
    parser.add_argument("sprite", type=pathlib.Path)
    parser.add_argument("x", type=int)
    parser.add_argument("y", type=int)
    args = parser.parse_args(argv)
    result = check(args.frame.read_bytes(), args.sprite.read_bytes(), args.x, args.y)
    print(f"{result['pixels']} drawn pixels")
    for name, share in sorted(result["orderings"].items(), key=lambda kv: -kv[1]):
        label = "  <- as exported" if name == "RGB" else ""
        print(f"  palette read as {name}: {share:.3f}{label}")
    print(f"one framebuffer colour per index: {'yes' if result['one_colour_per_index'] else 'no'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
