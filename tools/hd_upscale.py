"""The upscale options, defined once: the review renderer and the player's setup both run these.

What Jake reviewed on 2026-09-22 is what players get only if both run the same code, so neither
keeps its own copy of the model list or the resize.

  approved        the original portrait pipeline (despeckle, 4x-UltraSharp, shrink, dither back to
                  the image's own 256 colours) -- tools/portrait-upscale/upscale.py. Kept for
                  character portraits, where it was reviewed and approved.
  ultrasharp      4x-UltraSharp, shrunk to 2x, full colour
  ultrasharp-tta  the same with test-time augmentation (8 flipped/rotated passes averaged)
  anime2x         realesr-animevideov3 at its native 2x
  anime4x         realesr-animevideov3 at 4x, shrunk to 2x

Full-colour options neither despeckle nor remap to a palette: those two steps are what lost detail
in the first building upscales, and the overlay draws its own texture, so it needs neither.
"""
from __future__ import annotations

import os
import pathlib
import re
import shutil
import subprocess
import tempfile

OPTIONS = {                      # name -> (model, scale, extra realesrgan-ncnn-vulkan args)
    "ultrasharp": ("ultrasharp-4x", 4, []),
    "anime2x": ("realesr-animevideov3-x2", 2, []),
    "anime4x": ("realesr-animevideov3-x4", 4, []),
    "ultrasharp-tta": ("ultrasharp-4x", 4, ["-x"]),     # last: about 8x slower than the rest
}
APPROVED = "approved"
MODEL_FILES = sorted({f"{model}.{ext}" for model, _, _ in OPTIONS.values() for ext in ("param", "bin")})
CHARACTER = re.compile(r".*p\d\d")


def default_choice(group: str, stem: str) -> str:
    """For an image the review never saw -- another install's extra art. Character portraits keep
    the approved pipeline; everything else gets the option picked most often in review."""
    return APPROVED if group == "portrait" and CHARACTER.fullmatch(stem) else "ultrasharp-tta"


# --- the content check -------------------------------------------------------------------------
#
# The upscaler's output is only checked for SIZE on its way into the pack, and a tester's GPU once
# wrote renders of the right size whose pixels were garbage (diagonal bands on sprite__iceb,
# speckled buildings: 2026-09-26). The overlay then drew them, because it matches the ORIGINAL on
# screen, not the upscale. So every upscale is also compared with what it was made from:
#
#   error    each pixel of the source against the mean of its 2x2 block in the upscale (a box
#            downscale: the same for every option and every scale, since every upscale is exactly
#            2x), mean |difference| per channel;
#   texture  the source's own mean |difference| per channel to its right and lower neighbours;
#   score    error / (texture + TEXTURE_FLOOR).
#
# Only pixels that are opaque with all four neighbours opaque count: at an edge the upscale bleeds
# the transparent background in, which is right and says nothing. Dividing by the texture is what
# lets a dithered spell effect (which every upscaler smooths) through while a shifted or sheared
# upscale of a plain one is caught; the floor keeps a flat sprite from dividing by nothing.
#
# Measured 2026-09-26 with this function over the 28,399 sprite renders of a full --sprites run on
# the Mac (ultrasharp-tta 23,708, anime2x 3,610, ultrasharp 1,067, anime4x 14) and the 1,282
# pictures of the same run (the approved portraits among them): clean sprites p50 0.20, p99 0.37,
# p99.9 0.52, max 0.74 (a dithered spell effect, esp06br); clean pictures max 0.56; the tower's and
# the Mac's sprite__iceb 0.38. Rows at the wrong stride (1-4 px) or sheared, the tester's diagonal
# bands, score a median 1.7, and 98-99% of them clear DAMAGE_THRESHOLD (the misses are near-flat
# frames, where a shift changes little; a milder shear, heavy noise and a red/blue swap are caught
# about half the time). One threshold serves every option: no clean one comes near it. Below
# DAMAGE_MIN_PIXELS judgeable pixels (160 of the 28,399 frames) the frame passes unjudged.
DAMAGE_THRESHOLD = 0.9
DAMAGE_MIN_PIXELS = 64
TEXTURE_FLOOR = 8.0


def damage_score(width: int, height: int, source: bytes, upscale: bytes, source_bpp: int = 4,
                 upscale_bpp: int = 4) -> "float | None":
    """How unlike its source an upscale is (see above), or None when too little can be judged.
    `source` is width x height pixels, `upscale` exactly twice that each way; each is RGB
    (`bpp` 3, every pixel opaque) or straight RGBA (`bpp` 4, opaque where alpha is 255 -- in the
    SOURCE: the upscale's own alpha is not consulted)."""
    sb, hb = source_bpp, upscale_bpp
    if len(source) != width * height * sb or len(upscale) != width * height * 4 * hb:
        raise ValueError(f"damage_score: {len(source)} / {len(upscale)} bytes do not fit {width}x{height}")
    opaque = source[3::4] if sb == 4 else b"\xff" * (width * height)
    row, hrow = width * sb, width * 2 * hb
    error = texture = n = 0
    # A large picture is judged on every row_step-th row: a screen is 307,200 pixels, and a sample
    # of the rows sees a band or a shear as surely as all of them do. Every sprite is judged whole
    # (the overlay takes none over 65,536 pixels).
    row_step = max(1, width * height // 65536)
    for y in range(1, height - 1, row_step):
        top = upscale[2 * y * hrow:(2 * y + 1) * hrow]
        bottom = upscale[(2 * y + 1) * hrow:(2 * y + 2) * hrow]
        base = y * width
        for x in range(1, width - 1):
            p = base + x
            if not (opaque[p] == opaque[p - 1] == opaque[p + 1] == opaque[p - width]
                    == opaque[p + width] == 255):
                continue
            i = p * sb
            r, d = i + sb, i + row
            j = 2 * x * hb
            k = j + hb
            s0, s1, s2 = source[i], source[i + 1], source[i + 2]
            error += (abs(4 * s0 - top[j] - top[k] - bottom[j] - bottom[k])
                      + abs(4 * s1 - top[j + 1] - top[k + 1] - bottom[j + 1] - bottom[k + 1])
                      + abs(4 * s2 - top[j + 2] - top[k + 2] - bottom[j + 2] - bottom[k + 2]))
            texture += (abs(s0 - source[r]) + abs(s1 - source[r + 1]) + abs(s2 - source[r + 2])
                        + abs(s0 - source[d]) + abs(s1 - source[d + 1]) + abs(s2 - source[d + 2]))
            n += 1
    if n < DAMAGE_MIN_PIXELS:
        return None
    return (error / (12 * n)) / (texture / (6 * n) + TEXTURE_FLOOR)


def looks_damaged(score: "float | None") -> bool:
    return score is not None and score > DAMAGE_THRESHOLD


def png_size(path: pathlib.Path) -> tuple[int, int]:
    data = path.read_bytes()[16:24]
    return int.from_bytes(data[:4], "big"), int.from_bytes(data[4:], "big")


def render(option: str, inputs: dict[str, pathlib.Path], dest: pathlib.Path, esrgan: pathlib.Path,
           models: pathlib.Path) -> int:
    """Upscale every input PNG to exactly twice its size with one full-colour option, writing
    dest/<key>.png. The model runs once over a folder, not once per image. Returns how many were
    rendered; inputs whose output already exists are skipped, so a run can resume."""
    model, scale, extra = OPTIONS[option]
    dest.mkdir(parents=True, exist_ok=True)
    todo = {k: p for k, p in inputs.items() if not (dest / f"{k}.png").exists()}
    if not todo:
        return 0
    with tempfile.TemporaryDirectory() as tmp:
        stage, raw = pathlib.Path(tmp) / "in", pathlib.Path(tmp) / "out"
        stage.mkdir(); raw.mkdir()
        for k, p in todo.items():
            shutil.copy(p, stage / f"{k}.png")
        result = subprocess.run([str(esrgan), "-i", str(stage), "-o", str(raw), "-n", model,
                                 "-m", str(models), "-s", str(scale), "-f", "png", *extra],
                                capture_output=True, text=True)
        if result.returncode != 0:
            raise SystemExit(f"{option}: the upscaler failed\n{result.stderr.strip()[-400:]}")
        for k, p in todo.items():
            got = raw / f"{k}.png"
            if not got.exists():
                raise SystemExit(f"{option}: the upscaler wrote nothing for {k}")
            w, h = png_size(p)
            # An output that exists counts as done, so it is written under a name nothing reads
            # (not *.png: the review page lists those) and renamed into place only once whole.
            final, part = dest / f"{k}.png", dest / f"{k}.png.part"
            if scale == 2 and png_size(got) == (w * 2, h * 2):
                shutil.move(str(got), part)
            else:
                # Exactly 2x the original, whatever rounding the model applied.
                subprocess.run(["magick", str(got), "-filter", "MagicKernelSharp2021",
                                "-resize", f"{w * 2}x{h * 2}!", f"PNG:{part}"], check=True)
            os.replace(part, final)
    return len(todo)
