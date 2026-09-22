#!/usr/bin/env python3
"""Pack original portraits and their upscales into the file the HD overlay reads at run time.

    python3 tools/hd_portrait_pack.py OUT.pack --originals DIR --sources DIR --upscaled DIR [...]

  --originals  the portraits the PLAYER'S install holds -- what the game will put on screen
  --sources    the portraits the upscales were MADE FROM
  --upscaled   the upscales themselves

The overlay (a fork of cnc-ddraw, see docs/hd-overlay.md) finds a portrait on screen by matching
the ORIGINAL pixels in the finished frame, then draws the UPSCALED one over it at window
resolution. So each record carries both: the original, which the matcher turns into RGB565
templates, and the upscale, which it draws.

Both are stored as a palette plus indices, not as RGB. That is a quarter of the size (about 15 MB
for the full set rather than 56), and it leaves the colour conversion to one place -- the overlay,
which needs the palette anyway to build templates under both candidate RGB565 rules.

This is derived from the player's own install, like every other recipe here. It is never committed.

🔴 PAIRING IS BY CONTENT, NOT BY NAME. An upscale is packed only when the installed original is
pixel- and palette-identical to the original it was made from. Observed 2026-09-22: the vanilla
and GS5R3 installs share 445 portrait names, but 5 of those differ, and the vanilla Life banner is
not the GS5R3 one at all -- templates built from the wrong install matched the lord's portrait
(identical in both) and silently missed the banner. A name match would have drawn a GS5R3 upscale
over a different vanilla picture.

Format, little-endian:

    b"LOMHDPK1"  u32 count
    per record:  u8 name_len, name (ASCII, lowercase), then two images, original first:
                 u16 width, u16 height, 256 x (r, g, b), width*height indices
"""
from __future__ import annotations

import argparse
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "portrait-upscale"))
import lbm_png  # noqa: E402

MAGIC = b"LOMHDPK1"

# What the overlay's reader (src/lomhd_match.c in the cnc-ddraw fork) accepts. A pack it would
# refuse must fail HERE, at build time, not load as "corrupt" in the game with the overlay silently
# off. (Found by cross-model review, 2026-09-22: the writer checked none of these.)
MAX_PORTRAITS = 1365        # count * 2 rules * 3 probe rows must stay under half of 16384 slots
MIN_HEIGHT = 4              # three probe rows at h/4, h/2 and 3h/4 need at least four rows
MAX_UPSCALE_SIDE = 512      # the overlay's per-placement buffer


def lbm_files(directory: pathlib.Path) -> dict[str, pathlib.Path]:
    """Basename -> path, keyed lowercase. 8 of the 748 shipped portraits are spelled `.LBM`, and
    a case-sensitive match dropped them once already."""
    found: dict[str, pathlib.Path] = {}
    for entry in sorted(directory.iterdir()):
        if entry.is_file() and entry.suffix.lower() == ".lbm":
            key = entry.name.lower()
            if key in found:
                raise SystemExit(f"{entry} and {found[key]} differ only in case")
            found[key] = entry
    return found


def encode_image(width: int, height: int, indices: bytes, palette) -> bytes:
    if len(palette) != 256:
        # A short CMAP would shift every later record if padded silently; refuse instead.
        raise ValueError(f"expected a 256-colour palette, got {len(palette)}")
    if len(indices) != width * height:
        raise ValueError(f"expected {width * height} indices, got {len(indices)}")
    flat = bytes(channel for colour in palette for channel in colour)
    return struct.pack("<HH", width, height) + flat + bytes(indices)


def same_image(a: pathlib.Path, b: pathlib.Path) -> bool:
    w1, h1, i1, p1, _ = lbm_png.decode(a)
    w2, h2, i2, p2, _ = lbm_png.decode(b)
    return (w1, h1, bytes(i1), list(p1)) == (w2, h2, bytes(i2), list(p2))


def build(originals: pathlib.Path, upscaled: list[pathlib.Path],
          sources: pathlib.Path | None = None) -> tuple[bytes, list[str]]:
    """Return the pack and a list of what was left out and why -- reported, never silent.

    `sources` holds the originals the upscales were made from. When given, a portrait is packed
    only if the installed original is the same image; `None` means the installed originals ARE
    the sources, which is only true when the upscales were made from this very install."""
    small = lbm_files(originals)
    made_from = lbm_files(sources) if sources is not None else small
    large: dict[str, pathlib.Path] = {}
    for directory in upscaled:
        for key, path in lbm_files(directory).items():
            if key in large:
                raise SystemExit(f"{path} and {large[key]} are the same portrait twice")
            large[key] = path

    skipped = [f"{name}: no upscale" for name in sorted(set(small) - set(large))]
    skipped += [f"{name}: upscale with no original" for name in sorted(set(large) - set(small))]

    records = []
    first_width: int | None = None
    for name in sorted(set(small) & set(large)):
        if name not in made_from:
            skipped.append(f"{name}: no source original to verify the upscale against")
            continue
        if not same_image(small[name], made_from[name]):
            skipped.append(f"{name}: installed original differs from the one the upscale was made from")
            continue
        w, h, idx, pal, _ = lbm_png.decode(small[name])
        hw, hh, hidx, hpal, _ = lbm_png.decode(large[name])
        check_reader_limits(name, w, h, hw, hh, first_width)
        first_width = w if first_width is None else first_width
        encoded = name.encode("ascii")
        records.append(struct.pack("<B", len(encoded)) + encoded
                       + encode_image(w, h, idx, pal) + encode_image(hw, hh, hidx, hpal))

    if len(records) > MAX_PORTRAITS:
        raise SystemExit(f"{len(records)} portraits; the overlay accepts at most {MAX_PORTRAITS}")

    return MAGIC + struct.pack("<I", len(records)) + b"".join(records), skipped


def check_reader_limits(name: str, w: int, h: int, hw: int, hh: int, first_width: int | None) -> None:
    """Refuse what the overlay would refuse, with the reason, before any byte is written."""
    if first_width is not None and w != first_width:
        raise SystemExit(f"{name}: {w} wide, but the overlay scans one fixed width ({first_width})")
    if h < MIN_HEIGHT:
        raise SystemExit(f"{name}: {h} rows; the overlay needs at least {MIN_HEIGHT}")
    if hw > MAX_UPSCALE_SIDE or hh > MAX_UPSCALE_SIDE:
        raise SystemExit(f"{name}: upscale {hw}x{hh} exceeds the overlay's {MAX_UPSCALE_SIDE}x{MAX_UPSCALE_SIDE}")


def read(pack: bytes):
    """The inverse of `build`, used by the tests and by anyone checking a pack by hand."""
    if pack[:8] != MAGIC:
        raise ValueError("not a portrait pack")
    (count,) = struct.unpack_from("<I", pack, 8)
    pos, out = 12, []
    for _ in range(count):
        n = pack[pos]; name = pack[pos + 1:pos + 1 + n].decode("ascii"); pos += 1 + n
        images = []
        for _ in range(2):
            w, h = struct.unpack_from("<HH", pack, pos); pos += 4
            pal = [tuple(pack[pos + i * 3:pos + i * 3 + 3]) for i in range(256)]; pos += 768
            idx = pack[pos:pos + w * h]; pos += w * h
            images.append((w, h, pal, idx))
        out.append((name, images[0], images[1]))
    if pos != len(pack):
        raise ValueError(f"{len(pack) - pos} trailing bytes")
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("out", type=pathlib.Path)
    parser.add_argument("--originals", type=pathlib.Path, required=True)
    parser.add_argument("--upscaled", type=pathlib.Path, action="append", required=True)
    parser.add_argument("--sources", type=pathlib.Path, required=True,
                        help="the originals the upscales were made from")
    args = parser.parse_args()

    pack, skipped = build(args.originals, args.upscaled, args.sources)
    args.out.write_bytes(pack)
    print(f"{args.out}: {len(read(pack))} portraits, {len(pack) / 1e6:.1f} MB")
    for line in skipped:
        print(f"  left out -- {line}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
