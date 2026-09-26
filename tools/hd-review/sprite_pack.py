#!/usr/bin/env python3
"""Pack IMP sprites (and, optionally, the existing pictures) into a format-5 HD pack, for a DLL
test that covers masked records rather than only the picture path.

    python3 tools/hd-review/sprite_pack.py ARCHIVE OUT.pack --esrgan PATH --models DIR
        [--renders DIR] [--choices FILE] [--listfile PATH] [--animated]
        [--with-pack-inputs --originals DIR [...] --upscaled DIR [...] [--sources DIR [...]]]

The dev front end of what the player's setup does (`release/hd-overlay/lomhd_setup.py`): both run
`tools/hd_sprites.py` over sprites decoded by `tools/imp_read.py`, so a pack built here is built the
way players build theirs. See hd_sprites.py for what is packed and why.

STATIC sprites always; `--animated` adds every frame of every animated sprite (hours of rendering;
it resumes). Work files go under `artifacts/hd-review/_sprites/` (gitignored), keyed per member by
its path and its own bytes, as setup keys them. `--renders` points at the review's renders
(`<option>/sprite__<name>.png`): a static sprite's review render was made from the same prepared
frame, so it is copied in rather than rendered again. It is still refused if it is not exactly 2x.

Combined pack, one command, mirroring `hd_portrait_pack.py`'s own CLI for the picture half:

    python3 tools/hd-review/sprite_pack.py imp.mpq combined.pack --esrgan ... --models ... \\
        --with-pack-inputs --originals lomhd_work/originals/portrait \\
        --upscaled lomhd_work/upscaled/ultrasharp-tta/portrait
"""
from __future__ import annotations

import argparse
import itertools
import json
import pathlib
import shutil
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(ROOT / "tools"))
import hd_portrait_pack as pack  # noqa: E402
import hd_sprites  # noqa: E402
import hd_upscale  # noqa: E402
import mpq_read  # noqa: E402

WORK = ROOT / "artifacts" / "hd-review" / "_sprites"


def resolve_sprites(archive, listfile: pathlib.Path):
    """(name -> (member, frame count), considered, skipped) for this archive: setup's own resolver
    (`hd_sprites.resolve`), so a damaged member is a skip here too, not a stop."""
    found = hd_sprites.resolve(archive, listfile)
    return found.resolved, found.considered, found.skipped


def seed_review_renders(sprites, root: pathlib.Path, renders: pathlib.Path) -> int:
    """Copy each static sprite's review render in as its render, where there is one and none was
    rendered here yet. How many were copied."""
    copied = 0
    for sprite in sprites:
        frame = sprite.frames[0]
        review = renders / sprite.option / f"{hd_sprites.static_record_name(sprite.name)}.png"
        dest = root / "render" / sprite.option / f"{frame.stem}.png"
        if review.is_file() and not dest.exists():
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(review, dest)
            copied += 1
    return copied


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("archive", type=pathlib.Path)
    parser.add_argument("out", type=pathlib.Path)
    parser.add_argument("--esrgan", type=pathlib.Path, required=True, help="realesrgan-ncnn-vulkan")
    parser.add_argument("--models", type=pathlib.Path, required=True, help="its models folder")
    parser.add_argument("--renders", type=pathlib.Path,
                        help="the review's renders, reused for static sprites where present")
    parser.add_argument("--choices", type=pathlib.Path,
                        default=ROOT / "release" / "hd-overlay" / "upscale-choices.json")
    parser.add_argument("--listfile", type=pathlib.Path,
                        default=ROOT / "reports/member-names/all-profiles-imp-recovered.txt")
    parser.add_argument("--animated", action="store_true",
                        help="also every frame of every animated sprite (upscaled here: slow)")
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
    archive = mpq_read.Archive(args.archive)
    read_sprite = hd_sprites.archive_reader(archive)
    resolved, considered, skipped = resolve_sprites(archive, args.listfile)
    root = WORK
    plan = hd_sprites.plan(resolved, read_sprite, choices, root, animated=args.animated,
                           log=lambda line: print(f"  {line}", flush=True))
    skipped += plan.skipped
    if args.renders:
        print(f"{seed_review_renders(plan.static, root, args.renders)} static renders reused from "
              f"{args.renders}", flush=True)
    if args.animated:
        c = plan.counts
        print(f"animated: {len(plan.animated)} sprites, {sum(len(s.frames) for s in plan.animated)} "
              f"frames to pack of {c['frames']} ({c['repeats']} repeats, {c['ineligible']} too small "
              f"for the matcher, {c['no_probe']} with no probe of enough colours)", flush=True)

    def render(option, inputs, dest):
        hd_upscale.render(option, inputs, dest, args.esrgan, args.models)

    sprites = plan.static + plan.animated
    hd_sprites.render_all(sprites, root, render, log=lambda line: print(f"  {line}", flush=True))

    left_out: list[str] = []
    picture_skipped: list[str] = []
    counts: dict = {}
    records = hd_sprites.records(sprites, root, read_sprite, left_out, counts)
    if args.with_pack_inputs:
        records = itertools.chain(records, pack.unmasked_records(args.originals, args.upscaled,
                                                                 picture_skipped, args.sources))
    n = pack.write_records(args.out, records)
    print(f"sprites: {considered} considered, {len(plan.static)} static and {len(plan.animated)} "
          f"animated planned, {counts['packed']} frames packed, {len(skipped) + len(left_out)} left out")
    for line in skipped + left_out:
        print(f"  left out -- {line}")
    if args.with_pack_inputs:
        print(f"pictures: packed, {len(picture_skipped)} skipped")
        for line in picture_skipped:
            print(f"  left out -- {line}")
    print(f"{args.out}: {n} images total, {args.out.stat().st_size / 1e6:.1f} MB")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
