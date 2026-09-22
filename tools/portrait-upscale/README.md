# Portrait upscaling

Turns a shipped 70x67 `pic.mpq` portrait into a 140x134 one, keeping the member's own palette.

```
python3 tools/portrait-upscale/upscale.py SRC_DIR OUT_DIR \
    --names NAMES.txt --esrgan PATH/realesrgan-ncnn-vulkan --models PATH/models \
    [--model ultrasharp-4x] [--size 140x134] [--report-fidelity]
```

`SRC_DIR` holds members extracted from `pic.mpq`; `NAMES.txt` lists member names in the archive's
own spelling (`lom-mpq list`). Output mirrors those paths under `OUT_DIR`, ready to copy into a mod
tree's `archives/pic.mpq/`.

Needs `magick` (ImageMagick 7) and a Real-ESRGAN ncnn binary with models. Neither is vendored: the
binary is the upstream `Real-ESRGAN-ncnn-vulkan` release, the models came from
`upscayl/upscayl`'s `resources/models`. The macOS release ships **no models** — fetch them
separately.

## Why this is safe to run

The engine side is settled — see
[resolution and upscaling](../../docs/resolution-and-upscaling.md#-closed-the-70x67-portrait-was-never-an-engine-limit).
An oversize portrait loads, a `doodad` crops or paints at whatever size its rect asks for, and a
click follows the drawn position. Three attended runs on 2026-09-21.

Raising the art alone is **not enough**: the 17 portrait `doodad` rects have to be raised with it
(`gs\dlg\NEWBUILD5a.gs` and `gs\dlg\INFOPAN5.gs`), or the panel shows the top-left quadrant of a
larger picture. See `mods/portrait-2x-art`.

## Two deliberate refusals

**The palette is carried across, never re-optimised.** 193 distinct palettes across the 749 shipped
portraits, and **no entry is common to all of them**, so the corpus cannot say whether the engine
honours a member's `CMAP` or remaps it onto a screen palette. Re-quantising into colours the member
already uses is correct under either answer. A new palette is untested.

**The model invents, so measure it.** `--report-fidelity` downscales each result back to the source
size and compares against the original. Over three portraits on 2026-09-21: faithful resampling
**4.8/255** mean, `ultrasharp-4x` **9.7/255** with local deviations to **56**. `ultrasharp-4x` was
chosen over a lower-deviation blend by the project owner after a side-by-side, so the extra
invention is an accepted cost — report the number with any art this produces rather than re-opening
the choice.

`digital-art-4x` was tried and rejected: it posterises faces and destroys the parchment texture.

## Reviewing the output

`tools/portrait-review/` (served by `tools/review-server.py`) is a local side-by-side page with keyboard verdicts that persist to
disk. 353 of the 387 nameable portraits were reviewed this way on 2026-09-21 and all 353 were kept; the
other 34 are spelled `portrait\` lowercase and a case-insensitive filesystem cannot hold both
spellings in one mod tree.

## This does NOT transfer to sprites

`imp.mpq` is a different problem and the reasons are measured, not guessed — 1-bit transparency
against the model's 240 alpha levels, and palette index 1 carrying shadow semantics by index. See
[the sprite section](../../docs/resolution-and-upscaling.md#%EF%B8%8F-the-same-upscaler-on-sprites-tried-and-it-is-a-different-problem).
