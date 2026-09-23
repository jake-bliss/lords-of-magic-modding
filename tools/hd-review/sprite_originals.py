#!/usr/bin/env python3
"""One representative frame per IMP sprite, into the review page's originals.

    python3 tools/hd-review/sprite_originals.py IMP_MPQ OUT_DIR [--viewer PATH] [--listfile PATH]

Writes OUT_DIR/original/sprite__<name>.png (RGBA; the game's 1-bit transparency as alpha) for the
review page, one per sprite: the first frame of the middle facing of its STAND sequence, or of its
first sequence when it has no STAND. One frame stands for the whole sprite because animation needs
one upscaler per sprite -- a model switching between frames would flicker.

The review this feeds picks an upscaler per sprite. It does not make sprites the game can load
(see tools/sprite-review/README.md for why an upscaled .imp cannot go back); drawing them over
the frame, as the overlay does for pictures, is the route this would serve. Game art: keep
OUT_DIR under the gitignored artifacts/.
"""
from __future__ import annotations

import argparse
import pathlib
import struct
import subprocess
import sys
import tempfile
import zlib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(ROOT / "tools" / "sprite-review"))
from generate import describe  # noqa: E402  -- the parser the sprite review already uses
sys.path.insert(0, str(ROOT / "tools"))
import hd_upscale  # noqa: E402  -- the option names are the render folders
from png_index_patch import read_indexed_png  # noqa: E402

MIN_SIDE = 16                    # smaller than this is a spark or a dot: nothing to upscale
# Bumped whenever the viewer's decoding of IMP art changes. Originals already on disk are reused,
# so without this a rerun after a decoder fix keeps the old colours -- and so does every upscale
# rendered from them. 2026-09-23: palettes read blue, green, red (they were red/green swapped).
EXPORT_VERSION = "imp-bgr-2026-09-23"
STAMP = ".sprite-export-version"
SHADOW_INDEX = 1                 # keyed by index, whatever colour the palette gives it


def clear_shadow(png: pathlib.Path) -> None:
    """Make the shadow index transparent in the exported (indexed) PNG. The engine draws index 1
    as a darkening, not as its colour -- usually pure red -- and an upscaler would smear that colour
    into the sprite. A shadow would be drawn as its own mask; the art is upscaled without it."""
    data = png.read_bytes()
    out, pos, trns_seen = [data[:8]], 8, False
    while pos < len(data):
        length, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + length]
        if kind == b"tRNS":
            alpha = bytearray(body.ljust(SHADOW_INDEX + 1, b"\xff"))
            alpha[SHADOW_INDEX] = 0
            body, trns_seen = bytes(alpha), True
        if kind == b"IDAT" and not trns_seen:
            extra = bytes(b"\xff" * SHADOW_INDEX + b"\x00")
            out.append(struct.pack(">I4s", len(extra), b"tRNS") + extra
                       + struct.pack(">I", zlib.crc32(b"tRNS" + extra)))
            trns_seen = True
        out.append(struct.pack(">I4s", len(body), kind) + body + struct.pack(">I", zlib.crc32(kind + body)))
        pos += 12 + length
    png.write_bytes(b"".join(out))


def discard_stale(out: pathlib.Path) -> int:
    """Remove sprite originals, and every upscale rendered from them, written by an older export.

    Only the review's own folders -- `original` and one per upscale option -- and never through a
    symlink: OUT_DIR is the caller's, and anything else in it is not ours to delete."""
    stamp = out / "original" / STAMP
    if stamp.exists() and stamp.read_text().strip() == EXPORT_VERSION:
        return 0
    removed = 0
    for name in ("original", *hd_upscale.OPTIONS):
        folder = out / name
        if folder.is_symlink() or not folder.is_dir():
            continue
        for png in folder.glob("sprite__*.png"):
            png.unlink()
            removed += 1
    if removed:
        print(f"{removed} sprite images from an older export removed", flush=True)
    return removed


GREEN, RED = b"\x00\xff\x00", b"\xff\x00\x00"


def viewer_decodes_bgr(viewer: pathlib.Path, archive: pathlib.Path, members: list[str],
                       listfile: pathlib.Path, tries: int = 60) -> bool:
    """Whether this viewer binary decodes IMP palettes blue, green, red.

    The stamp says which decoder the source has; the binary doing the export can be older (a
    worktree clones `target/` from its donor). Most shipped palettes hold pure green in slot 0 and
    pure red in slot 1, so one export reads the answer off the file: red then green is the old,
    swapped decode."""
    with tempfile.TemporaryDirectory() as scratch:
        for n, member in enumerate(members[:tries]):
            png = pathlib.Path(scratch) / f"{n}.png"
            result = subprocess.run([str(viewer), "--export-imp-frame", str(archive), member, "0",
                                     str(png), "--listfile", str(listfile)],
                                    capture_output=True, text=True)
            if result.returncode != 0 or not png.exists():
                continue
            slots = read_indexed_png(png.read_bytes()).palette()[:6]
            if slots == GREEN + RED:
                return True
            if slots == RED + GREEN:
                return False
    raise SystemExit(f"could not check how {viewer} decodes palettes: no export showed the usual "
                     f"slot colours in the first {tries} sprites")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("archive", type=pathlib.Path)
    parser.add_argument("out", type=pathlib.Path)
    parser.add_argument("--viewer", type=pathlib.Path,
                        default=ROOT / "spikes/asset-viewer/target/release/lom-asset-viewer")
    parser.add_argument("--listfile", type=pathlib.Path,
                        default=ROOT / "reports/member-names/all-profiles-imp-recovered.txt")
    args = parser.parse_args()
    originals = args.out / "original"
    originals.mkdir(parents=True, exist_ok=True)
    members = sorted({m.strip() for m in args.listfile.read_text().splitlines()
                      if m.strip().lower().endswith(".imp")}, key=str.lower)
    if not viewer_decodes_bgr(args.viewer, args.archive, members, args.listfile):
        raise SystemExit(f"{args.viewer} predates the palette fix and would export red and green "
                         "swapped: rebuild it (cargo build --release in spikes/asset-viewer)")
    discard_stale(args.out)
    (originals / STAMP).write_text(EXPORT_VERSION + "\n")
    written = skipped = 0
    seen = set()
    for n, member in enumerate(members, 1):
        name = member.split("\\")[-1].rsplit(".", 1)[0].lower()
        png = originals / f"sprite__{name}.png"
        if name in seen:
            continue
        seen.add(name)
        if png.exists():
            written += 1
            continue
        try:
            sequences = describe(args.viewer, args.archive, member, args.listfile)
        except SystemExit:
            skipped += 1                     # not in this install's archive
            continue
        if not sequences or not any(s["facings"] for s in sequences):
            skipped += 1
            continue
        chosen = next((s for s in sequences if s["label"].upper() == "STAND" and s["facings"]),
                      next(s for s in sequences if s["facings"]))
        facing = chosen["facings"][len(chosen["facings"]) // 2]
        if not facing:
            skipped += 1
            continue
        result = subprocess.run([str(args.viewer), "--export-imp-frame", str(args.archive), member,
                                 str(facing[0]), str(png), "--listfile", str(args.listfile)],
                                capture_output=True, text=True)
        if result.returncode != 0 or not png.exists():
            skipped += 1
            continue
        try:
            clear_shadow(png)
            # Transparent pixels keep the key colour (green) underneath, and the upscaler bleeds
            # what is under the edge into it. A neutral dark grey bleeds least visibly.
            subprocess.run(["magick", str(png), "-background", "#202228", "-alpha", "background",
                            f"PNG32:{png}"], check=True)
            w, h = (int(v) for v in subprocess.run(
                ["magick", "identify", "-format", "%w %h", str(png)],
                capture_output=True, text=True, check=True).stdout.split())
        except BaseException:
            png.unlink(missing_ok=True)      # half-converted: a rerun must redo it, not skip it
            raise
        if min(w, h) < MIN_SIDE:
            png.unlink()
            skipped += 1
            continue
        written += 1
        if n % 100 == 0:
            print(f"  {n}/{len(members)}", flush=True)
    print(f"{written} sprites written, {skipped} skipped", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
