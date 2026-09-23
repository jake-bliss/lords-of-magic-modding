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
            if scale == 2 and png_size(got) == (w * 2, h * 2):
                shutil.move(str(got), dest / f"{k}.png")
            else:
                # Exactly 2x the original, whatever rounding the model applied.
                subprocess.run(["magick", str(got), "-filter", "MagicKernelSharp2021",
                                "-resize", f"{w * 2}x{h * 2}!", str(dest / f"{k}.png")], check=True)
    return len(todo)
