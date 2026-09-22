# HD portrait overlay

Sharp portraits in the unmodified game. The engine keeps drawing 640x480 in 16 bpp exactly as it
always has; a fork of cnc-ddraw finds each portrait in the finished frame and draws its 140x134
upscale over it at window resolution.

**Status 2026-09-22: works in the live game.** Observed in gameplay on the macOS/Wine development
profile: detection of the lord's portrait in the native bottom-strip slot and the script-drawn info
panel, and of cavalry, infantry, missile and wizard portraits in the recruitment dialog, each logged
at its screen position; the upscale drawn in place; no crash; cursor still drawn on top. A side by
side against the vanilla wrapper at the same window size shows smooth parchment and clean line work
where vanilla shows dither grain. Not yet tested on Windows, not yet reviewed, not yet packaged.

Code: `~/personal-projects/cnc-ddraw-lom`, branch `lom-hd-overlay` off upstream cnc-ddraw 7.1.0.0
(`541b5de`, the version the game ships). Pack tool: `tools/hd_portrait_pack.py` here.

## Why this, and not the three things tried first

Each of these was built and run on 2026-09-22, and each failed for a reason worth keeping:

| approach | what broke |
| --- | --- |
| Whole UI at 2x (`mods/ui-2x-infopan`, 1280x960 binary patch) | Every unconverted panel draws half-size. 112 members, 5,016 geometry sites before anything is playable. On a 945-point-tall display a 2x frame shows at vanilla apparent size anyway. |
| Bigger portrait slot, vanilla frame, portraits **replaced** in `pic.mpq` (`mods/ui-bigportrait`) | The re-flowed panel was right; every OTHER consumer cut `0 0 70 67` from a 140x134 file and got a quarter of it (recruitment), covered its own text (alchemist), or ran off screen (bottom strip). |
| The same, with upscales **added** alongside | `pic.mpq`'s hash table is 2048 entries, 1406 used; 749 do not fit. `SFileSetMaxFileCount` fails 10007 (unknown member names), so the table cannot grow. |

Two facts made an in-engine route a dead end regardless: the bottom-strip portrait is drawn by
**native code** (`setleaderdoodadcallbackproc` only returns a filename), and no GameScript operator
scales an image (`copydoodad`/`tcopydoodad` take x, y and no size). So two sizes must coexist and
the second set has nowhere to live.

## How it works

**Detection is by pixels, not hooks.** Observed in a local binary: the engine composites into a
back buffer with its own software copier (`rep movsd` at `0x495db0`, `0x496340`) and presents only
dirty rectangles via `BltFast` (`0x476b5e`). cnc-ddraw emulates the primary surface, so it holds the
whole finished frame. Observed in gameplay: portraits land in that frame as **exact RGB565 copies**
of their LBMs -- 100% of pixels for both the native slot (45,408) and the info panel (285,147).
Nothing in `lomse.exe` is hooked, so every consumer is covered, including ones no script names.

**Matching** (`src/lomhd_match.c`, pure -- no cnc-ddraw state, GL or files): a rolling hash over
each 70-pixel window of every row, looked up in a table of three probe rows per portrait (so a
cursor on one row does not hide it) under both candidate RGB565 conversions (truncating and
rounding; the frames seen so far cannot tell them apart, so both are accepted), then a full-block
check at 85%. Runs only when the frame changed. **2.5 ms mean, 3.0 ms worst** on captured frames.

**Drawing** (`src/lomhd.c`): after cnc-ddraw draws the scaled frame and before `SwapBuffers`. The
context is GL 3.2 core, so the quad has its own shader and buffers. Pixels the frame no longer shows
as the portrait (cursor, tooltip) are **discarded**, not blended; every GL binding touched is saved
and restored, because cnc-ddraw sets some of its state once at init.

**Threading rule: the render thread never touches a file.** It holds `g_ddraw.cs` whenever it calls
in. An early build logged and wrote screenshots there; under Wine that wedged the render thread and
a second thread writing the same log after about a dozen dumps, while the game carried on. One
worker thread owns every file operation.

## The pack

`tools/hd_portrait_pack.py OUT --originals INSTALLED --sources MADE_FROM --upscaled DIR...` writes
`lomhd_portraits.pack` beside `lomse.exe`: per portrait, the original (templates) and the upscale
(drawn), each as a palette plus indices.

🔴 **Pairing is by content.** An upscale is packed only when the installed original is pixel- and
palette-identical to the original it was made from. The vanilla and GS5R3 installs share 445
portrait names and 5 of them differ; templates built from the wrong install matched the lord (the
same in both) and silently missed the Life banner. Vanilla: 440 portraits. GS5R3: 748 (all but
`aipotm.lbm`, which was never upscaled).

## Operating it

- **Debug mode:** a file named `lomhd_debug` beside `lomse.exe` at start. Adds the detection record
  (`seen: NAME at (x,y)`), a five-second watchdog, and a frame trigger: create `lomhd_dump` and the
  next frame is written as `lomhd_frame_*.raw` (`LOMHDRAW`, u32 w/h/bpp, packed RGB565). Without it
  the log only records the pack loading, the overlay turning itself off, or an error.
- **Off switch:** delete `lomhd_portraits.pack`. A missing or malformed pack, or a missing GL entry
  point, turns the overlay off and leaves the game exactly as it was.
- **Matcher test:** `tests/lomhd_match_test.exe PACK FRAME.raw...` runs the shipped matcher over
  captured frames under the app's own Wine (needs `WINEESYNC=1 WINEMSYNC=1` and the app's
  `Contents/Frameworks` on `DYLD_FALLBACK_LIBRARY_PATH`, or it exits 136 with no output).
- **Original DLL:** `artifacts/experiment-backups/ddraw-20260922/ddraw.dll.orig`
  (`85e0f7d530dfda13`, identical to the copy in the GS5R3 profile).

## Traps this cost time

- **`glGetIntegerv` is NULL** in cnc-ddraw on Wine: it is fetched through `wglGetProcAddress`, which
  does not return GL 1.1 functions. The first live run crashed on the first portrait (EIP 0,
  `GL_CURRENT_PROGRAM` on the stack). Every GL entry point is now checked before first use.
- **A capture loop whose `pgrep -f lomse.exe` matched its own command line** never saw the game close.
- **A full-screen `screencapture` grabs the whole desktop.** Capture the game window only, by its
  CGWindowID (`screencapture -l`).
- **The dev profile is cloned from vanilla**, so a rollback leaves vanilla archives in it.
  `mods/gs5r3-base` (on `claude/ui-2x-barracks`) puts GS5R3's back, with two inert changes because
  the pipeline refuses a declared replacement whose content is unchanged.
- **A refused build leaves its directory behind** with only manifests, and the next run reports
  "build already exists". Check the build directory holds its archives before installing.

## Release plan (option A: ship a recipe, never the art)

The player's machine does the derivation: extract portraits from their own `pic.mpq`, upscale,
build the pack, install the DLL. No game art is distributed.

1. Cross-model review (Claude + Codex) of the wrapper -- in progress 2026-09-22.
2. Live test on GS5R3 with all 748 portraits -- the dev profile is staged for it.
3. A wider playtest across every portrait consumer (barracks, alchemist, character screens, spy
   panel, combat) and a longer session.
4. Windows: untested. The code has no Wine dependency, but nothing has run there.
5. Packaging: a player-side extractor (the pipeline's tools are dev builds), the upscaler
   (ImageMagick + Real-ESRGAN ncnn + the 4x-UltraSharp model -- **check that model's licence**,
   believed non-commercial share-alike), the cnc-ddraw MIT notice, a README, and an uninstaller
   that restores the original `ddraw.dll`.

## Known gaps

- The on-screen **size does not change**; this buys sharpness at the slot's size. Bigger portraits
  need layout changes as well (the re-flow in `mods/ui-bigportrait` works for the info panel).
- The vanilla **faith banner** is not in the pack; GS5R3's is, but has not been seen on screen yet.
- A portrait **clipped at the screen edge, or with all three probe rows covered**, is not detected
  and stays vanilla -- a safe failure.
- Only the **OpenGL** renderer draws the overlay.
