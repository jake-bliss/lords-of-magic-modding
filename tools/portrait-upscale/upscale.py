#!/usr/bin/env python3
"""Upscale LBM portraits to a larger size, keeping each one's own palette.

    python3 tools/portrait-upscale/upscale.py SRC_DIR OUT_DIR --names NAMES.txt \
        --esrgan PATH --models DIR [--model ultrasharp-4x] [--size 140x134]

SRC_DIR holds the members extracted from `pic.mpq`; NAMES.txt lists the member names to process,
one per line, in the archive's own spelling (`lom-mpq list`). Output lands under OUT_DIR at the same
relative paths, ready to be copied into a mod tree's `archives/pic.mpq/`.

The engine side of this is settled **for GS5R3** -- see docs/resolution-and-upscaling.md,
"CLOSED: the 70x67 portrait was never an engine limit". An oversize portrait loads, a doodad crops
or paints at whatever size its rect asks for, and a click follows the drawn position. All three runs
were GS5R3; vanilla and 3.02 have a different portrait namer and were not tested. This script only
makes the pixels.

**The palette is carried across unchanged, and that is a refusal rather than a shortcut.** There are
193 distinct palettes across the 749 shipped portraits and **not one entry is common to all of
them**, so the corpus cannot say whether the engine reads each member's `CMAP` or remaps it onto a
screen palette. Re-quantising into the member's existing colours is correct under either answer,
because those colours already display correctly today. Introducing a new palette is untested.

**The model invents; measure it rather than arguing about it.** `--report-fidelity` downscales each
result back to the source size and prints the mean and worst deviation from the original. Measured
2026-09-21: faithful resampling 4.8/255 mean, `ultrasharp-4x` 9.7 with local deviations to 56.
Report that number with any art this produces. Note the measure only covers the variant this script
produces; the classical and blend figures came from a one-off comparison and are not reproducible
from here.

Needs `magick` (ImageMagick 7) on PATH and a Real-ESRGAN ncnn binary plus its models; neither is
vendored.
"""
from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import lbm_png  # noqa: E402


def run(cmd: list[str]) -> None:
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise SystemExit(f"failed: {' '.join(cmd[:3])}...\n{result.stderr.strip()[:400]}")


def nearest_index(palette: list[tuple[int, int, int]], colour: tuple[int, int, int]) -> int:
    return min(range(len(palette)),
               key=lambda k: sum((palette[k][j] - colour[j]) ** 2 for j in range(3)))


def upscale_one(src: pathlib.Path, dest: pathlib.Path, work: pathlib.Path,
                esrgan: pathlib.Path, models: pathlib.Path, model: str,
                width: int, height: int) -> None:
    w, h, px, palette, chunks = lbm_png.decode(src)
    lbm_png.write_png(work / "in.png", w, h,
                      [[palette[px[y * w + x]] for x in range(w)] for y in range(h)])
    lbm_png.write_png(work / "pal.png", len(palette), 1, [list(palette)])

    # De-speckle first: the source dither is noise to the model, and feeding it through raw
    # produces speckle rather than tone. Measured better on every subject tried.
    run(["magick", str(work / "in.png"), "-despeckle", str(work / "dd.png")])
    run([str(esrgan), "-i", str(work / "dd.png"), "-o", str(work / "up.png"),
         "-n", model, "-m", str(models), "-s", "4"])
    run(["magick", str(work / "up.png"), "-filter", "MagicKernelSharp2021",
         "-resize", f"{width}x{height}!", "-dither", "FloydSteinberg",
         "-remap", str(work / "pal.png"), "PNG24:" + str(work / "q.png")])

    _, _, rows = lbm_png.read_png_rgb(work / "q.png")
    # 666 of the 749 shipped portraits hold the same RGB at more than one index, and the
    # intermediate PNG has already discarded which one a pixel came from -- so a colour can come
    # back as a DIFFERENT index that looks identical. Measured 2026-09-21: no ACTIVE `CRNG` range
    # in any of the 749 covers a duplicated colour, so nothing animated depends on the distinction
    # in this corpus. It is an imprecision with no measured consequence, not a proof of safety.
    lut: dict[tuple[int, int, int], int] = {}
    for index, colour in enumerate(palette):
        lut.setdefault(colour, index)
    indices = bytearray()
    for row in rows:
        for colour in row:
            index = lut.get(colour)
            if index is None:          # -remap should make this unreachable; be explicit, not lucky
                index = lut[colour] = nearest_index(palette, colour)
            indices.append(index)

    dest.parent.mkdir(parents=True, exist_ok=True)
    lbm_png.encode(dest, width, height, bytes(indices), palette, chunks)


def fidelity(src: pathlib.Path, result: pathlib.Path, work: pathlib.Path) -> tuple[float, float]:
    """Mean and worst per-pixel deviation after downscaling the result back to the source size.

    ⚠️ This measures the WHOLE pipeline -- despeckle, model, resample, dither, quantise, downsample
    -- not the model alone. It is the right number for "how far did the shipped art drift from the
    original", and the wrong one for attributing that drift to any single stage.
    """
    w, h, px, palette, _ = lbm_png.decode(src)
    rw, rh, rpx, rpal, _ = lbm_png.decode(result)
    lbm_png.write_png(work / "f-new.png", rw, rh,
                      [[rpal[rpx[y * rw + x]] for x in range(rw)] for y in range(rh)])
    run(["magick", str(work / "f-new.png"), "-filter", "Box", "-resize", f"{w}x{h}!",
         "PNG24:" + str(work / "f-back.png")])
    _, _, back = lbm_png.read_png_rgb(work / "f-back.png")
    total = 0.0
    worst = 0
    for y in range(h):
        for x in range(w):
            a = palette[px[y * w + x]]
            b = back[y][x]
            deviation = sum(abs(a[i] - b[i]) for i in range(3)) / 3
            total += deviation
            worst = max(worst, deviation)      # not int(): truncating understates the worst case
    return total / (w * h), worst


def index_sources(src_dir: pathlib.Path) -> tuple[dict[str, pathlib.Path], dict[str, int]]:
    """Index by relative path AND by basename, so a name can be matched either way.

    Keying on basename alone lets two members with the same filename in different `pic.mpq`
    directories collide silently, with whichever the walk reached last winning. The basename map
    therefore records collisions so they can be reported rather than resolved by luck.
    """
    by_path: dict[str, pathlib.Path] = {}
    counts: dict[str, int] = {}
    by_base: dict[str, pathlib.Path] = {}
    for path in src_dir.rglob("*"):
        if not path.is_file():
            continue
        by_path[str(path.relative_to(src_dir)).replace("/", "\\").lower()] = path
        base = path.name.lower()
        counts[base] = counts.get(base, 0) + 1
        by_base.setdefault(base, path)
    by_path.update({k: v for k, v in by_base.items() if k not in by_path})
    return by_path, counts


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("src_dir", type=pathlib.Path)
    parser.add_argument("out_dir", type=pathlib.Path)
    parser.add_argument("--names", type=pathlib.Path, required=True)
    parser.add_argument("--esrgan", type=pathlib.Path, required=True,
                        help="path to realesrgan-ncnn-vulkan")
    parser.add_argument("--models", type=pathlib.Path, required=True)
    parser.add_argument("--model", default="ultrasharp-4x")
    parser.add_argument("--size", default="140x134", help="WIDTHxHEIGHT, e.g. 140x134")
    parser.add_argument("--report-fidelity", action="store_true",
                        help="also measure how far each result drifts from the original")
    args = parser.parse_args()

    try:
        width, height = (int(v) for v in args.size.lower().split("x", 1))
    except ValueError:
        parser.error(f"--size must be WIDTHxHEIGHT, got {args.size!r}")

    names = [n for n in args.names.read_text().splitlines() if n.strip()]
    sources, basename_counts = index_sources(args.src_dir)

    done = 0
    missing: list[str] = []
    ambiguous: list[str] = []
    scores: list[tuple[str, float, float]] = []
    work_root = args.out_dir / ".work"
    for i, member in enumerate(names, 1):
        key = member.replace("/", "\\").lower()
        src = sources.get(key) or sources.get(member.split("\\")[-1].lower())
        if src is None:
            missing.append(member)
            continue
        if key not in sources and basename_counts.get(src.name.lower(), 0) > 1:
            ambiguous.append(member)      # matched by basename, and that basename is not unique
            continue
        # A scratch directory per member: shared scratch names mean a stage that exits 0 without
        # writing silently hands the PREVIOUS member's file to the next stage.
        work = work_root / f"{i:05d}"
        work.mkdir(parents=True, exist_ok=True)
        dest = args.out_dir / member.replace("\\", "/")
        upscale_one(src, dest, work, args.esrgan, args.models, args.model, width, height)
        if args.report_fidelity:
            mean, worst = fidelity(src, dest, work)
            scores.append((member, mean, worst))
        for leftover in work.iterdir():
            leftover.unlink()
        work.rmdir()
        done += 1
        if i % 25 == 0:
            print(f"  {i}/{len(names)}", flush=True)
    if work_root.exists():
        work_root.rmdir()

    print(f"{done} written to {args.out_dir}, {len(missing)} missing, {len(ambiguous)} ambiguous")
    for member in missing[:10]:
        print(f"  MISSING {member}")
    for member in ambiguous[:10]:
        print(f"  AMBIGUOUS {member}: basename occurs more than once under {args.src_dir}")
    if scores:
        mean = sum(s[1] for s in scores) / len(scores)
        worst = max(s[2] for s in scores)
        print(f"fidelity: mean deviation {mean:.2f}/255, worst local {worst:.1f}/255 over {len(scores)}")
        for member, m, w in sorted(scores, key=lambda s: -s[1])[:5]:
            print(f"  drifted most: {member}  mean {m:.2f}  worst {w:.1f}")
    return 1 if (missing or ambiguous) else 0


if __name__ == "__main__":
    raise SystemExit(main())
