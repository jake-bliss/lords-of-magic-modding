# Lords of Magic HD Art

Sharp, redrawn art for Lords of Magic Special Edition: every character portrait, every item and
artifact picture, and every building picture, everywhere the game shows one; the map's sprites
(buildings, trees and other scenery), with the animated ones -- units, spell effects -- optional;
and optional HD terrain. Without `--terrain` the game itself is not modified. A
replacement `ddraw.dll` (a fork of cnc-ddraw, which the game already uses) spots each picture as it
is drawn and paints a 2x-resolution version over it.

**No game art is included.** The setup reads the pictures and sprites from your own copy of the game and
upscales them on your machine, each with the upscaler that was picked for it by hand when the mod
was made (the choices ship as `upscale-choices.json`).

The art is sharper, not bigger: each picture fills the same space on screen as before.

> **Beta (0.5.1).** New in this release: a fix for the game hanging when a Death Shade or Frozen
> Shade dies (see Install), and a check of every upscale's contents -- an upscale of the right size
> whose pixels came out as bands or speckle is made again, or left out so the original shows (see
> HD sprites). Rerunning setup over a 0.5.0 install finds and replaces any such upscale already in
> it. 0.5.0 added HD sprites. The HD art overlay and HD terrain (`--terrain`) have both been played
> on Windows 11 (NVIDIA) and macOS (Wine); the sprites are new, so please report anything odd --
> see the end of this file. `--uninstall` puts every file back.

## What you need

- Lords of Magic Special Edition (Steam or GOG). Vanilla and the GS5R3 patch both work.
- **Python 3.9 or newer.** Windows: install from python.org and tick *Add python.exe to PATH*.
- **ImageMagick 7.** Windows: `winget install ImageMagick.ImageMagick` (setup finds it straight away,
  even in the terminal you installed it from).
  macOS: `brew install imagemagick`.
- A graphics card with Vulkan (any recent NVIDIA, AMD or Intel GPU; Apple Silicon works).
- About 1 GB of disk while it runs, and 20-60 minutes, almost all of it upscaling. With
  `--sprites`, several hours more and about 2.5 GB more disk (see below).

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

It also fixes a bug in the game itself: `lomse.exe` could hang when a Death Shade or Frozen Shade
died in combat (found and fixed by Skarn). That is one byte of `lomse.exe`, changed only on the
Steam version (Special Edition with the GS5R3 patch), after saving your original as
`lomse.exe.lomhd-backup`; any other `lomse.exe` is left as it is. `--uninstall` puts the original back.

## HD sprites

The map is drawn from sprites -- buildings, trees, rocks, units, spell effects -- kept in the
game's `imp.mpq`. Setup reads them from your own copy, like the pictures, and adds them to the same
image pack:

- **Sprites that do not move** (buildings, scenery: one picture each) are built on every run. They
  add a few minutes. Only sprites that were given an upscaler by hand when the mod was made are
  built; the rest -- mostly tiny aura and effect sprites whose preview was too small to judge -- are
  left as the game draws them.
- **Animated sprites** (units, spell effects, anything with more than one frame) are optional,
  because every frame is upscaled on its own -- tens of thousands of them:

  ```
  python lomhd_setup.py --sprites
  ```

  This takes **several hours on a typical GPU**. Stop it any time (Ctrl+C or closing the window);
  run the same command again and it carries on where it stopped: every frame already upscaled is
  kept in `lomhd_work\sprites`, which grows to about 1 GB. The image pack grows by about 0.7 GB
  too, and setup keeps a copy of it in `lomhd_work` as well as the one in the game folder.

  Setup remembers `--sprites`: later runs (a plain `python lomhd_setup.py`, after `--review` say)
  keep the animated sprites and say "Animated sprites: on, remembered from your last install".
  They reuse every frame already upscaled; only a sprite whose pick you changed (or that a mod
  changed) is upscaled again. To go back to sprites that do not move only, run
  `python lomhd_setup.py --no-sprites`.

  You can delete `lomhd_work\sprites` to get the space back once the install is done, but then any
  later run -- including a plain one, since `--sprites` is remembered -- upscales every animated
  sprite again, for hours. Running later updates with `--no-sprites` avoids that, but also takes
  the animated sprites out of the game.

Each sprite is upscaled with the method picked for it by hand (one pick covers all of a sprite's
frames, so an animation does not flicker between styles). Frames that repeat are packed once, and
units are also found drawn facing the other way. A sprite with no hand-picked upscaler, or too small
or too plain for the overlay to spot reliably, is left out and stays as the game draws it; setup
lists what it left out, and why, in `lomhd_work\sprites-left-out.txt`. If a mod changes some sprites
in `imp.mpq`, the next run upscales only those again.

Every upscale -- sprite frame or picture -- is also checked against what it was made from, every
time setup builds the pack: shrunk back to the original's size, it must still look like the
original. An upscaler or graphics driver has been seen to write an upscale of the right size whose
pixels were garbage (diagonal bands, speckle). One that fails is made again once; if it fails again
it is left out, the original shows in its place, and setup says so at the end ("upscale looked
damaged"). The check reads the upscales already kept in `lomhd_work`, so rerunning setup finds a
damaged one from an earlier run too.

## HD terrain (optional, beta)

The map's terrain is drawn by the game itself, so the overlay alone cannot sharpen it. With
`--terrain`, setup also makes it 2x:

```
python lomhd_setup.py --terrain
```

This one **does change the game**, in two places, always together:

- **`lomse.exe` is patched** -- 65 same-size edits so the terrain is drawn at 2x (and the Shade fix). It only works on
  Lords of Magic Special Edition with the GS5R3 patch (the Steam version); any other `lomse.exe` is
  refused and left untouched. Your original is saved first as `lomse.exe.lomhd-backup`.
- **A new folder, `lomhd_terrain`**, beside `lomse.exe`, holds the 2x terrain tiles. Like the
  pictures, they are made on your machine from your own `pic.mpq` (which is not changed). This adds
  to the run time: every terrain tile is upscaled on its own.

The exe is patched last, after the art is in place, so a run that stops part-way leaves your game
working. The overlay's `ddraw.dll` only reads `lomhd_terrain` when the patched `lomse.exe` is the one
running, so the original exe with the folder left beside it plays exactly as before.

**Recommended window: 1280x960.** The terrain is drawn at twice the game's 640x480, so it shows its
detail at 1280x960 or larger. Setup does not change your settings; to set it, edit `ddraw.ini` in
the game folder (cnc-ddraw creates it on first run) and set `width=1280` and `height=960` in the
`[ddraw]` section.

**Undo:** `python lomhd_setup.py --uninstall` puts your original `lomse.exe` back from the backup
(checking it first) and removes `lomhd_terrain`, along with the rest of the mod. If something else
has changed `lomse.exe` since, uninstall leaves it alone and says so rather than overwrite it. If
`lomhd_terrain` holds files setup did not write (you edited or added art by hand), setup leaves the
folder where it is and says so; `--force-terrain-folder` lets it replace or remove the folder anyway.

Steam's **Verify integrity of game files** puts the original `lomse.exe` back and so undoes the exe
half. That is harmless: the game runs as normal with the ordinary terrain. Run
`python lomhd_setup.py --terrain` again to re-apply it, or `--uninstall` to tidy up the backup and
the folder.

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

The terrain atlases and the sprites have picks too (the `terrain__` and `sprite__` entries in
`upscale-choices.json`); the review page does not show them, but setup uses any you add to
`my-upscale-choices.json`, one atlas or sprite at a time, and the shipped pick for the rest.

Close the page and press Ctrl+C, then run `python lomhd_setup.py` as usual: it says
"Using your own picks" and installs with them (and keeps the animated sprites if you installed
with `--sprites`). Delete `my-upscale-choices.json` to go back to the
shipped picks.

## Uninstall

```
python lomhd_setup.py --uninstall
```

If you installed with `--game`, uninstall with the same `--game "..."` too.

This puts your original `ddraw.dll` back (it was saved as `ddraw.dll.lomhd-backup`) and removes the
image pack (pictures and sprites). It also puts your original `lomse.exe` back, and if you installed HD terrain removes
`lomhd_terrain` (unless you changed files in it -- see above). To turn the HD art off without uninstalling, delete `lomhd_portraits.pack` from the
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
- **A sprite or picture with bands or speckle inside its outline** is an upscale that came out
  damaged but passed the content check. Delete it from `lomhd_work\sprites\render` (the file names
  include the sprite's name) and run setup again, and please report it with the sprite's name.
- **Scrambled or striped terrain** means the patched `lomse.exe` is running without its
  `lomhd_terrain` folder: run `python lomhd_setup.py --terrain` again, or `--uninstall`.
- **Reporting a problem:** open an issue at
  <https://github.com/jake-bliss/lords-of-magic-modding/issues> with your `lomhd.log`, your
  Windows or macOS version, and whether you used `--sprites` or `--terrain`.

See `NOTICES.md` for licences.
