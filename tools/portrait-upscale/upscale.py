#!/usr/bin/env python3
"""Upscale LBM portraits to a larger size, keeping each one's own palette.

    python3 tools/portrait-upscale/upscale.py SRC_DIR OUT_DIR --names NAMES.txt \
        --esrgan PATH --models DIR [--model ultrasharp-4x] [--size 140x134]

SRC_DIR holds the members extracted from `pic.mpq`; NAMES.txt lists the member names to process,
one per line, in the archive's own spelling (`lom-mpq list`). Output lands under OUT_DIR at the same
relative paths, ready to be copied into a mod tree's `archives/pic.mpq/`.

The engine side of this is settled -- see
docs/resolution-and-upscaling.md, "CLOSED: the 70x67 portrait was never an engine limit".
An oversize portrait loads, a doodad crops or paints at whatever size its rect asks for, and a
click follows the drawn position. This script only makes the pixels.

**The palette is carried across unchanged, and that is a refusal rather than a shortcut.** There are
193 distinct palettes across the 749 shipped portraits and **not one entry is common to all of
them**, so the corpus cannot say whether the engine reads each member's `CMAP` or remaps it onto a
screen palette. Re-quantising into the member's existing colours is correct under either answer,
because those colours already display correctly today. Introducing a new palette is untested and
would need its own attended run.

**The model invents; measure it rather than arguing about it.** `--report-fidelity` downscales each
result back to the source size and prints the mean and worst deviation from the original. Measured
2026-09-21 over three portraits: faithful resampling 4.8/255 mean, `ultrasharp-4x` 9.7 with local
deviations to 56. Report that number with any art this produces.

Needs `magick` (ImageMagick 7) on PATH and a Real-ESRGAN ncnn binary plus its models; neither is
vendored. The models used here came from upscayl/upscayl's `resources/models`.
"""
from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import lbm_png  # noqa: E402


def run(cmd: list[str]) -> None:
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise SystemExit(f"failed: {' '.join(cmd[:3])}...\n{result.stderr.strip()[:400]}")


def nearest_index(palette: list[tuple[int, int, int]], colour: tuple[int, int, int]) -> int:
    return min(range(len(palette)),
               key=lambda k: sum((palette[k][j] - colour[j]) ** 2 for j in range(3)))


def upscale_one(src: pathlib.Path, dest: pathlib.Path, tmp: pathlib.Path,
                esrgan: pathlib.Path, models: pathlib.Path, model: str,
                width: int, height: int) -> tuple[float, float] | None:
    w, h, px, palette, bmhd = lbm_png.decode(src)
    lbm_png.write_png(tmp / "in.png", w, h,
                      [[palette[px[y * w + x]] for x in range(w)] for y in range(h)])
    lbm_png.write_png(tmp / "pal.png", len(palette), 1, [list(palette)])

    # De-speckle first: the source dither is noise to the model, and feeding it through raw
    # produces speckle rather than tone. Measured better on every subject tried.
    run(["magick", str(tmp / "in.png"), "-despeckle", str(tmp / "dd.png")])
    run([str(esrgan), "-i", str(tmp / "dd.png"), "-o", str(tmp / "up.png"),
         "-n", model, "-m", str(models), "-s", "4"])
    run(["magick", str(tmp / "up.png"), "-filter", "MagicKernelSharp2021",
         "-resize", f"{width}x{height}!", "-dither", "FloydSteinberg",
         "-remap", str(tmp / "pal.png"), "PNG24:" + str(tmp / "q.png")])

    _, _, rows = lbm_png.read_png_rgb(tmp / "q.png")
    lut: dict[tuple[int, int, int], int] = {}
    for index, colour in enumerate(palette):
        lut.setdefault(colour, index)
    indices = bytearray()
    for row in rows:
        for colour in row:
            index = lut.get(colour)
            if index is None:            # -remap should make this unreachable; be explicit, not lucky
                index = lut[colour] = nearest_index(palette, colour)
            indices.append(index)

    dest.parent.mkdir(parents=True, exist_ok=True)
    lbm_png.encode(dest, width, height, bytes(indices), palette, bmhd)
    return None


def fidelity(src: pathlib.Path, result: pathlib.Path, tmp: pathlib.Path) -> tuple[float, int]:
    """Mean and worst per-pixel deviation after downscaling the result back to the source size.

    This is the honest measure of how much the model invented: a faithful upscale round-trips to
    something close to the original, a hallucinating one does not.
    """
    w, h, px, palette, _ = lbm_png.decode(src)
    rw, rh, rpx, rpal, _ = lbm_png.decode(result)
    lbm_png.write_png(tmp / "f-new.png", rw, rh,
                      [[rpal[rpx[y * rw + x]] for x in range(rw)] for y in range(rh)])
    run(["magick", str(tmp / "f-new.png"), "-filter", "Box", "-resize", f"{w}x{h}!",
         "PNG24:" + str(tmp / "f-back.png")])
    _, _, back = lbm_png.read_png_rgb(tmp / "f-back.png")
    total = count = 0.0
    worst = 0
    for y in range(h):
        for x in range(w):
            a = palette[px[y * w + x]]
            b = back[y][x]
            d = sum(abs(a[i] - b[i]) for i in range(3)) / 3
            total += d
            count += 1
            worst = max(worst, int(d))
    return total / count, worst


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("src_dir", type=pathlib.Path)
    parser.add_argument("out_dir", type=pathlib.Path)
    parser.add_argument("--names", type=pathlib.Path, required=True)
    parser.add_argument("--esrgan", type=pathlib.Path, required=True,
                        help="path to realesrgan-ncnn-vulkan")
    parser.add_argument("--models", type=pathlib.Path, required=True)
    parser.add_argument("--model", default="ultrasharp-4x")
    parser.add_argument("--size", default="140x134")
    parser.add_argument("--report-fidelity", action="store_true",
                        help="also measure how far each result drifts from the original")
    args = parser.parse_args()

    width, height = (int(v) for v in args.size.lower().split("x"))
    names = [n for n in args.names.read_text().splitlines() if n.strip()]
    by_name = {p.name.lower(): p for p in args.src_dir.rglob("*") if p.is_file()}

    done = 0
    missing: list[str] = []
    scores: list[tuple[str, float, int]] = []
    with tempfile.TemporaryDirectory() as td:
        tmp = pathlib.Path(td)
        for i, member in enumerate(names, 1):
            src = by_name.get(member.split("\\")[-1].lower())
            if src is None:
                missing.append(member)
                continue
            dest = args.out_dir / member.replace("\\", "/")
            upscale_one(src, dest, tmp, args.esrgan, args.models, args.model, width, height)
            if args.report_fidelity:
                mean, worst = fidelity(src, dest, tmp)
                scores.append((member, mean, worst))
            done += 1
            if i % 25 == 0:
                print(f"  {i}/{len(names)}", flush=True)

    print(f"{done} written to {args.out_dir}, {len(missing)} missing")
    for member in missing[:10]:
        print(f"  MISSING {member}")
    if scores:
        mean = sum(s[1] for s in scores) / len(scores)
        worst = max(s[2] for s in scores)
        print(f"fidelity: mean deviation {mean:.2f}/255, worst local {worst}/255 over {len(scores)}")
        for member, m, w in sorted(scores, key=lambda s: -s[1])[:5]:
            print(f"  drifted most: {member}  mean {m:.2f}  worst {w}")
    return 1 if missing else 0


if __name__ == "__main__":
    raise SystemExit(main())
