# HD art overlay

Sharp portraits, item art and building pictures in the unmodified game. The engine keeps drawing 640x480 in 16 bpp exactly as it
always has; a fork of cnc-ddraw finds each known picture in the finished frame and draws its 2x
upscale over it at window resolution.

**Status 2026-09-22: works in the live game.** Observed in gameplay on the macOS/Wine development
profile: detection of the lord's portrait in the native bottom-strip slot and the script-drawn info
panel, and of cavalry, infantry, missile and wizard portraits in the recruitment dialog, each logged
at its screen position; the upscale drawn in place; no crash; cursor still drawn on top. A side by
side against the vanilla wrapper at the same window size shows smooth parchment and clean line work
where vanilla shows dither grain.

**Update 2026-09-22 (late): 917 pictures** -- 749 from `portrait\` (character portraits, items,
artifacts, spells) and 168 building pictures from `lbm\building\`, 34 widths from 98 to 228.
Jake picked the upscaler per picture on a local review page (below). Seen in play on the dev
profile: Life and Order barracks, mage tower and guild pictures detected and drawn. Windows passed
for the 749-portrait release (RTX 3070); the 917 release is not yet re-tested there.

**Update 2026-09-23: full screens, 1,281 pictures.** Keeps, full screens (`lbm\`), panels, skies
and the library join -- 371 more, most 640x480, upscaled to 1280x960. A capture run first showed
the engine copies screens into the frame as exact RGB565 too (loading 99.7%, start 99.5%, library
95.4%, a keep 94.6%, the unit roster 91.1%; quest2 sits at (70,20)). Seen in play on the dev
profile and judged "substantially better" side by side with the untouched GS5R3 profile: start,
new game, loading, the interface bar under the map, the Life library and a book page, the Life
keep, the unit roster, report and quest dialogs, with portraits and buildings drawn over them.
This needed pack format 3 and lazy loading (below): format 2 would have held ~1.8 GB of upscales
in a 32-bit process. Players can now pick their own upscaler per picture (`--review`, below).
Not yet re-tested on Windows.

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
each 32-pixel window of every row, looked up in a table of three probe rows per picture (so a
cursor on one row does not hide it). Each probe is the picture's **busiest** 32-pixel slice of that
row: a flat slice (sky, parchment) hashes to the same value in half the frame, and the first
any-width build took about 10 s per frame for it. Both candidate RGB565 conversions (truncating
and rounding) are accepted. A candidate then passes a 1-in-16 sample at 60% (cheap rejection of
false hits only) and a full count at 85%, through a palette-to-RGB565 table (1 byte per pixel
held, not two 2-byte templates).

**Large pictures** (over 65,536 pixels -- screens) are scored on their 1-in-16 sample alone, at
**30%**: a library page with text on it showed 76% of its pixels, and the interface bar under the
live map 30-38%. Nothing else in a frame matches 30% of a ~20,000-point sample of a 640x480 picture
exactly, and the mask (below) draws the upscale only where the original's exact pixel is still
showing, so a partly covered screen still draws correctly. Their full indices are loaded only
when they are found.

**One picture per spot, the best.** An upgraded building shares most of its pixels with the level
below (`llwizt3a` matched 78% where `llwizt1a` matched 100%). Candidates of a **similar size**
(neither more than twice the other's area) sharing at least half of the smaller one are rivals; a
new one must beat **every** rival, and replaces them all. Pictures that merely touch, and a
portrait on a screen, are both drawn. `tests/lomhd_find_test.c` asserts this on synthetic frames.
Runs only when the frame changed: **1.0 ms mean, 1.3 ms worst** (native build) over 64 captured
frames with all 1,281 pictures loaded.

**Loading** (`src/lomhd.c`): the worker keeps the pack open and reads only its index and matching
data at start (1.7 s, 18 MB resident for 1,281 pictures). The first time a picture is found the
render thread asks for it; the worker inflates its upscale (and a screen's full indices) and hands
it over; the next scan draws it -- the original shows for a frame or two. Each upscale is uploaded
to the GPU once and its CPU copy freed; a 160 MB texture budget evicts the least recently drawn
picture not on screen. A new GL context forgets every texture and forces a rescan; a picture that
finished loading after it left the screen is dropped after 60 scans. A picture whose stream fails
is switched off alone and logged.

**Drawing**: after cnc-ddraw draws the scaled frame and before `SwapBuffers`. The context is GL 3.2
core, so the quad has its own shader and buffers. Each placement uploads a **mask** at the
original's size (one byte a pixel: does the frame still show the original's exact pixel here?);
the shader samples it NEAREST and **discards** covered pixels -- cursor, tooltip, text on a page,
the live map in the interface bar. Every GL binding touched, both texture units and the unpack
state are saved and restored, because cnc-ddraw sets some of its state once at init.

**Threading rule: the render thread never touches a file.** It holds `g_ddraw.cs` whenever it calls
in. An early build logged and wrote screenshots there; under Wine that wedged the render thread and
a second thread writing the same log after about a dozen dumps, while the game carried on. One
worker thread owns every file operation.

## The pack

`tools/hd_portrait_pack.py OUT --originals INSTALLED --sources MADE_FROM --upscaled DIR...` writes
`lomhd_portraits.pack` beside `lomse.exe` (the name predates buildings). **Format 5** (`LOMHDPK5`,
2026-09-23, replacing format 4): an index first (name, sizes, flags, colour key, group, palette, two
stream lengths per record), then per record zlib of its indices and zlib of its upscale, back to
back to the end of the file -- offsets are sums of lengths, so none can point anywhere odd. Any
width from 32 and height from 4, upscale at most 1280 a side. Format 4 adds a per-record `flags`
byte (bit 0 MASKED) and a `key` byte between the sizes and the palette; every picture packed so far
is unmasked (`flags=0`, `key=0`, upscale stream `hw*hh*3` RGB), unchanged from format 3 apart from
those extra header bytes. A **masked** record (a sprite, see below) has pixels whose index
equals `key` (its transparent colour) or 1 (the shadow, keyed by index rather than colour) that are
not part of the image, and its upscale stream is `hw*hh*4` straight RGBA -- the transparency an
upscaler produced that the 1-bit game format never had room for. Format 5 adds, for masked records
only, flags bit 1 **MIRROR** (the game also draws the sprite flipped left to right, as it draws map
armies facing the other way) and a u16 **group** after the key: the frames of one animated sprite,
which must be consecutive. ~870 MB with screens, so the
writer streams it. Inflation in the overlay is **bounded**
(`lodepng_zlib_decompress_bounded`): a stream that would inflate past its picture's size fails
while inflating. An older-format pack is reported ("run lomhd_setup.py again"). The writer refuses
what the reader would, and refuses pixel-doubled "upscales": 395 of the pictures in the first pack
played were 2x2 repeats that changed nothing. Setup leaves out pictures with fewer than 16 colours:
a flat picture's probes match anywhere.

**Probe capacity.** The DLL sizes its probe table to the pack (at most half full) and refuses a pack
of more than 131,072 records (`MAX_IMAGES`) or 1,048,576 probes (`MAX_PROBES`). A picture costs 3
probe rows x 2 colour-rounding rules (6); a sprite up to 4 rows x 3 column bands, one rule (every
sprite match in the captures used truncation), doubled when MIRROR (24). The writer sums each
record's own reservation and refuses what the DLL would, before the game finds out at load time.

**Sprites** (`tools/hd-review/sprite_pack.py`, a dev tool, not shipped -- sprites are not part of
the player release yet) pack STATIC sprites -- an IMP member with exactly one frame in total, whose
one review render is its upscale -- and, with `--animated`, every frame of every animated sprite
(`anim_frames.py`): each frame prepared the way the reviewed still was and upscaled with that
sprite's pick, repeats packed once (a third of all frames), frames of `units\` members marked
MIRROR, one group per sprite. Rendering ~35,000 frames takes hours; it goes in resumable batches.
A sprite's low-res half (palette
and indices, what the matcher compares on screen) comes straight from the asset viewer's
`--export-imp-frame` -- no shadow-clearing, no background fill, because the game still draws the
shadow and the transparent key exactly as the archive stores them; only the review's own originals
(`sprite_originals.py`) do that cleanup, for upscaling, not for the pack. A masked record must also
satisfy what the DLL requires: width >= 8, height >= 4, `w * h <= 65,536`, and at least 3 rows each
holding a run of >= 8 consecutive pixels that are neither the colour key nor the shadow index
(`hd_portrait_pack.masked_is_eligible`); the builder skips and reports anything short of that, the
same as every other kind of skip. One command builds a pack with both sprites and the existing
pictures, for a single DLL test that covers both:

    python3 tools/hd-review/sprite_pack.py imp.mpq combined.pack --with-pack-inputs \
        --originals lomhd_work/originals/portrait \
        --upscaled lomhd_work/upscaled/ultrasharp-tta/portrait

**Interface icons** (`tools/hd-review/sheet_icons.py`, a dev tool): the map bar's arrows, zoom and
footprint buttons, the eye, the gem wheel and the status icons are drawn one rectangle at a time
from a few UI sheets (`intspr1`, `eoturn`, `staticon` and its GS5R3 variants, and six more). The
sheets were packed as pictures, but a picture is only found drawn whole or cut at an edge, so no
icon ever was. The tool cuts each sheet into its icons -- the scripts' own `NAME_page x y w h doodad`
rectangles first (any variable bound to an LBM is a page: `/unitinfo_staticon"LBM/STATICON.lbm"lbm
def`), then each separate shape on the sheet's key colour -- and packs each as a masked record keyed
on index 0, the pure-green chroma key, cropped at 2x from one upscale of the whole sheet with the
sheet's own pick (`screen__<sheet>`). 365 icons from the GS5R3 sheets; the shipped matcher finds 65
different ones in the 93 captured frames at 4.9 ms a search over the whole 30,046-record dev pack.

    python3 tools/hd-review/sheet_icons.py icons.pack --lbm <pic.mpq LBM folder> \
        --scripts <gs.mpq scripts> --esrgan realesrgan-ncnn-vulkan --models models

**Which upscaler, per picture** (`tools/hd_upscale.py`, shared by the review renderer and the
player's setup so both run the same code): `approved` (the original palette pipeline -- despeckle,
4x-UltraSharp, remap to the picture's own colours; kept for the 396 character portraits `...pNN`),
`ultrasharp`, `ultrasharp-tta`, `anime2x`, `anime4x` (full colour, no despeckle, no remap -- those
two steps lost detail on buildings). Picks live in `release/hd-overlay/upscale-choices.json` (names
only, no art); a picture the review never saw gets `approved` if it is a character portrait,
otherwise `ultrasharp-tta`. The review page: `tools/hd-review/render_variants.py` then
`tools/hd-review/serve.py` (127.0.0.1:8765, game art stays local). Jake picked all 2,845 on
2026-09-23 -- including sprites (one frame each, `sprite_originals.py`, shadow index cleared),
terrain sheets and icons, which the overlay cannot draw yet. **The 1,512 sprite picks were made on
red/green-swapped originals** (the viewer decoded IMP palettes wrongly until 2026-09-23; see the
[research log](research-log.md#2026-09-23--imp-palettes-are-bgr-after-all-the-capture-reader-swapped-red-and-green)): the originals and renders have since been regenerated, and
Jake confirmed the sprite picks on the corrected renders the same day. Terrain cannot use the overlay at
all -- the overland map is drawn in 3D, so no tile reaches the screen as a pixel copy.

**Players choose too.** The release ships the page: `lomhd_setup.py --review` renders every option
from the player's own game into `lomhd_work/review` (plus the palette pipeline for character
portraits, as its own tile), opens the page with the shipped picks preselected, and saves to
`my-upscale-choices.json`, which a plain install then uses instead of the shipped file.

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
- **Tests** build natively on macOS against a small `windows.h` shim (the types and
  `HeapAlloc`/`HeapFree` as `malloc`/`free`): `cc -O2 -std=c99 -I<shim> -Iinc -Itests
  tests/lomhd_{pack,find}_test.c src/lomhd_match.c src/lodepng.c`. `lomhd_match_test PACK
  FRAME.raw...` runs the shipped matcher over captured frames and prints what it finds and how long
  the search took; no Wine or Windows machine needed.
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

1. Cross-model review (Claude + Codex) of the wrapper -- **first round done 2026-09-22.** Both
   found the draw reading the game's surface outside the lock (a use-after-free on surface
   release) and 32-bit overflow in the pack parser; Claude alone found GL objects reused across
   cnc-ddraw's context recreation, the writer not enforcing the reader's limits, and no parser
   tests. All fixed (fork `ff9605c`, `cecf1fd`), with `tests/lomhd_pack_test.c` whose control
   build fails the wraparound and oversized-upscale cases against the pre-fix parser.
   **Second Codex pass (2026-09-22) found three more, all confirmed and fixed (fork `700e952`):**
   a GL 3.0 state query ran before the context was vetted, so in a 2.x context cnc-ddraw's own
   error check would turn the OpenGL renderer off; `make CFLAGS=...` dropped the new dependency
   flags; and the wraparound test passed with the 64-bit check reverted, because the 512 cap also
   refused it. The test now asserts the refusal offset, and a mutant restoring the 32-bit check
   fails it.
2. Live test on GS5R3 with all 748 portraits -- **run 2026-09-22 on a clean build, no errors**;
   lord (strip and info panel) and recruitment detected. 🔴 The runs before it used a DLL linked
   from two layouts of `LOMHD_PACK` (the Makefile tracked no headers; the log's "probe width
   11964208" gave it away) -- fixed in `1412b7d`. The other 745 GS5R3 portraits have not been on
   screen yet.
3. A wider playtest across every portrait consumer (barracks, alchemist, character screens, spy
   panel, combat) and a longer session.
4. Windows -- **passed 2026-09-22** (Windows 11 26200, RTX 3070, driver 591.86, vanilla Steam
   install, release 0.1.1). The setup found the game itself, the pinned downloads verified, the 3070
   upscaled all 445 portraits, and the elven archer in recruitment matched its upscale on screen.
   What Windows taught us:
   - 🔴 **The Windows Steam install ships no `ddraw.dll` and no `ddraw.ini`.** cnc-ddraw then runs
     with `renderer=auto`, which picks **Direct3D 9 on real Windows** (OpenGL only under Wine), so
     the overlay would have loaded and never drawn. Fork `e815726` + `c41dd7e`: a pack beside the game tips
     `auto` to OpenGL when OpenGL loads (else auto keeps its Direct3D 9 fallback); an explicit renderer is never overridden. cnc-ddraw writes a default
     `ddraw.ini` on first run; the install record notes it did not exist and uninstall sets it aside as `ddraw.ini.lomhd-saved` (never deletes it: the player may have tuned it).
   - **Upscales are visually equivalent across GPUs, not bit-identical.** The 3070's 440 shared
     portraits differ from the Mac's in about half their pixel indices, but by 3.5/255 per pixel,
     and 1.0/255 after a 3x3 blur: fp16 rounding amplified by Floyd-Steinberg dither.
   - Driving it remotely: OpenSSH runs in session 0, so a program started there is invisible.
     `steam://` from session 0 started `LOMLauncher.exe` in session 0. A one-off `schtasks /it`
     task runs in the logged-in desktop session instead. Scp needs `-O` (no SFTP subsystem).
5. Packaging -- **built 2026-09-22** (`release/hd-overlay/`, `scripts/build-hd-overlay-release.sh`,
   zip 196 KB, no game art). The player runs `lomhd_setup.py`: it reads the portraits with
   `tools/mpq_read.py` (pure-stdlib MPQ reader, byte-identical to StormLib on every named member of
   vanilla and GS5R3 `pic.mpq`), fetches Real-ESRGAN ncnn v0.2.5.0 and `ultrasharp-4x` with pinned
   SHA-256s, upscales, packs, backs up `ddraw.dll` and installs; `--uninstall` restores it.
   Reviewed by Claude and Codex (all findings fixed: Steam "Verify integrity", a locked DLL on a
   running game, torn writes, strict explode, SECTOR_CRC). End to end on a copy of the dev
   profile's files: 749 portraits in 11.5 minutes, uninstall restored the original DLL byte for
   byte. Licences: the model is **CC BY-NC-SA 4.0**, so it is downloaded, never shipped;
   Real-ESRGAN ncnn and cnc-ddraw are MIT; the explode port credits zlib's `blast.c`. The release
   build compiles the DLL twice with no PE timestamp and refuses if the two differ (one build in
   ten differed once, cause not found).

🔴 **The pack played on 2026-09-22 was only 353/748 real upscales.** The other 395 -- mostly
artifact and item art -- were 2x2 pixel repeats copied from the 2x UI experiment
(`mods/ui-2x-infopan/.../PORTRAIT_lowercase` and friends), which look exactly like the original
once drawn. Nothing seen in play was among them, so no run could notice; a byte comparison against
a fresh recipe run did. `hd_portrait_pack.py` now leaves any pixel-repeat "upscale" out with a
reason. The release recipe makes real upscales for all 749.

## Known gaps

- **Not ours: dialog remnants after "Buy Potion".** Closing the potion dialog in a Mage Tower leaves
  pieces of it on screen until the next map repaint. Reproduced 2026-09-22 in the untouched GS5R3
  profile (stock `ddraw.dll` `85e0f7d5`, no overlay files), so it is the game's own dirty-rectangle
  repaint, not the overlay. If a player reports it, this is the answer.

- The on-screen **size does not change**; this buys sharpness at the slot's size. Bigger portraits
  need layout changes as well (the re-flow in `mods/ui-bigportrait` works for the info panel).
- **A hostile pack can exhaust memory while inflating** (this lodepng has no output cap). Accepted:
  the pack sits beside `ddraw.dll`, so anyone who can write one can replace the DLL.
- The vanilla **faith banner** is not in the pack; GS5R3's is, but has not been seen on screen yet.
- A portrait **clipped at the screen edge, or with all three probe rows covered**, is not detected
  and stays vanilla -- a safe failure.
- Only the **OpenGL** renderer draws the overlay.
