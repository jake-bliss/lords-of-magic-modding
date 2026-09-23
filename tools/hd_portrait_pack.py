#!/usr/bin/env python3
"""Pack original images and their upscales into the file the HD overlay reads at run time.

    python3 tools/hd_portrait_pack.py OUT.pack --originals DIR [...] --sources DIR [...] --upscaled DIR [...]

  --originals  the images the PLAYER'S install holds -- what the game will put on screen
  --sources    the images the upscales were MADE FROM
  --upscaled   the upscales themselves: full-colour PNGs, or older palette LBMs

Each may be given more than once (portraits and building pictures live in different folders).
Files pair by name, case-insensitively, ignoring the extension.

The overlay (a fork of cnc-ddraw, see docs/hd-overlay.md) finds an image on screen by matching the
ORIGINAL pixels in the finished frame, then draws the UPSCALE over it at window resolution. So each
record carries both: the original as palette plus indices (the matcher turns them into RGB565
through the palette), and the upscale as zlib-compressed full-colour RGB. Full colour because the overlay
draws its own texture: squeezing the upscale back into the original's 256 colours, with a despeckle
before it, is what lost detail in the first building upscales (2026-09-22).

This is derived from the player's own install, like every other recipe here. It is never committed.

🔴 PAIRING IS BY CONTENT, NOT BY NAME. An upscale is packed only when the installed original is
pixel- and palette-identical to the original it was made from. Observed 2026-09-22: the vanilla
and GS5R3 installs share 445 portrait names, but 5 of those differ, and the vanilla Life banner is
not the GS5R3 one at all. A name match would have drawn a GS5R3 upscale over a different picture.

Format LOMHDPK3, little-endian. An index first, so the overlay can read it without reading the
rest: full-screen upscales make the pack ~850 MB, and the game is a 32-bit process.

    b"LOMHDPK3"  u32 count
    count index records, in order:
                 u8 name_len, name (ASCII, lowercase), u16 w, u16 h, u16 hw, u16 hh,
                 256 x (r, g, b), u32 idx_len, u32 hd_len
    then, for each record in the same order and with nothing between them:
                 zlib(w*h palette indices), idx_len bytes
                 zlib(hw*hh*3 RGB), hd_len bytes
    The last stream ends at the end of the file.
"""
from __future__ import annotations

import argparse
import pathlib
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "portrait-upscale"))
import lbm_png  # noqa: E402

MAGIC = b"LOMHDPK3"

# What the overlay's reader (src/lomhd_match.c in the cnc-ddraw fork) accepts. A pack it would
# refuse must fail HERE, at build time, not load as "corrupt" in the game with the overlay silently
# off. (Found by cross-model review, 2026-09-22: the writer checked none of these.)
MAX_IMAGES = 5461           # count * 2 rules * 3 probe rows must stay under half of 65536 slots
MIN_WIDTH = 32              # the matcher hashes a 32-pixel slice of each probe row
MIN_HEIGHT = 4              # three probe rows at h/4, h/2 and 3h/4 need at least four rows
MAX_UPSCALE_SIDE = 1280     # a 640x480 screen at 2x
IMAGE_SUFFIXES = {".lbm", ".png"}


def image_files(directories) -> dict[str, pathlib.Path]:
    """Name -> path across one or more flat directories, keyed by lowercase stem. 8 of the 748
    shipped portraits are spelled `.LBM`, and a case-sensitive match dropped them once already."""
    if isinstance(directories, (str, pathlib.Path)):
        directories = [directories]
    found: dict[str, pathlib.Path] = {}
    for directory in directories:
        for entry in sorted(pathlib.Path(directory).iterdir()):
            if entry.is_file() and entry.suffix.lower() in IMAGE_SUFFIXES:
                key = entry.stem.lower()
                if key in found:
                    raise SystemExit(f"{entry} and {found[key]} are the same image twice")
                found[key] = entry
    return found


def load_rgb(path: pathlib.Path) -> tuple[int, int, bytes]:
    """Any upscale as full-colour RGB. PNGs go through ImageMagick, which the recipe needs anyway;
    the standard library has no PNG decoder."""
    if path.suffix.lower() == ".lbm":
        w, h, idx, pal, _ = lbm_png.decode(path)
        return w, h, b"".join(bytes(pal[i]) for i in idx)
    out = subprocess.run(["magick", str(path), "-depth", "8", "-alpha", "off", "PPM:-"],
                         check=True, capture_output=True).stdout
    # Header tokens, then EXACTLY one whitespace byte before the pixels. split() would also eat a
    # first pixel that happens to be 0x09-0x0d or 0x20, one byte short (seen on aifitp00).
    tokens, pos = [], 0
    while len(tokens) < 4:
        while out[pos:pos + 1].isspace():
            pos += 1
        end = pos
        while not out[end:end + 1].isspace():
            end += 1
        tokens.append(out[pos:end])
        pos = end
    magic, w, h, maxval = tokens
    data = out[pos + 1:]
    if magic != b"P6" or maxval != b"255":
        raise SystemExit(f"{path}: magick did not return an 8-bit RGB image")
    w, h = int(w), int(h)
    if len(data) != w * h * 3:
        raise SystemExit(f"{path}: {len(data)} bytes of RGB, {w * h * 3} expected")
    return w, h, data


def encode_record(name: str, w: int, h: int, indices: bytes, palette, hw: int, hh: int,
                  rgb: bytes) -> tuple[bytes, bytes, bytes]:
    """(index entry, zlib indices, zlib upscale) for one image."""
    if len(palette) != 256:
        # A short CMAP would shift every later record if padded silently; refuse instead.
        raise ValueError(f"expected a 256-colour palette, got {len(palette)}")
    if len(indices) != w * h:
        raise ValueError(f"expected {w * h} indices, got {len(indices)}")
    if len(rgb) != hw * hh * 3:
        raise ValueError(f"expected {hw * hh * 3} bytes of RGB, got {len(rgb)}")
    zidx, zhd = zlib.compress(bytes(indices), 9), zlib.compress(rgb, 9)
    flat = bytes(channel for colour in palette for channel in colour)
    encoded = name.encode("ascii")
    entry = (struct.pack("<B", len(encoded)) + encoded + struct.pack("<HHHH", w, h, hw, hh) + flat
             + struct.pack("<II", len(zidx), len(zhd)))
    return entry, zidx, zhd


def same_image(a: pathlib.Path, b: pathlib.Path) -> bool:
    w1, h1, i1, p1, _ = lbm_png.decode(a)
    w2, h2, i2, p2, _ = lbm_png.decode(b)
    return (w1, h1, bytes(i1), list(p1)) == (w2, h2, bytes(i2), list(p2))


def is_pixel_multiple(w: int, h: int, idx, pal, hw: int, hh: int, rgb: bytes) -> bool:
    """An 'upscale' that only repeats each original pixel k x k looks exactly like the original once
    drawn at the slot's size -- the overlay would draw it and change nothing. Observed 2026-09-22:
    395 of the 748 portraits in the pack played that day were 2x2 repeats copied from the 2x UI
    experiment (`double_indices`), and only a byte-level comparison against a fresh recipe run
    exposed it. Compared as colours, so a different palette order cannot hide one."""
    if hw % w or hh % h or hw // w != hh // h or hw == w:
        return False
    k = hw // w
    return all(rgb[(y * hw + x) * 3:(y * hw + x) * 3 + 3] == bytes(pal[idx[(y // k) * w + x // k]])
               for y in range(hh) for x in range(hw))


def write(out: pathlib.Path, originals, upscaled, sources=None) -> tuple[int, list[str]]:
    """Write the pack to `out`; return how many images it holds and what was left out and why --
    reported, never silent. Streams: only the index is held in memory, the compressed images go
    to a temporary file beside `out` and are copied in after it.

    `sources` holds the originals the upscales were made from. When given, an image is packed only
    if the installed original is the same image; `None` means the installed originals ARE the
    sources, which is only true when the upscales were made from this very install."""
    small = image_files(originals)
    made_from = image_files(sources) if sources is not None else small
    large = image_files(upscaled)

    skipped = [f"{name}: no upscale" for name in sorted(set(small) - set(large))]
    skipped += [f"{name}: upscale with no original" for name in sorted(set(large) - set(small))]

    out = pathlib.Path(out)
    entries = []
    with tempfile.TemporaryFile(dir=out.parent) as streams:
        for name in sorted(set(small) & set(large)):
            if name not in made_from:
                skipped.append(f"{name}: no source original to verify the upscale against")
                continue
            if not same_image(small[name], made_from[name]):
                skipped.append(f"{name}: installed original differs from the one the upscale was made from")
                continue
            w, h, idx, pal, _ = lbm_png.decode(small[name])
            hw, hh, rgb = load_rgb(large[name])
            if is_pixel_multiple(w, h, idx, pal, hw, hh, rgb):
                skipped.append(f"{name}: the upscale is the original with each pixel repeated, not an upscale")
                continue
            check_reader_limits(name, w, h, hw, hh)
            entry, zidx, zhd = encode_record(name, w, h, idx, pal, hw, hh, rgb)
            entries.append(entry)
            streams.write(zidx)
            streams.write(zhd)

        if not entries:
            raise SystemExit("no images to pack; the overlay refuses an empty pack as corrupt")
        if len(entries) > MAX_IMAGES:
            raise SystemExit(f"{len(entries)} images; the overlay accepts at most {MAX_IMAGES}")
        streams.seek(0)
        with out.open("wb") as f:
            f.write(MAGIC + struct.pack("<I", len(entries)) + b"".join(entries))
            shutil.copyfileobj(streams, f, 1 << 20)
    return len(entries), skipped


def build(originals, upscaled, sources=None) -> tuple[bytes, list[str]]:
    """`write`, returning the pack's bytes: for tests and small packs."""
    with tempfile.TemporaryDirectory() as tmp:
        path = pathlib.Path(tmp) / "pack"
        _, skipped = write(path, originals, upscaled, sources)
        return path.read_bytes(), skipped


def count(path: pathlib.Path) -> int:
    """How many images a pack file holds, from its header alone."""
    with open(path, "rb") as f:
        head = f.read(12)
    if head[:8] != MAGIC:
        raise ValueError("not a format-3 image pack")
    return struct.unpack_from("<I", head, 8)[0]


def check_reader_limits(name: str, w: int, h: int, hw: int, hh: int) -> None:
    """Refuse what the overlay would refuse, with the reason, before any byte is written."""
    if w < MIN_WIDTH:
        raise SystemExit(f"{name}: {w} wide; the overlay needs at least {MIN_WIDTH}")
    if h < MIN_HEIGHT:
        raise SystemExit(f"{name}: {h} rows; the overlay needs at least {MIN_HEIGHT}")
    if hw > MAX_UPSCALE_SIDE or hh > MAX_UPSCALE_SIDE:
        raise SystemExit(f"{name}: upscale {hw}x{hh} exceeds the overlay's {MAX_UPSCALE_SIDE}x{MAX_UPSCALE_SIDE}")


def read(pack: bytes):
    """The inverse of `write`, used by the tests and by anyone checking a pack by hand.
    Returns (name, (w, h, palette, indices), (hw, hh, rgb)) per record."""
    if pack[:8] != MAGIC:
        raise ValueError("not a format-3 image pack")
    (n_records,) = struct.unpack_from("<I", pack, 8)
    pos, index = 12, []
    for _ in range(n_records):
        n = pack[pos]; name = pack[pos + 1:pos + 1 + n].decode("ascii"); pos += 1 + n
        w, h, hw, hh = struct.unpack_from("<HHHH", pack, pos); pos += 8
        pal = [tuple(pack[pos + i * 3:pos + i * 3 + 3]) for i in range(256)]; pos += 768
        idx_len, hd_len = struct.unpack_from("<II", pack, pos); pos += 8
        index.append((name, w, h, hw, hh, pal, idx_len, hd_len))
    out = []
    for name, w, h, hw, hh, pal, idx_len, hd_len in index:
        idx = zlib.decompress(pack[pos:pos + idx_len]); pos += idx_len
        rgb = zlib.decompress(pack[pos:pos + hd_len]); pos += hd_len
        if len(idx) != w * h or len(rgb) != hw * hh * 3:
            raise ValueError(f"{name}: stream sizes do not match {w}x{h} / {hw}x{hh}")
        out.append((name, (w, h, pal, idx), (hw, hh, rgb)))
    if pos != len(pack):
        raise ValueError(f"{len(pack) - pos} trailing bytes")
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("out", type=pathlib.Path)
    parser.add_argument("--originals", type=pathlib.Path, action="append", required=True)
    parser.add_argument("--upscaled", type=pathlib.Path, action="append", required=True)
    parser.add_argument("--sources", type=pathlib.Path, action="append", required=True,
                        help="the originals the upscales were made from")
    args = parser.parse_args()

    n, skipped = write(args.out, args.originals, args.upscaled, args.sources)
    print(f"{args.out}: {n} images, {args.out.stat().st_size / 1e6:.1f} MB")
    for line in skipped:
        print(f"  left out -- {line}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
