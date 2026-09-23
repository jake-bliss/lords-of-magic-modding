# Lords of Magic HD Portraits

Sharp, redrawn portraits for Lords of Magic Special Edition, everywhere the game shows one: the
lord in the corner, the info panel, recruitment, buildings. The game itself is not modified. A
replacement `ddraw.dll` (a fork of cnc-ddraw, which the game already uses) spots each portrait as it
is drawn and paints a 2x-resolution version over it.

**No game art is included.** The setup reads the portraits from your own copy of the game and
upscales them on your machine.

The portraits are sharper, not bigger: each one fills the same space on screen as before.

## What you need

- Lords of Magic Special Edition (Steam or GOG). Vanilla and the GS5R3 patch both work.
- **Python 3.9 or newer.** Windows: install from python.org and tick *Add python.exe to PATH*.
- **ImageMagick 7.** Windows: `winget install ImageMagick.ImageMagick`, then open a new terminal.
  macOS: `brew install imagemagick`.
- A graphics card with Vulkan (any recent NVIDIA, AMD or Intel GPU; Apple Silicon works).
- About 200 MB of disk and 10-20 minutes, most of it upscaling.

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

## Uninstall

```
python lomhd_setup.py --uninstall
```

If you installed with `--game`, uninstall with the same `--game "..."` too.

This puts your original `ddraw.dll` back (it was saved as `ddraw.dll.lomhd-backup`) and removes the
portrait pack. To turn the portraits off without uninstalling, delete `lomhd_portraits.pack` from
the game folder.

## If something looks wrong

- **A portrait stays blurry:** it is covered or clipped at that moment (a tooltip, the screen edge),
  or it is one the game builds on the fly. The original is shown; nothing breaks.
- **No portraits are sharp at all:** check `lomhd.log` in the game folder. It says whether the pack
  loaded and, if the overlay switched itself off, why.
- The overlay needs cnc-ddraw's **OpenGL** renderer (the default). With the Direct3D 9 or GDI
  renderer the game runs normally with the original portraits.

See `NOTICES.md` for licences.
