#!/usr/bin/env python3
"""Pack STATIC IMP sprites (and, optionally, the existing pictures) into a format-4 HD pack, for a
DLL test that covers masked records rather than only the picture path.

    python3 tools/hd-review/sprite_pack.py ARCHIVE OUT.pack [--renders DIR] [--choices FILE]
        [--viewer PATH] [--listfile PATH]
        [--with-pack-inputs --originals DIR [...] --upscaled DIR [...] [--sources DIR [...]]]

A dev tool, not shipped: sprites are not part of the player release yet (see docs/hd-overlay.md,
"Terrain cannot use the overlay at all" and the sprite-pick caveat above it). Only STATIC sprites
are packed -- members whose IMP has exactly one frame in total, so one upscale covers the whole
sprite the way one covers a portrait. An animated sprite would need one upscale per frame or a
flickering mismatch between them; nothing here decides that yet.

Each sprite's LOW-RES half (palette + indices, what the matcher compares on screen) comes straight
from `--export-imp-frame` -- no shadow-clearing, no background fill, because the game still draws
the shadow and the transparent key exactly as the archive stores them. The HD half comes from the
review renders in `--renders/<option>/sprite__<name>.png`, resized to exactly 2x if the render on
disk is not already that (renders are usually already exact -- `hd_upscale.render` guarantees it --
but a stale render from an older export size should not silently ship a mismatched picture).

Frame exports are cached under `artifacts/hd-review/_sprite_pack_frames/` (gitignored) and reused
across runs; delete that folder to force a re-export.

Combined pack, one command, mirroring `hd_portrait_pack.py`'s own CLI for the picture half:

    python3 tools/hd-review/sprite_pack.py imp.mpq combined.pack --with-pack-inputs \\
        --originals lomhd_work/originals/portrait --upscaled lomhd_work/upscaled/ultrasharp-tta/portrait
"""
from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys
import tempfile
import zlib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(ROOT / "tools"))
import hd_portrait_pack as pack  # noqa: E402
import hd_upscale  # noqa: E402
from png_index_patch import IndexedPng, PngError, read_indexed_png  # noqa: E402
sys.path.insert(0, str(HERE))
from sprite_originals import viewer_decodes_bgr  # noqa: E402  -- the same palette-decode check

MAX_RECORD_NAME_LEN = 39         # the DLL's name[40]: 39 characters plus a null terminator
WORK = ROOT / "artifacts" / "hd-review" / "_sprite_pack_frames"


def frame_count(viewer: pathlib.Path, archive: pathlib.Path, member: str,
                listfile: pathlib.Path) -> int | None:
    """How many `frame` records --describe-imp lists for this member, across every sequence and
    facing -- the count that says whether a sprite is static. `None` if the member is not in this
    archive (a name from a listfile aggregated across several profiles)."""
    result = subprocess.run([str(viewer), "--describe-imp", str(archive), member,
                             "--listfile", str(listfile)], capture_output=True, text=True)
    if result.returncode != 0:
        return None
    return sum(1 for line in result.stdout.splitlines() if line.startswith("frame\t"))


def transparent_key(png: IndexedPng) -> int | None:
    """The palette index the exporter's tRNS marks as fully transparent -- the sprite's colour
    key. `None` if there is not exactly one such index (nothing to key on, or an export this tool
    does not understand)."""
    for kind, payload in png.chunks:
        if kind == b"tRNS":
            zero = [index for index, alpha in enumerate(payload) if alpha == 0]
            return zero[0] if len(zero) == 1 else None
    return None


def pad_palette(plte: bytes) -> list[tuple[int, int, int]]:
    """PLTE as 256 (r, g, b) tuples; a PNG with fewer entries is padded with black, matching what
    `hd_portrait_pack.encode_record` requires of every record's palette."""
    entries = [tuple(plte[i:i + 3]) for i in range(0, len(plte) - len(plte) % 3, 3)]
    entries += [(0, 0, 0)] * (256 - len(entries))
    return entries[:256]


def identify_size(path: pathlib.Path) -> tuple[int, int]:
    out = subprocess.run(["magick", "identify", "-format", "%w %h", str(path)],
                         capture_output=True, text=True, check=True).stdout
    w, h = (int(v) for v in out.split())
    return w, h


def load_rgba(path: pathlib.Path) -> bytes:
    w, h = identify_size(path)
    data = subprocess.run(["magick", str(path), "-depth", "8", "RGBA:-"],
                          check=True, capture_output=True).stdout
    if len(data) != w * h * 4:
        raise SystemExit(f"{path}: {len(data)} bytes of RGBA, {w * h * 4} expected")
    return data


def load_hd_rgba(render: pathlib.Path, w: int, h: int) -> tuple[int, int, bytes]:
    """The render's pixels as straight RGBA, resized to exactly (2w, 2h) if it is not already --
    `hd_upscale.render` already guarantees that size, but a render picked from an older export
    should not silently ship a mismatched picture. Same resampling `hd_upscale.render` uses for a
    scale it must force: MagicKernelSharp2021, forced to size."""
    target_w, target_h = w * 2, h * 2
    rw, rh = identify_size(render)
    if (rw, rh) == (target_w, target_h):
        return target_w, target_h, load_rgba(render)
    with tempfile.TemporaryDirectory() as tmp:
        resized = pathlib.Path(tmp) / "resized.png"
        subprocess.run(["magick", str(render), "-filter", "MagicKernelSharp2021",
                       "-resize", f"{target_w}x{target_h}!", str(resized)], check=True)
        return target_w, target_h, load_rgba(resized)


def sprite_names(listfile: pathlib.Path) -> list[tuple[str, str]]:
    """(name, member) for every distinct `.imp` stem the listfile names -- two spellings of one
    member (case differences) collapse to a single entry, same dedup rule as
    `sprite_originals.py`. Which spelling of a tied name survives is not guaranteed."""
    members = sorted({m.strip() for m in listfile.read_text().splitlines()
                      if m.strip().lower().endswith(".imp")}, key=str.lower)
    seen: set[str] = set()
    out = []
    for member in members:
        name = member.split("\\")[-1].rsplit(".", 1)[0].lower()
        if name in seen:
            continue
        seen.add(name)
        out.append((name, member))
    return out


def build_sprite_records(archive: pathlib.Path, viewer: pathlib.Path, listfile: pathlib.Path,
                         renders: pathlib.Path, choices: dict) -> tuple[list, int, list[str]]:
    """(records, considered, skipped) -- every masked record this archive's static sprites can
    give, with what was left out and why."""
    names = sprite_names(listfile)
    if not viewer_decodes_bgr(viewer, archive, [member for _, member in names], listfile):
        raise SystemExit(f"{viewer} predates the palette fix and would export red and green "
                         "swapped: rebuild it (cargo build --release in spikes/asset-viewer)")
    WORK.mkdir(parents=True, exist_ok=True)

    records, skipped = [], []
    for name, member in names:
        record_name = f"sprite__{name}"
        if len(record_name) > MAX_RECORD_NAME_LEN:
            skipped.append(f"{name}: record name {record_name!r} is longer than the DLL's "
                           f"{MAX_RECORD_NAME_LEN}-character limit")
            continue

        frames = frame_count(viewer, archive, member, listfile)
        if frames is None:
            skipped.append(f"{name}: not in this archive")
            continue
        if frames != 1:
            skipped.append(f"{name}: {frames} frames, not a static sprite")
            continue

        raw = WORK / f"{name}.png"
        if not raw.exists():
            result = subprocess.run([str(viewer), "--export-imp-frame", str(archive), member, "0",
                                     str(raw), "--listfile", str(listfile)],
                                    capture_output=True, text=True)
            if result.returncode != 0 or not raw.exists():
                raw.unlink(missing_ok=True)
                skipped.append(f"{name}: could not export frame 0 ({result.stderr.strip()[:200]})")
                continue

        try:
            png = read_indexed_png(raw.read_bytes())
        except (PngError, zlib.error) as error:
            skipped.append(f"{name}: unreadable export ({error})")
            continue

        key = transparent_key(png)
        if key is None:
            skipped.append(f"{name}: not exactly one fully-transparent palette index")
            continue

        w, h, indices = png.width, png.height, bytes(png.indices)
        if not pack.masked_is_eligible(w, h, indices, key):
            skipped.append(f"{name}: {w}x{h} is too small, too large, or has no row with a long "
                           "enough run of opaque pixels for the matcher")
            continue

        choice = choices.get(record_name)
        if choice is None or choice not in hd_upscale.OPTIONS:
            skipped.append(f"{name}: no usable upscale pick ({choice!r})")
            continue

        render = renders / choice / f"{record_name}.png"
        if not render.exists():
            skipped.append(f"{name}: {choice} render is missing ({render})")
            continue

        try:
            hw, hh, rgba = load_hd_rgba(render, w, h)
            record = pack.encode_record(record_name, w, h, indices, pad_palette(png.palette()),
                                        hw, hh, rgba, flags=pack.FLAG_MASKED, key=key)
        except (ValueError, SystemExit, subprocess.CalledProcessError) as error:
            skipped.append(f"{name}: {error}")
            continue

        records.append(record)
    return records, len(names), skipped


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("archive", type=pathlib.Path)
    parser.add_argument("out", type=pathlib.Path)
    parser.add_argument("--renders", type=pathlib.Path, default=ROOT / "artifacts" / "hd-review")
    parser.add_argument("--choices", type=pathlib.Path,
                        default=ROOT / "release" / "hd-overlay" / "upscale-choices.json")
    parser.add_argument("--viewer", type=pathlib.Path,
                        default=ROOT / "spikes/asset-viewer/target/release/lom-asset-viewer")
    parser.add_argument("--listfile", type=pathlib.Path,
                        default=ROOT / "reports/member-names/all-profiles-imp-recovered.txt")
    parser.add_argument("--with-pack-inputs", action="store_true",
                        help="also pack pictures, mirroring hd_portrait_pack.py's own CLI")
    parser.add_argument("--originals", type=pathlib.Path, action="append",
                        help="picture originals (with --with-pack-inputs)")
    parser.add_argument("--upscaled", type=pathlib.Path, action="append",
                        help="picture upscales (with --with-pack-inputs)")
    parser.add_argument("--sources", type=pathlib.Path, action="append",
                        help="picture originals the upscales were made from (defaults to --originals)")
    args = parser.parse_args()

    if args.with_pack_inputs and not (args.originals and args.upscaled):
        parser.error("--with-pack-inputs needs --originals and --upscaled")

    choices = json.loads(args.choices.read_text())["choices"]
    records, considered, skipped = build_sprite_records(
        args.archive, args.viewer, args.listfile, args.renders, choices)

    picture_skipped: list[str] = []

    def all_records():
        yield from records
        if args.with_pack_inputs:
            yield from pack.unmasked_records(args.originals, args.upscaled, picture_skipped, args.sources)

    n = pack.write_records(args.out, all_records())
    print(f"sprites: {considered} considered, {len(records)} packed, {len(skipped)} skipped")
    for line in skipped:
        print(f"  left out -- {line}")
    if args.with_pack_inputs:
        print(f"pictures: {n - len(records)} packed, {len(picture_skipped)} skipped")
        for line in picture_skipped:
            print(f"  left out -- {line}")
    print(f"{args.out}: {n} images total, {args.out.stat().st_size / 1e6:.1f} MB")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
