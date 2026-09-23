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
review renders in `--renders/<option>/sprite__<name>.png`, which `hd_upscale.render` always makes
exactly 2x the frame it was made from. A render of a DIFFERENT size is refused, not resized: it was
made from a different original (a stale render, or the classic case of two different members
sharing a record name -- see `imp_members.py`), and resizing it would ship art for a shape it was
never upscaled from.

Frame exports are cached under `artifacts/hd-review/_sprite_pack_frames/` (gitignored) and reused
across runs, keyed by a content hash of the archive, the exact member path, and the sprite's name --
two different archives, or two different members resolved for the same name across two builds of
the SAME archive (see `imp_members.py`), never share a cached export. Delete the folder to force
every export to redo anyway.

Combined pack, one command, mirroring `hd_portrait_pack.py`'s own CLI for the picture half:

    python3 tools/hd-review/sprite_pack.py imp.mpq combined.pack --with-pack-inputs \\
        --originals lomhd_work/originals/portrait --upscaled lomhd_work/upscaled/ultrasharp-tta/portrait
"""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
import zlib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(ROOT / "tools"))
import hd_portrait_pack as pack  # noqa: E402
import hd_upscale  # noqa: E402
from png_index_patch import IndexedPng, PngError, read_indexed_png  # noqa: E402
sys.path.insert(0, str(HERE))
from imp_members import candidate_members, resolve_members  # noqa: E402
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
    """The render's pixels as straight RGBA. `hd_upscale.render` always makes a render exactly
    (2w, 2h); a DIFFERENT size means this render was made from a different original -- a stale
    render left over from a prior export size, or the classic case of two different members
    sharing a record name (`imp_members.py`) -- not a shape worth silently fixing up by resizing.
    Refuses instead, so the caller reports it as a skip like every other reason."""
    target_w, target_h = w * 2, h * 2
    rw, rh = identify_size(render)
    if (rw, rh) != (target_w, target_h):
        raise SystemExit(f"render is {rw}x{rh}, expected {target_w}x{target_h}: made from a "
                         "different original")
    return target_w, target_h, load_rgba(render)


def archive_fingerprint(archive: pathlib.Path) -> str:
    """A short, content-based key for `archive`, so a cached frame export can never leak into a
    different archive's pack -- or a rebuilt archive that reused the same path -- even though the
    cache folder is reused across runs. Hashed rather than sized/dated: a rebuilt archive (as in a
    test, or a re-exported profile) can keep both its path and its size."""
    digest = hashlib.sha256()
    with archive.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()[:16]


def member_fingerprint(member: str) -> str:
    """A short, case-normalized key for one member PATH, not just its basename -- so a cached
    frame export is also keyed by WHICH candidate spelling was actually resolved for a name (see
    `imp_members.resolve_members`). Without this, one archive built once with a listfile naming
    only `aura\\agx06b.imp` and once with a listfile naming only `imp\\agx06b.imp` would have the
    second build's "agx06b" reuse the first build's export, even though the two are unrelated
    sprites that merely share a record name."""
    return hashlib.sha256(member.lower().encode("utf-8")).hexdigest()[:12]


def build_sprite_records(archive: pathlib.Path, viewer: pathlib.Path, listfile: pathlib.Path,
                         renders: pathlib.Path, choices: dict) -> tuple[list, int, list[str]]:
    """(records, considered, skipped) -- every masked record this archive's static sprites can
    give, with what was left out and why."""
    grouped = candidate_members(listfile)

    # An oversized name is refused before any candidate for it is even tried against the archive
    # -- it would be refused regardless of which spelling turned out to be present, so there is no
    # reason to pay for a --describe-imp call first.
    skipped: list[str] = []
    usable: dict[str, list[str]] = {}
    for name, candidates in grouped.items():
        record_name = f"sprite__{name}"
        if len(record_name) > MAX_RECORD_NAME_LEN:
            skipped.append(f"{name}: record name {record_name!r} is longer than the DLL's "
                           f"{MAX_RECORD_NAME_LEN}-character limit")
            continue
        usable[name] = candidates

    all_paths = [member for candidates in usable.values() for member in candidates]
    if not viewer_decodes_bgr(viewer, archive, all_paths, listfile):
        raise SystemExit(f"{viewer} predates the palette fix and would export red and green "
                         "swapped: rebuild it (cargo build --release in spikes/asset-viewer)")
    WORK.mkdir(parents=True, exist_ok=True)
    fingerprint = archive_fingerprint(archive)

    # Try every spelling a name has anywhere in the listfile: which of them, if any, THIS
    # particular archive actually holds is not known until now. Deduplicating to one candidate
    # before this point can silently keep a spelling absent from this archive while a present one
    # under a different folder is never even tried; if more than one spelling is present, that is
    # reported as an ambiguous collision rather than a silent pick (see imp_members.py).
    resolved, resolve_skipped = resolve_members(
        usable, lambda member: frame_count(viewer, archive, member, listfile))
    skipped += resolve_skipped

    records = []
    for name, (member, frames) in resolved.items():
        record_name = f"sprite__{name}"
        if frames != 1:
            skipped.append(f"{name}: {frames} frames, not a static sprite")
            continue

        # Keyed by archive, member path AND name: two different builds of the SAME archive can
        # resolve two different members for one record name (one listfile naming only the aura\\
        # spelling, another naming only the imp\\ spelling), and each must export its own frame.
        raw = WORK / f"{fingerprint}__{member_fingerprint(member)}__{name}.png"
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

        # The upscale is always exactly (2w, 2h) (`load_hd_rgba` forces it), so the DLL's own
        # limit on the upscale's side -- MAX_UPSCALE_SIDE -- can be checked here, before the render
        # is even looked up: a narrow-but-tall or wide-but-short sprite can pass the eligibility
        # rule above (which only bounds w*h) and still upscale past what the DLL will load, and a
        # SINGLE oversized record makes the DLL refuse the WHOLE pack, not just that one image.
        try:
            pack.check_reader_limits(name, w, h, w * 2, h * 2, masked=True)
        except SystemExit as error:
            skipped.append(str(error))
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
    return records, len(grouped), skipped


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
