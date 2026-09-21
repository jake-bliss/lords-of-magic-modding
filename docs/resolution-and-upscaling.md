# Screen resolution, and whether upscaled art would work

**Verdict: the formats would mostly take 2x art; the renderer will not use it.** The framebuffer is
hard-coded 640x480x16 with no configuration path anywhere in the image, so doubled art is a zoom,
not a remaster. Getting real detail out of this game is a patched-binary or reimplementation
project, which is what [the native engine plan](native-engine-plan.md) is for.

Written 2026-09-21. Every assertion carries an evidence class.

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
