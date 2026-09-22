#!/usr/bin/env python3
"""Render original-vs-upscaled PNG pairs for a sample of IMP sprites, for side-by-side review.

    python3 tools/sprite-review/generate.py OUT_DIR --archive imp.mpq --listfile NAMES.txt \
        --members MEMBERS.txt --esrgan PATH --models DIR [--model ultrasharp-4x]

This produces PICTURES TO LOOK AT. It does not write `.imp` files and cannot: see
docs/resolution-and-upscaling.md, "The same upscaler on SPRITES". Two format properties stop an
upscaled sprite going back into the game, and both are visible in the output this script makes:

  * **IMP transparency is 1-bit.** A source frame has 2 distinct alpha values; the model's output
    has ~240. There is no alpha channel to put them in, so they would have to be thresholded back,
    discarding the softened silhouette that is most of what the model added.
  * **Palette index 1 is the shadow**, keyed by index rather than colour (Observed in gameplay
    2026-09-17, docs/hotspots.md). An RGB upscale interpolates it against its neighbours, which is
    why the stipple smears. Nothing here lifts it out as a mask first.

A third, not visible here: doubling a frame without doubling its anchor and hotspot records puts
the unit in the wrong place.

So the review this feeds answers one narrow question -- *is the damage bad enough to be worth
fixing before the other 1,770 units* -- and not "is this art good".
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import tempfile

FACING = re.compile(r"^facing\t(\d+)\tsequence:(\d+)\t\S*\t\S*\tframe:(\d+)\tframe:(\d+)")
SEQUENCE = re.compile(r"^sequence\t(\d+)\t\S*\t(\S+)\t")


def run(cmd: list[str], **kw) -> subprocess.CompletedProcess:
    result = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if result.returncode != 0:
        raise SystemExit(f"failed: {' '.join(str(c) for c in cmd[:4])}...\n{result.stderr.strip()[:400]}")
    return result


def describe(viewer: pathlib.Path, archive: pathlib.Path, member: str,
             listfile: pathlib.Path) -> list[dict]:
    """Parse --describe-imp into [{label, facings: [[frame indices]]}]."""
    out = run([str(viewer), "--describe-imp", str(archive), member, "--listfile", str(listfile)]).stdout
    labels: dict[int, str] = {}
    facings: list[tuple[int, int, int]] = []
    for line in out.splitlines():
        m = SEQUENCE.match(line)
        if m:
            labels[int(m.group(1))] = m.group(2)
            continue
        m = FACING.match(line)
        if m:
            _, seq, first, count = (int(g) for g in m.groups())
            facings.append((seq, first, count))
    sequences: dict[int, list[list[int]]] = {}
    for seq, first, count in facings:
        sequences.setdefault(seq, []).append(list(range(first, first + count)))
    return [{"label": labels.get(seq, f"SEQ{seq}"), "facings": rows}
            for seq, rows in sorted(sequences.items())]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("out_dir", type=pathlib.Path)
    parser.add_argument("--archive", type=pathlib.Path, required=True)
    parser.add_argument("--listfile", type=pathlib.Path, required=True)
    parser.add_argument("--members", type=pathlib.Path, required=True)
    parser.add_argument("--esrgan", type=pathlib.Path, required=True)
    parser.add_argument("--models", type=pathlib.Path, required=True)
    parser.add_argument("--model", default="ultrasharp-4x")
    parser.add_argument("--frames-per-facing", type=int, default=1,
                        help="frames to render from each facing. Every facing is always covered; "
                             "frames WITHIN one facing are near-duplicates for judging quality, so "
                             "the default renders the first of each and keeps directions complete.")
    parser.add_argument("--viewer", type=pathlib.Path,
                        default=pathlib.Path("spikes/asset-viewer/target/debug/lom-asset-viewer"))
    args = parser.parse_args()

    images = args.out_dir / "images"
    images.mkdir(parents=True, exist_ok=True)
    members = [m for m in args.members.read_text().splitlines() if m.strip()]

    manifest = []
    with tempfile.TemporaryDirectory() as td:
        tmp = pathlib.Path(td)
        for n, member in enumerate(members, 1):
            unit = member.split("\\")[-1].rsplit(".", 1)[0]
            sequences = describe(args.viewer, args.archive, member, args.listfile)
            keep = args.frames_per_facing
            for s in sequences:
                s["facings"] = [row[:keep] for row in s["facings"]]
            frames = sorted({f for s in sequences for row in s["facings"] for f in row})
            print(f"[{n}/{len(members)}] {unit}: {len(sequences)} sequences, {len(frames)} frames",
                  flush=True)
            sizes: dict[str, list[int]] = {}
            for frame in frames:
                orig = images / f"{unit}__{frame:03d}.orig.png"
                run([str(args.viewer), "--export-imp-frame", str(args.archive), member,
                     str(frame), str(orig), "--listfile", str(args.listfile)])
                # 4x then down to 2x: supersampling gives a cleaner 2x than asking for 2x directly.
                run([str(args.esrgan), "-i", str(orig), "-o", str(tmp / "up.png"),
                     "-n", args.model, "-m", str(args.models), "-s", "4"])
                run(["magick", str(tmp / "up.png"), "-filter", "MagicKernelSharp2021",
                     "-resize", "50%", str(images / f"{unit}__{frame:03d}.new.png")])
                probe = run(["magick", "identify", "-format", "%w %h", str(orig)]).stdout.split()
                sizes[str(frame)] = [int(probe[0]), int(probe[1])]
            manifest.append({"id": unit, "member": member,
                             "sequences": sequences, "sizes": sizes})

    (args.out_dir / "manifest.json").write_text(json.dumps(manifest, indent=1))
    total = sum(len(u["sizes"]) for u in manifest)
    print(f"{len(manifest)} units, {total} frames, {total * 2} images -> {args.out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
