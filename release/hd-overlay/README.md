# Lords of Magic HD Art

Sharp, redrawn art for Lords of Magic Special Edition: every character portrait, every item and
artifact picture, and every building picture, everywhere the game shows one. The game itself is not modified. A
replacement `ddraw.dll` (a fork of cnc-ddraw, which the game already uses) spots each picture as it
is drawn and paints a 2x-resolution version over it.

**No game art is included.** The setup reads the pictures from your own copy of the game and
upscales them on your machine, each with the upscaler that was picked for it by hand when the mod
was made (the choices ship as `upscale-choices.json`).

The art is sharper, not bigger: each picture fills the same space on screen as before.

## What you need

- Lords of Magic Special Edition (Steam or GOG). Vanilla and the GS5R3 patch both work.
- **Python 3.9 or newer.** Windows: install from python.org and tick *Add python.exe to PATH*.
- **ImageMagick 7.** Windows: `winget install ImageMagick.ImageMagick`, then open a new terminal.
  macOS: `brew install imagemagick`.
- A graphics card with Vulkan (any recent NVIDIA, AMD or Intel GPU; Apple Silicon works).
- About 1 GB of disk while it runs, and 20-60 minutes, almost all of it upscaling.

## Install

1. **Close the game.** Windows will not let a running game's `ddraw.dll` be replaced.
   Then unzip this folder anywhere.
2. Open a terminal in it and run:

   ```
   python lomhd_setup.py
   ```

   It looks for the Steam install. If yours is elsewhere, point it at the folder holding
   `lomse.exe`:

   ```
   python lomhd_setup.py --game "D:\Games\Lords of Magic Special Edition\English"
   ```

3. Start the game as usual.

The setup downloads two things, each checked against a fixed SHA-256 before use:
Real-ESRGAN ncnn Vulkan v0.2.5.0 from its GitHub release, and the 4x-UltraSharp model files.

## Choose your own upscaler (optional)

Every picture ships with an upscaler already picked for it -- the maintainer compared four
options side by side for each one. To make your own picks instead:

```
python lomhd_setup.py --review
```

This renders **every** option for every picture from your own game (a few hours; stop it any
time, it resumes), then opens a review page on your own computer at http://127.0.0.1:8765 with
the shipped picks already selected. Nothing is uploaded and nothing is installed.

- Each picture shows the original (**0**) and each option (**1**-**5**; the palette pipeline
  appears only on character portraits). Click a tile or press its number to pick it.
- **Zoom -> Detail** shows the same close-up of every option; move the mouse to pan.
- Picks save as you make them, to `my-upscale-choices.json` next to `lomhd_setup.py`.

Close the page and press Ctrl+C, then run `python lomhd_setup.py` as usual: it says
"Using your own picks" and installs with them. Delete `my-upscale-choices.json` to go back to the
shipped picks.

## Uninstall

```
python lomhd_setup.py --uninstall
```

If you installed with `--game`, uninstall with the same `--game "..."` too.

This puts your original `ddraw.dll` back (it was saved as `ddraw.dll.lomhd-backup`) and removes the
image pack. To turn the HD art off without uninstalling, delete `lomhd_portraits.pack` from the
game folder (the name dates from when it held only portraits).

## If something looks wrong

- **A picture stays blurry:** it is covered or clipped at that moment (a tooltip, the screen edge),
  or it is one the game builds on the fly. The original is shown; nothing breaks.
- **Nothing is sharp at all:** check `lomhd.log` in the game folder. It says whether the pack
  loaded and, if the overlay switched itself off, why.
- The overlay needs cnc-ddraw's **OpenGL** renderer. With `renderer=auto` (the default) the overlay
  selects it; if you have set `direct3d9` or `gdi` in `ddraw.ini` yourself, that is respected and
  the game runs normally with the original art.
- **On Windows the Steam version ships without cnc-ddraw**, so installing this also brings in
  cnc-ddraw itself (windowing, scaling, and a `ddraw.ini` it creates on first run). Uninstalling
  removes cnc-ddraw, so the game uses Windows' own DirectDraw again, and renames that `ddraw.ini`
  to `ddraw.ini.lomhd-saved` rather than deleting settings you may have changed. The game ignores
  it; delete it if you like.

See `NOTICES.md` for licences.
