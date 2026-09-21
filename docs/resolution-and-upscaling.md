# Screen resolution, and whether upscaled art would work

**Verdict: the framebuffer patches fine and the game runs at 1280x960 — but terrain and sprites
decouple, so a bigger world needs all 41,373 sprite frames redrawn.** ⭐ **Measured by an attended
binary patch on 2026-09-21, not argued.** The framebuffer is hard-coded 640x480x16 with no
*configuration* path, but it is seven constants wide and they were patched and run. Getting real detail out of this game is a patched-binary or reimplementation
project, which is what [the native engine plan](native-engine-plan.md) is for.

Written 2026-09-21. Every assertion carries an evidence class.

## ⭐ The 2026-09-21 spike: patched to 1280x960 and played

**Observed in gameplay, 2026-09-21.** `lomse.exe` was backed up, patched and restored (all
checksums verified). Three tiers.

| tier | patched | result |
| --- | --- | --- |
| 1-2 | `SetDisplayMode`, primary surface, app-object fields (6 immediates) | ✅ **boots, renders, fully playable.** Window is a true 1280x960; the UI draws at native 1:1 in the **top-left quadrant**; menus and the loaded game are clickable and **input coordinates still line up with the drawing** |
| 3 | the three NDC-to-viewport floats at `0x0054D7D4` (`320/-320/192` -> `640/-640/384`) | ⚠️ **terrain scaled 2x; sprites did not** |

**Tier 1-2 is an unambiguous pass.** DirectDraw under DXVK accepts a non-640x480 mode, the engine
runs in it, and the mouse-to-screen mapping is not separately hardcoded — so the input path follows
the layout for free. The UI sitting in one quadrant is the *expected* outcome and is a
script-coordinate problem, not an engine one.

**`playvideo` is already resolution-derived.** The intro video drew at a new offset without being
touched (*Observed in gameplay*), so at least one blit path computes position from the screen
dimensions rather than a literal.

### 🔴 The finding that decides the strategy: terrain and sprites decouple

**Observed in gameplay.** With the camera constants doubled, **terrain rendered twice as large while
every unit sprite stayed its original pixel size.** Riders on a magnified landscape.

*Derived:* the terrain is a 3D mesh drawn through the parameterised camera, so it follows the
projection. Units, trees and buildings are **IMP sprites blitted at native pixel size**, positioned
by `map2screen` but not *scaled* by it — their position follows the camera and their size does not.

**Therefore any change that scales the world requires every sprite redrawn at the new scale.** That
is **41,373 IMP frames**, plus 1,377 PBMs and 26 tilesets. This is no longer an inference from asset
counts; it was put on screen.

⚠️ **And the "three constants" framing was wrong.** The NDC floats are the **zoom**, not the viewport
extent: they map NDC `[-1,1]` onto a half-width in pixels, so doubling them magnifies the same world
rather than revealing more of it. The map's clip/viewport rectangle is a *separate* constant, still
unidentified — plausibly among the four unaccounted `push 640` / `push 480` sites (`0x4b293d`,
`0x4c945d`, `0x504c6c`, `0x531145`). Showing **more world at the same detail** would mean widening
the projection frustum, which `perspective` / `orthographic` / `create3dmap` expose to script and
may not need a binary patch at all. **Untested.**

### What this makes viable, and what it kills

- 🔴 **A 2x *world* is not a patch project.** It is an art project of the original game's scale.
- ✅ **A 2x *UI* is viable and independent.** Hold the map at 1x (leave the camera constants alone),
  double the script layout literals, and redraw only the UI chrome and the portrait members.
  Portraits go from 4,690 to 18,760 pixels — the difference between a thumbnail and a portrait.

  🔴 **The two counts in this bullet were estimates and both were wrong.** Measured 2026-09-21:
  **1,012 literal `doodad` rects and 975 `additem` placements across 127 of the 1,491 `.gs`
  members** (not "~5,000 literals"), and **749 portrait members, of which 353 are nameable by
  `portrait_file_names`** (not 149). The conclusion survives the correction — 749 UI images against
  **41,373 IMP frames** for a 2x world is still about a 30x difference — but the estimates should
  not be quoted.
- ⚠️ That path still needs the shared UI sheets (`intspr1_page` and the dialog backgrounds) redrawn
  as one coherent batch, because their contents are cut by literal coordinates.

### The patch manifest, for whoever repeats this

*Observed in a local binary.* Offsets are file offsets; all seven were verified against their
expected current bytes before writing.

| constant | file offset | 640x480 | 1280x960 |
| --- | --- | --- | --- |
| `SetDisplayMode` height | `0x7466d` | `480` | `960` |
| `SetDisplayMode` width | `0x74674` | `640` | `1280` |
| primary surface height | `0x7470e` | `480` | `960` |
| primary surface width | `0x74713` | `640` | `1280` |
| app-object width | `0xfcf44` | `640` | `1280` |
| app-object height | `0xfcf4e` | `480` | `960` |
| NDC x scale | `0x14c1d4` | `320.0f` | `640.0f` |
| NDC y scale | `0x14c1d8` | `-320.0f` | `-640.0f` |
| NDC y offset | `0x14c1dc` | `192.0f` | `384.0f` |

`ddraw.ini`'s cnc-ddraw window was set to 1280x960 so the frame was judged 1:1 rather than
letterboxed.

🔴 **The PE layout note this project had been using was incomplete.** `lomse.exe` has **four**
sections, not two: `.text` VA `0x401000` (raw `0x400`), **`.rdata` VA `0x54d000` (raw `0x14ba00`)**,
`.data` VA `0x555000` (raw `0x153200`), `.rsrc` VA `0x5d7000` (raw `0x176400`). The float constants
live in `.rdata`, which was undocumented — converting their VA with the `.data` formula reads a page
of zeros, which it duly did on the first attempt.

## ⭐ CLOSED: the 70x67 portrait was never an engine limit

**Observed in gameplay, 2026-09-21, two attended runs.** The 2x-UI path above stopped being a
prediction. Both halves of it were put in front of the engine and both held.

### Why the corpus could not answer this

All **749** `portrait\` members in GS5R3's `pic.mpq` are **exactly 70x67** — zero variance on the
one axis in question — and both call sites cut them with `... 0 0 70 67 doodad`. A corpus with no
variance on an axis cannot be interrogated about that axis, which is the same shape as the
[fixture problem](../CLAUDE.md): a body of evidence that agrees with itself teaches nothing.

Two other corpus facts made a *prediction* possible, and the runs were designed to break it rather
than to go looking:

- **`lbm` is not size-constrained.** `pic.mpq` holds **120 distinct image dimensions**, 46 of them
  in `building\` alone. The loader reads `BMHD` and allocates from it.
- **`doodad page x y w h` cuts a rect out of a LARGER page.** Hundreds of sites do exactly that
  against sprite sheets — `unitinfo_staticon5R3A 503 33 127 34`, `intspr1_page 313 126 27 18`. A
  page bigger than its rect is the normal case everywhere in the UI *except* portraits.

### Run 1 — an oversize portrait loads, and the rect crops it

`mods/portrait-oversize`. Three Life portraits replaced with a quadrant test card — four solid
colours, a black cross on the seams, a black border. `gs.mpq` and `imp.mpq` untouched, so no probe,
hotkey or edited dialog could explain the reading.

| rung | member | card | predicted | observed |
| --- | --- | --- | --- | --- |
| **A** writer control | `LIMISP00.LBM` Archers | **70x67** | the whole card | **whole card** ✅ |
| **B** subject | `LICAVp01.lbm` Riders | **140x134** | solid green, black frame | **green, black border** ✅ |
| **C** subject | `LIINFP00.LBM` Staffmen | **140x134** | solid green, black frame | **green, black border** ✅ |

**Rung A is what makes B and C mean anything.** Same writer, same palette, same chunk shape, native
size — so a failure at 140x134 could not have been blamed on the encoder.

⭐ **The black border is the load-bearing detail.** In the crop, black on the top and left is the
card's own border, but black on the **right and bottom** is the cross at x=69 and y=66 — and the
cross exists *only because the image is 140 wide and 134 tall*. A 70x67 image cannot produce it. So
the reading is specifically the top-left quadrant of the oversize card, not a green square from some
fallback path.

### Run 2 — a bigger rect paints more screen pixels, at 1:1

`mods/portrait-2x-doodad`. **One** script change: `gs\dlg\NEWBUILD5a.gs`'s barracks portrait rect,
`0 0 70 67` -> `0 0 140 134`. `gs\dlg\INFOPAN5.gs` deliberately left at 70x67.

| slot | rect | observed |
| --- | --- | --- |
| barracks recruit portrait | 140x134 | **the whole card, at double size, overflowing its frame** |
| pop-out unit panel | 70x67 | **solid green** — the crop, unchanged |

⭐ **The two slots disagreed, reading the same member, in the same sitting.** No single-mechanism
story produces that; it is the same discriminating pattern that settled the portrait panel on
2026-09-21, and it is why the rects were split rather than both raised.

**It also establishes which barracks module is live.** `gs\dlg\NEWBUILD5.gs` `run`s `NEWBUILD5a.gs`
and `NEWBUILD5b.gs` by name; `NEWBUILD50.gs` is byte-identical to `NEWBUILD5a.gs` (both 87,143
bytes) and `newbuild.gs` is the vanilla original. Only `NEWBUILD5a.gs` was patched, and the barracks
changed — so that is the one the engine loads.

### What this settles

**The 70x67 portrait is a number in a script, not a property of the engine.** Both the loader and
the blit are size-agnostic. A 2x UI needs no binary patch for the portrait path at all.

### What it does not settle

- ✅ **Mouse hit-testing was the last open risk and it is now CLOSED.** See the run below.
- ⚠️ **Fonts.** 39 bitmap font members in `pic.mpq`. Doubled panels with un-doubled text will look
  wrong, and fonts are the least automatable art in the set.
- ⚠️ **Not every UI rect is in script.** The map viewport clip rect is a binary constant and remains
  unidentified.
- 🔴 **Whether the engine honours a portrait's own `CMAP`.** There are **193 distinct palettes**
  across the 749 portraits and **not one palette entry is common to all of them**, so the corpus
  cannot say whether the engine reads each file's palette or remaps it onto a screen palette. Every
  portrait build so far has stayed inside colours that already display correctly, which is true
  under either answer — so nothing has tested it.

### Run 3 — a click follows the drawn position

**Observed in gameplay, 2026-09-21.** `mods/button-hit-rect`. **One literal**, in a member proven
live by run 2:

```
gs\dlg\NEWBUILD5b.gs   ; TRAIN/HIRE UP ARROW
  building_dialog_panel up_arrow_button 273 320   ->   ... 60 320
```

Only x moves, so the button travels on one axis and two displacements cannot confound the reading.
`down_arrow_button` stays at `273 337` as a control in the same dialog, driving the same counter.

| rung | action | predicted | observed |
| --- | --- | --- | --- |
| **A** | click the up arrow **where it now draws**, far left | count rises | **rises** ✅ |
| **B** | click the **empty spot** it used to occupy | nothing | **nothing** ✅ |
| **C** | click the unmoved down arrow | count falls | **falls** ✅ |

⭐ **A button's hit rectangle comes from its `additem` placement — the same literal that draws it.**
B is the rung that carries the result: had the hit rect been held separately, the old position would
still have responded. It did not.

**Why the subject had to be changed first.** The main menu was the obvious test — reachable on
launch, no setup. It was abandoned because **nothing in `gs.mpq` references `newdlg.gs` or
`NEWDLG5.gs`, and `lomse.exe` contains no such string**. Where the main menu is loaded from is
**Unknown**, and a subject whose liveness is unknown cannot carry rung B: "nothing happened" would
have had two causes. The barracks pair is live by measurement, not assumption.

### The art, settled separately

**353 of 353 kept.** Every portrait `portrait_file_names` can name was upscaled with Real-ESRGAN
ncnn `ultrasharp-4x` to 140x134 and quantised back into **that portrait's own 256 colours**, then
reviewed one by one against the original at the same physical size
(`artifacts/portrait-review/`, verdicts in `verdicts.json`). The verdict was *better or no worse*,
unanimously, across every faith and unit class.

⚠️ **The model invents, and the amount is measured.** Downscaling each candidate back to 70x67 and
comparing against the original, over three portraits: faithful resampling deviates **4.8/255** mean;
`ultrasharp-4x` deviates **9.7/255** with local deviations to **56**. That is a known, accepted cost
— the alternative (a 50/50 blend, 6.5/255) was offered and declined after a side-by-side. It is not
an oversight and should not be "fixed" without asking.

**An engine result fell out of it.** A portrait decoded, upscaled, requantised and **re-encoded** by
this pipeline renders correctly. Re-encoded portrait palettes work. That is narrower than it sounds
— every colour used was already in the member's own `CMAP` — so whether a *new* palette would be
honoured is still **Unknown**.

### So the whole chain is closed

| link | evidence |
| --- | --- |
| `lbm` loads arbitrary dimensions | corpus — 120 distinct sizes in `pic.mpq` |
| `doodad` crops a rect from a larger page | **gameplay, run 1** |
| a bigger rect paints more screen pixels at 1:1 | **gameplay, run 2** |
| a click follows the drawn position | **gameplay, run 3** |
| the framebuffer can be 1280x960, UI still clickable | gameplay, PR #83 |
| upscaled portrait art is worth having | 353/353 reviewed |

**A 2x UI is a coordinate transform plus art.** No binary patch beyond the six resolution
immediates, and no engine work at all.

### What is left, and it is not a blocker

- ⚠️ **Fonts.** 39 bitmap font members in `pic.mpq`. Doubled panels with un-doubled text will look
  wrong, and fonts are the least automatable art in the set. This is now the largest open item.
- ⚠️ **Shared sheets are cut by literal coordinates** (`intspr1_page`, the dialog backgrounds), so
  they have to be redrawn and re-cut as one coherent batch rather than piecemeal.
- ⚠️ **Not every UI rect is in script.** The map viewport clip rect is a binary constant, still
  unidentified.
- 🔴 **Whether the engine honours a NEW portrait `CMAP`** — untested, see above.

### ⚠️ The same upscaler on SPRITES: tried, and it is a different problem

**Measured 2026-09-21, offline.** The portrait result invites the obvious question — run the model
over `imp.mpq` and get a 2x world for the cost of GPU time. The cost is indeed not the obstacle:
`ultrasharp-4x` takes ~0.8s a frame, so **41,373 frames is about 11 hours**, which is one overnight
run rather than an art department.

Two things stop it being the same job, and both are properties of the format rather than of the
model.

🔴 **IMP transparency is 1-bit, and the model produces 240 levels.** `units\imp\lifita.imp`
frame 0 has **2 distinct alpha values** (0 and 255) — a cutout. The 4x result has **240**. IMP has
no alpha channel; a pixel is the transparent index or it is not. Every one of those intermediate
values has to be thresholded back to binary, which discards exactly the softened silhouette the
model was adding, and makes the threshold itself a visible choice on every sprite edge. Portraits
never hit this because a portrait is an opaque rectangle.

🔴 **Palette index 1 is the shadow, keyed by INDEX not by colour** — measured in the running engine
2026-09-17, see [hotspots](hotspots.md) and [the native asset stage](native-asset-stage.md). An
RGB upscale followed by re-quantisation has no way to know that index 1 means "draw the background
at half intensity": it will interpolate the shadow against its neighbours and scatter the result
across whatever indices are nearest in colour. The shadow has to be lifted out as its own mask,
scaled as a mask, and stamped back — not carried through the colour pipeline at all.

⚠️ **And the hotspots move.** Placement is `top_left = anchor + placement - (w>>1, h>>1)`
([hotspots](hotspots.md)); doubling a frame without doubling its anchor and hotspot records puts
every unit in the wrong place. That is a data edit in the `.imp` itself, not a re-encode.

**So the PR #83 verdict should be read more precisely.** "A 2x world is an art project of the
original game's scale" is right that it is not a patch, and **wrong if it is taken to mean 41,373
hand-drawn frames**. It is: solve the alpha threshold once, solve the shadow-mask path once, scale
the hotspot records, then spend a night of GPU. That is a real project and an order of magnitude
smaller than redrawing. **None of it is verified in the engine** — no upscaled sprite has been put
in front of the game, and the alpha and shadow handling above are the reasons not to until they are
written.

### 🔴 A rect that must not be raised

```
/infopan_portatrait unitinfo_staticon5R3A 180 0 70 67 doodad def      (INFOPAN5.gs:1949)
```

That cuts 70x67 out of a static **icon sheet**, not a portrait page — and a replace of the literal
`0 0 70 67 doodad` catches it **by substring**, because `180 0 70 67` ends with `0 0 70 67`. Raising
it silently doubles an unrelated interface element. Match on the page NAME
(`unit_portrait_page0`..`3`, `unit_portrait_page`) and assert the sheet cut still reads 70x67
afterwards.

## The blocker: 640x480 is two `push` immediates

**Observed in a local binary.** At `0x00475263`:

```
475263  8b 07                 mov  eax,[edi]
475265  68 24 87 55 00        push 0x558724   ; "DDOBJECT->SetDisplayMode(ScreenWidth(),
                                              ;  ScreenHeight(),BITS_PER_PIXEL)"
47526a  6a 10                 push 0x10       ; 16 bpp
47526c  68 e0 01 00 00        push 0x1E0      ; height = 480   <- immediate
475271  8b 08                 mov  ecx,[eax]
475273  68 80 02 00 00        push 0x280      ; width  = 640   <- immediate
475278  50                    push eax
475279  ff 51 54              call [ecx+0x54] ; IDirectDraw::SetDisplayMode
```

The engine's own debug string calls them `ScreenWidth()` and `ScreenHeight()`, as though they were
runtime queries. They are compile-time constants. They appear again as immediates in the
app-object constructor: `movl $0x280, 0x22fe8(%esi)` at `0x4fdb3e` and `movl $0x1e0, 0x22fec(%esi)`
at `0x4fdb48`, and again at the primary-surface creation at `0x47530b`.

### There is no other path, and here is the reach of that negative

**Observed in a local binary.** An exhaustive `push imm32` sweep of `.text`:

| value | occurrences |
| ---: | ---: |
| 640 | 7 |
| 480 | 4 |
| 800 | 2 (assert line numbers — the adjacent pushes are `0x3fa` and `"C:\lomse\source\storm.h"`) |
| **600** | **0** |
| **768** | **0** |
| 1024 | 13 |

**No 800x600 and no 1024x768 path exists anywhere in the image.**

**Observed in a local binary.** The complete command-line switch table at `0x005733a8` is:

```
/s=  /x=  /debug  /nodebug  /cd=  /nompq  /testseed=  /notrimlogs=
```

**No `/w=`, `/h=` or `/res=`.** `/x=` (parsed at `0x004feeaf`) sets only field `+0x6ac`, which
selects `DDSCL_EXCLUSIVE|FULLSCREEN` against `DDSCL_NORMAL`; the 640x480 surfaces are created
unconditionally after the branch rejoins.

**Observed in the corpus.** Neither config file carries a resolution field: `settings.cfg` has 23
keys (scroll speeds, volumes, combat display toggles), `lom.cfg` 160 bytes across 11 fields
recovered from `saveconfig` (`0x00487570`) and `loadconfig` (`0x00487580`).

**What this sweep could not see:** a resolution assembled arithmetically rather than pushed as a
literal, or one loaded from a global written by code the sweep did not attribute. Both are possible
in principle; neither is suggested by anything found.

⚠️ **`English/ddraw.ini`'s `1120x882` is not a counter-example.** That is cnc-ddraw upscaling the
finished 640x480 frame in a window, downstream of everything above.

## Why that kills 2x art rather than merely limiting it

⚠️ **Scope, added 2026-09-21.** The three reasons below are all about **doubling art while the
framebuffer stays 640x480**. They do *not* argue against doubling the framebuffer too — reasons 1
and 3 dissolve at 1280x960, and reason 2 is a camera matrix that responds to its constants. The
spike above is the answer to that separate question. Do not quote this section against a
patched-framebuffer proposal, which is a mistake made once already.

Three independent reasons, any one sufficient.

1. **No added detail, by definition.** On a fixed 640x480 framebuffer, 2x art does not show more
   of anything. It draws each sprite twice as large. That is a zoom.
2. **The grid does not move with it.** **Observed in gameplay** (`research-log.md:1375-1403`,
   encoded in `tools/map_projection.py:33-36`): the measured per-cell step is **33.941 px**
   (= 24·sqrt(2)) in screen x, 14.4 px per iso step, 20.3625 px per elevation level. **None is 32
   or a multiple of it.** The cell-to-screen projection is a 3D camera matrix (`map2screen`
   `0x0046B0C0` -> `0x00469AE0`, two 4x4 multiplies then NDC-to-viewport with float literals
   320.0 / -320.0 / 192.0 at `0x0054D7D4`) and it knows nothing about how big the sprites are.
   Doubling sprites on an unchanged stride is misplacement and overlap.
3. **The largest sprite stops fitting on the screen.** `units\imp\deldrada.imp` is 334x250; at 2x
   it needs a **668x628** surface, larger than the 640x480 primary and far larger than the 640x384
   map viewport.

**The one escape hatch, and why it is not one.** `scale` is a real scriptable native (`0x0046af90`,
arity 7, mutating camera state at `0x5876d0`), alongside `rotatemap`, `perspective`, `orthographic`
and `create3dmap`. Camera scale *can* be doubled from GameScript — but on a fixed framebuffer that
shows a quarter of the map at the same apparent detail. It relocates the problem.

## What the formats would have allowed

Recorded because it is the non-obvious half, and because it is exactly what a reimplementation
would inherit.

| Field | Where | Width | Corpus max | 2x fits? |
| --- | --- | --- | --- | --- |
| `maximum_width` / `maximum_height` | IMP header +4 / +6 | u16 | 334 / 314 | yes |
| frame `width` / `height` | frame record +2 / +4 | u16 | 334 / 314 | yes |
| `origin_x` / `origin_y` | frame record +8 / +10 | i16 | -66..70 / -207..77 | yes |
| hotspot `x` / `y` | hotspot record +2 / +4 | i16 | -115..123 / -232..86 | yes |
| PBM BMHD width / height | +0 / +2 | u16 BE | — | yes (1280x960 is 2% of range) |
| **IMP `encoded_size`** | **frame record +6** | **u16** | **64,232** | **NO** |

**The near-miss is worth knowing on its own.** The largest single frame payload is
`imp\detort1a.imp`, 259x248 at 8bpp uncompressed = **64,232 bytes, 98.0% of the u16 ceiling**. The
1998 authoring tool was working right against that wall, and it shows: of 39,325 frames under 16k
pixels 89% are 8bpp, but of the 47 frames at 65k pixels or more, **24 are 4bpp** — including every
frame of the largest sprite in the game. At 2x, roughly 71-157 frames would overflow.

**Buffers are not a limit.** **Observed in a local binary.** The IMP load path at `0x0049B539`
reads the header maxima and calls `0x0049B180`, which takes the max against the existing surface
and calls the same CreateSurface helper that builds the primary. There is no cap, assert or ceiling
constant on that path — the surface simply grows. The limit is DirectDraw and video memory, and
whether a 668x628 surface survives in exclusive 16-bit mode is **Cannot determine offline**.

**Palette is unaffected but constrains the upscaler.** Art is 8-bit palettised, 256 entries (IMP:
256x4 = 1,024 bytes, engine's own copy loop at `0x0049B220`; PBM: 768-byte CMAP). Upscaling does not
change colour depth. But **Inferred**: any bilinear, Lanczos or AI upscale produces off-palette
colours and must be re-quantised to the file's own 256 entries or the encoder refuses
(`pbm.rs:229-237`). Nearest-neighbour doubling is palette-exact and free. **So the upscale that adds
detail is the one that fights the palette, and the one that does not fight the palette adds no
detail** — which is the framebuffer problem restated on the colour axis.

## A consequence for the mirror rule

**Doubling art makes every frame width even.** [hotspots.md](hotspots.md#the-rule) records that the
engine subtracts one further pixel when a mirrored frame's width is **even**, and that this branch
has **never been put in front of the engine** — the 2026-09-20 run that established the mirror sign
used a 65-wide frame. A 2x art project would walk into that untested branch on every mirrored frame.

## Open questions

| # | Question | How to settle |
| ---: | --- | --- |
| 1 | Does the engine read IMP `encoded_size` at all? A sweep of the whole IMP module (`0x499000`-`0x4AD5F1`) found ~30 word reads at frame-record `+2`/`+4` and **zero** at `+6`. If it is dead, our writer is stricter than the engine. | Attended run: a frame with a deliberately wrong `encoded_size` |
| 2 | Does a 668x628 DirectDraw surface survive in exclusive 16-bit mode? | Attended run |
| 3 | Does the engine read `TILESIZE=` from a `.til` at all? All 26 shipped tilesets declare `32, 32`. | Attended run; `til-format.md:202` already refuses writes to that field on this ground |
| 4 | ~~What value does `START.GS` pass to `maxgraphics`?~~ **Answered 2026-09-21: `3500 maxgraphics`.** | closed |

### `maxgraphics` is 3,500, and it is a slot count worth watching

**Observed in the corpus, 2026-09-21.** `START.GS` declares `3500 maxgraphics`, alongside
`200 maxpalettes` and `300 maxdialogs`. The native is `0x004d9ee0`, arity 1, writing two 4-byte
fields of a static record at `0x5a7b50` that `getimpmemory` (`0x004db4a0`) reads back.

**Inferred:** it is a **slot count**, not a byte budget. Worth watching for two separate reasons:
`imp.mpq` alone holds 3,600 members after rung 5's addition, so 3,500 is not a comfortable margin
over the art that exists; and a community note (*Documented, unverified*) reports `maxgraphics`
being "roughly doubled" in `START.gs` because `copydoodad`'s graphics never unload. Any mod that
**adds** art — including [a new unit](new-units.md), which adds two IMP members and a portrait —
spends slots here.

⚠️ Open question 1's negative comes from a mnemonic grep over a linear disassembly. It would miss a
read through an already-advanced register, a scaled-index form, or code outside that range.
Treating it as settled would overclaim.
