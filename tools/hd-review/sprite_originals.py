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
from png_index_patch import PngError, read_indexed_png  # noqa: E402

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

    Only the review's own folders -- `original` and one per upscale option. A symlinked one stops the
    run: it is not ours to delete through, and skipping it would leave old renders under a current
    stamp."""
    stamp = out / "original" / STAMP
    if stamp.exists() and stamp.read_text().strip() == EXPORT_VERSION:
        return 0
    removed = 0
    for name in ("original", *hd_upscale.OPTIONS):
        folder = out / name
        if folder.is_symlink():
            raise SystemExit(f"{folder} is a symlink and may hold images from the old decode: "
                             "clear its sprite__*.png by hand, then rerun")
        if not folder.is_dir():
            continue
        for png in folder.glob("sprite__*.png"):
            png.unlink()
            removed += 1
    if removed:
        print(f"{removed} sprite images from an older export removed", flush=True)
    return removed


PALETTE_AT = 8                   # u32 in the IMP header: offset of 256 entries of 4 bytes


def expected_plte(imp: bytes) -> tuple[bytes, bytes]:
    """The PLTE a viewer should export for this IMP, and the one the pre-fix viewer did.

    Entries are stored blue, green, red, pad (research log, 2026-09-23); the old decode read them
    blue, red, green."""
    at = struct.unpack_from("<I", imp, PALETTE_AT)[0]
    if at + 1024 > len(imp):
        raise ValueError("palette runs past the end of the file")
    entries = [imp[at + i * 4:at + i * 4 + 3] for i in range(256)]
    return (b"".join(bytes((e[2], e[1], e[0])) for e in entries),
            b"".join(bytes((e[1], e[2], e[0])) for e in entries))


def viewer_decodes_bgr(viewer: pathlib.Path, archive: pathlib.Path, members: list[str],
                       listfile: pathlib.Path, read_member=None, tries: int = 60) -> bool:
    """Whether this viewer binary decodes IMP palettes blue, green, red.

    The stamp says which decoder the source has; the binary doing the export can be older (a
    worktree clones `target/` from its donor). So export one sprite and compare its PLTE with the
    member's own palette bytes, read independently of the viewer: the answer comes from the file
    format, not from which colours this archive happens to hold."""
    if read_member is None:
        from mpq_read import Archive
        read_member = Archive(archive).read
    with tempfile.TemporaryDirectory() as scratch:
        for n, member in enumerate(members[:tries]):
            png = pathlib.Path(scratch) / f"{n}.png"
            result = subprocess.run([str(viewer), "--export-imp-frame", str(archive), member, "0",
                                     str(png), "--listfile", str(listfile)],
                                    capture_output=True, text=True)
            if result.returncode != 0 or not png.exists():
                continue
            try:
                fixed, swapped = expected_plte(read_member(member))
            except Exception:                # not in this archive, or not readable here
                continue
            if fixed == swapped:
                continue                     # every entry has red equal to green: says nothing
            try:
                plte = read_indexed_png(png.read_bytes()).palette()[:768]
            except PngError:
                continue
            if plte == fixed:
                return True
            if plte == swapped:
                return False
    raise SystemExit(f"could not check how {viewer} decodes palettes: none of the first {tries} "
                     "sprites both exported and read back")


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
    if originals.is_symlink():
        raise SystemExit(f"{originals} is a symlink: refusing to write sprites through it")
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
            png.unlink(missing_ok=True)      # a half-written file would be skipped as done next time
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
