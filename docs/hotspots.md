# IMP sprite placement and hotspots

**This file is the single source of truth for how the engine places a sprite frame.** Other documents
should link here rather than restate the rule. Everything below was measured in the running engine or
read out of `lomse.exe`; the derivations, with dates and methods, are in the
[research log](research-log.md).

Resolves [issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1).

## The rule

```
top_left = anchor + placement - (width >> 1, height >> 1)
```

The stored `placement` pair is the vector from the anchor point to the **centre** of the frame, in
screen pixels with **`+y` downward**, and it is **added**. Halving is `floor`, i.e. a plain shift.

**When the engine mirrors a frame, the x half of the rule changes** — and it is not simply the
same expression with a sign flipped:

```
unflipped:  top_left.x = anchor.x + placement.x - (width >> 1)
flipped:    top_left.x = anchor.x - (width >> 1) - placement.x - (1 if width is EVEN else 0)
```

`placement.y` and the height term are unchanged; mirroring is horizontal only.

**Observed in a local binary.** `ImpPlayer::GetPlacement` (`0x0049CC80`) computes the unflipped
value at `0x0049CD01` and the flipped value at `0x0049CCC8..0x0049CCD3`. The even-width `dec` is
read from the instruction stream, not deduced: reflecting the unflipped span about the anchor
column reproduces the odd-width result exactly and lands **two** pixels away on even widths, so
that extra pixel is an engine convention rather than a consequence of the mirror. Implement
`imp_anim::mirrored_anchor_x`, not the algebra.

**Observed in gameplay, 2026-09-20 — the odd-width branch only.** Three independently recovered
anchors, a 65-wide frame with `placement.x = +14`: unflipped predicts a left edge of 370, flipped
predicts 342, and 342 was measured, three times. A 28-pixel discrimination. See
[the run sheet](unit-anchor-run-sheet.md#the-2026-09-20-run).

⚠️ **The even-width `dec` has never been put in front of the engine.** It rests on the
disassembly alone. A mirrored frame of even width is the next rung for anyone extending this.

Two consequences worth stating plainly:

- **Shipped `y` values are negative** because art grows upward from where the object stands.
- **The convention is centre-relative, not corner-relative.** Re-cropping art changes the placement it
  needs by *half* the crop on each axis. That is why the board's crop-and-re-centre workaround kept
  almost working: the error tracked the edit instead of staying fixed.

## Where the placement is stored

Frame record bytes `+8..+12` are **overloaded**, and which meaning applies is decided by the record's
hotspot-count byte at `+1`:

| Hotspot count | Meaning of bytes `+8..+12` | Frames in GS5R3 |
| --- | --- | --- |
| `0` | `origin_x: i16`, `origin_y: i16` — the placement itself | 15,725 |
| `> 0` | `u32` file offset to the hotspot array; the placement is **record 0** of that array | 28,771 |

**Stale, 2026-09-17.** Both counts were measured with the frame-table bug corrected that day, which
swallowed 29 frame records across five files. A spot re-measurement over the 1,798 stem-paired
sprites reads 15,677 and 28,800; that is a different population from the one above, so these totals
are pending re-measurement rather than replaced. The split itself — which record kind a frame
carries — is unaffected.

**And the rule on this page is untouched.** Measured over all 1,800 sprites in the archive, exactly
**five files and 29 frame records** were affected: `aicr3b`, `chcr3b`, `chwmmb`, `ficr3b` and
`ficr5b`, which are precisely the five that were failing header validation. The validator was a
complete detector of the bug. The placement measurements behind the rule were taken on frames of
sprites that are not among those five, and the totals above are wrong by at most those 29 records.
| — | duplicate / back-reference frames carry neither | 7,170 |

A frame therefore carries **one form or the other, never both**. Hotspot records are 6 bytes
(`id: u16`, `x: i16`, `y: i16`) and the array is padded to an 8-byte boundary.

### Record 0 is engine-reserved

`getimphotspot` (`0x0049BF90`) and `enumimphotspots` (`0x0049C1D0`) both begin their walk at record
index **1**, byte offset 6, and both bail out when the count is `<= 1`:

```asm
mov  cx,[edx]          ; frame record's first u16
shr  ecx,8             ; record count, from record byte +1
cmp  ecx,eax           ; eax = 1
jle  <fail>
mov  ebx,[edx+8]       ; hotspot array pointer
lea  edx,[ebx+6]       ; start at record 1, not record 0
```

So **no GameScript can read or enumerate record 0 by any means.** Corpus agreement: record 0 carries
type 0 in 99.62% of frames, type 0 never appears in any other slot, and no frame has fewer than two
records.

Because the engine never reads record 0's *type tag*, an unusual value there is harmless.

## Hotspot types

Values come from the engine's GameScript constant table at `0x00560108`, stored as 8-byte
`{char* name, int value}` pairs.

| Value | Names | Meaning, from script usage |
| --- | --- | --- |
| 0 | `NO_HOTSPOT` | No anchor — the effect is drawn on the unit as a whole. Also the tag on the reserved placement record. |
| 1 | `CURSOR_HOTSPOT`, `MISSILE_ORIGIN_HOTSPOT` | Launch anchor for attack and breath projectiles. |
| 2–5 | `SPELL_ORIGIN1..4_HOTSPOT` | Caster-side emission points. The hydra uses 2 and 3 for different heads. |
| 6 | `FLAP_OFFSET_HOTSPOT` | Never passed to a native by any script; name suggests a wing-flap offset. |
| 7 | `MISSILE_TARGET_HOTSPOT`, `SPELL_TARGET_HOTSPOT` | Impact anchor on the target. The default for per-spell auras. |
| 8 | `STREAMER_HOTSPOT` | Never passed to a native by any script; name suggests a trailing-streamer attach point. |

**Eleven names, nine distinct values, two aliased pairs.** `BOLT_HOTSPOT_S0..S3` and `D0..D3` are
**not** hotspot types — they are field indices into a bolt definition record (values 15–22, in a block
ending `BOLT_SPELLDEF_ID` 23 and `BOLT_RESULT_PROC` 24), and `MISSILE_HOTSPOT` 14 is the same kind of
thing. Text describing a "19-constant hotspot vocabulary" predates this and is wrong.

`getimphotspot` and `enumimphotspots` are never called by any script in the 4,692-member corpus; they
are tool-facing. The one script consumer of these constants is `addauratype` in `gs\aura.gs`.

## Writing placement

```sh
# solve for the value a re-cropped frame needs
lom-asset-viewer --imp-placement-for WIDTH HEIGHT ANCHOR_X ANCHOR_Y TOP_LEFT_X TOP_LEFT_Y

# write it back into a loose IMP
lom-asset-viewer --set-imp-placement IN.imp FRAME X Y OUT.imp            # origin-pair frames
lom-asset-viewer --set-imp-placement IN.imp FRAME X Y OUT.imp --hotspot 0 # record-bearing frames
```

Worked example: `palm1b.imp` frame 0 is 53x53 at `(9, -20)`. Pad the art by 4 pixels on every side to
61x61 and it needs `(13, -16)` — each axis shifts by exactly half the added pixels.

The writer keeps file length identical so stored offsets stay valid, re-parses before writing, refuses
a placement it cannot read back, refuses to overwrite an existing output, refuses a duplicate frame's
origin, and warns when several frames share the record or the hotspot array being written.
`--describe-imp` shows which form a frame uses.

## Why this matters

`lomut`, the tool the community used for years, writes **no hotspot array at all**. Since the
placement is authored per frame and not derivable from the art — across 28,447 unit frames, `x` is
independent of frame width (median exactly 0) and height explains only about half of `y` — art that
passes through `lomut` cannot have its placement reconstructed. It has to be written back, which is
what the commands above are for.

**This is not an indictment of every community tool.** Hexdragon's `LOM_Sprite_Tool` (impz thread
2086, November 2023) is *described by its author* as preserving the hotspot structure and adjusting
the `+8` dword as a file offset when the count is non-zero, while leaving it alone when the count is
zero because it is then the displacement pair they call `DspX`/`DspY`. That is a community claim we
have not verified — the tool has not been run here. What it cannot do, and what nothing else does either, is
say what placement a **re-cropped** frame should now carry. Carrying `DspX`/`DspY` over unchanged is
right when the pixels do not move and wrong the moment they do, which is every attempt to fix the
wobble by cropping. That is the gap the rule above closes.

## Still open

- ~~The shadow blend.~~ **Answered 2026-09-17: palette index 1 draws the background at half
  brightness.** Of 903 index-1 pixels in the control and 889 in a copy whose index-1 entry had been
  rewritten to magenta, **100%** rendered within one palette step of exactly half the background, and
  the two copies rendered identically. The spread is the snap to the nearest entry in an indexed
  framebuffer, so the blend is a palette remap rather than per-pixel arithmetic — and the entry's own
  RGB really is ignored, now by controlled test rather than inference.
- ~~A channel-order discrepancy.~~ **Answered 2026-09-17: palette entries are stored blue, red,
  green, pad.** Pairing every index in a frame against the pixel the engine painted, `(p1, p2, p0)`
  fits 14 of 14 sampled indices and the next best permutation fits 4; writing raw `ff 00 00`,
  `00 ff 00` and `00 00 ff` rendered blue, red and green respectively. Our decoder reversed the
  triple, which **swapped red and green and left blue correct** — exactly the "agree where red equals
  green" symptom this entry used to describe. Fixed in `imp.rs`; against the capture the old mapping
  scored 3/10 and the new one 10/10. It also refutes the community specification's "BGRA, swapped to
  RGB" claim, which we had accepted.
- Related, on the reader side: `screencapture` writes its pixel bytes as R, G, B rather than the
  BMP-standard B, G, R. Confirmed numerically on known materials — carved stone, wood and parchment
  come out first-byte-dominant 62-91% against 0.8-2.4%. Use `tools/probe_captures.py`.
- ~~A unit-path caveat.~~ **Answered 2026-09-20: the unit draw path computes its anchor the same
  way the terrain-sprite path does.** Record 0 was originally confirmed by placing a unit IMP
  through the *terrain sprite* path, which left a unit-specific constant in the *anchor*
  unruled-out. [`LOM_PROBE=unitanchor`](unit-anchor-run-sheet.md) ran on **three cells**, each with
  its own control rung and therefore its own independently recovered anchor, and the residual is
  **zero in x and y on all three**. Three anchors agreeing leaves no room for an additive
  unit-specific offset. **Inferred**, conditional on one frame identification that is unique among
  all 86 frames of the file in both orientations; see
  [the run sheet](unit-anchor-run-sheet.md#the-2026-09-20-run). Two things stay open and are not
  this caveat: only **one unit type** has been measured, and all four placements across both runs
  reported the same facing.
- ~~The engine mirrors frames, and this file does not say what that does to `placement`.~~
  **Answered 2026-09-20: `placement.x` is negated.** Stated in [the rule](#the-rule) above. The
  2026-09-19 run could not tell `+placement.x` from `-placement.x` because the frame that drew had
  record-0 `x = 0` and the two predict the identical pixel; the 2026-09-20 subject was chosen for a
  record-0 `x` far from zero on every STAND frame, and the frame that drew had `x = +14`.
  Unmirrored predicts a left edge of 370, mirrored predicts 342, and 342 was measured — a
  **28-pixel** discrimination, reproduced on all three cells.
- **The world-map unit sprite is the `...b.imp` zoom variant, not `...a.imp`.** `gs\imps.gs`'s
  `unit_zoom_letter` maps COMBAT_SCREEN and LOCATION_SCREEN to `A` and SCROLLINGMAP_SCREEN,
  REGION_SCREEN and WORLD_SCREEN to `B`, and the engine pushes the screen mode. A world-map army is
  also a **composite** — body, faith flag at `(0,-40)`, health bars, group number at `(0,-20)` —
  so a bounding box around a placed unit is several sprites, not one. Both facts cost a wrong
  reading of the 2026-09-19 captures.
- **Out-of-vocabulary types in searchable slots.** `units\imp\eacr5a.imp` carries types 106, 138 and
  143 in slots 1 and 2, which the engine does search. Unexplained. (Its record-0 tag of 136, and
  `aiwm1b.imp`'s 190, are accounted for: record 0's tag is never read.)
- **Types 6 and 8** have names but no observed behaviour.

## Do not

- **Do not use `drawimpframe`.** It type-checks its six operands, looks up the imp, and paints
  nothing. Zero call sites in 1,471 scripts. It is vestigial.
- **Do not trust the recovered arity of an operator.** The arity walk undercounts operators that pop
  through the shared helper at `0x0040ADB0`. Three have been caught: `drawimpframe` (6, reported 5),
  `map2screen` (3), `getimphotspot` (5, reported 1). Read the entry point before designing an
  experiment around an operator.
