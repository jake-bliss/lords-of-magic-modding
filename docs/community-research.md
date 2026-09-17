# Community Research Survey

## Status

**Survey complete; every load-bearing claim checked against the local corpus.** On 2026-09-16 the
surviving Lords of Magic modding community was surveyed for prior art: Mantera's fan site and the
`impz.proboards.com` "LOMSE Modding" board. Both are live. The board is still active, with tool
releases in June and July 2026.

This document records what the community knows, what it gets wrong, and what changed in this
repository as a result. Community material is **Documented** evidence at best. Two headline claims
were **refuted** by our own corpus, so nothing here should be cited without its verdict.

No proprietary text, script source, or binary from these sources is stored in Git.

## Sources

| Source | URL | Nature |
| --- | --- | --- |
| Mantera's LOMSE site | `http://mantera.xorgate.com/website.html` | HTTP-only frameset, created 2004, last updated 2007-02-19 |
| GS5 changelog | `http://mantera.xorgate.com/mods/GS5/history.html` | Per-revision changelog naming `.gs` files and functions |
| LOMSE Modding board | `https://impz.proboards.com/board/18` | 169 threads, active through July 2026 |
| IMP sprite format | `https://impz.proboards.com/thread/2012/imp-sprites-rle-algorithm` | snv's 2011 binary spec and RLE loop |
| Auto-calc spec | `https://impz.proboards.com/thread/2243/auto-calc-explained-numbers` | Boaster's combat-resolution formula |
| GS5R3 updates | `https://impz.proboards.com/thread/1682/gs5r3-updates-reports` | 167 posts, 2010-2021 |
| GSZ updates | `https://impz.proboards.com/thread/1948/gsz-updates-reports` | GSZ changelog |
| MPQ repack ruleset | `https://impz.proboards.com/thread/2102/error-when-mpq-edditing` | Compression settings for `.gs` members |
| 2026 tool suite | `https://impz.proboards.com/thread/2590/lords-magic-utility-suite-open` | eyesodilated's MPQ/IMP/map toolchain |
| Sprite hotspot thread | `https://impz.proboards.com/thread/2176/great-masters-necropian-abyss-summon` | 115 posts, 2014-07 to 2026-06; the hotspot and mirroring mechanism |

The site is HTTP-only with no TLS listener, so any fetcher that force-upgrades to HTTPS fails with
`ECONNREFUSED` on port 443. Use plain HTTP. The forum redirects to HTTPS and rate-limits automated
requests with a proof-of-work challenge; it was read at a polite rate and the challenge was not
circumvented.

People worth knowing: **Boaster** (aka Mantera, aka Ellelone) authored GS5R3 and GSZ and wrote most
of the technical exegesis; **snv** reverse-engineered the IMP format and wrote `lomut` in 2011;
**eyesodilated** is the most active current reverse engineer; **orzie** maintains the "Legends of
Urak" total conversion.

## Claims checked against our corpus

### Refuted

**`extra_strong?` controls difficulty-scaled AI bonuses.** Boaster posted this in December 2020 with
the body `true getmultiplayerflag{pop false}if`. The shipped GS5R3 body, in `gs\scenario\default.gs`
lines 147-152, is `[false false false]getdifficultylevel get` — **false on Easy, Medium and Hard**.
The name appears in exactly two GS5R3 files and in neither 3.02 nor baseline, and it appears nowhere
in `gs\LEVLMODS5.gs`. The word that actually gates the AI stat bonus there is `insane_mode?`. He
quoted a hand-edited or pre-ship version; treat `extra_strong?` as vestigial as installed. The
underlying phenomenon is real — see [Difficulty and AI](difficulty-ai.md).

Executing both bodies later confirmed the charitable reading: the shipped body is `false` at every
difficulty, while the body he quoted returns `true` on Hard in single-player and `false` in
multiplayer, which is precisely the behaviour he described. He was describing code that did not ship,
not misremembering the game.

**A definition ends with `;`.** The forum fragment's trailing `;` is a line comment, not a
terminator. Definitions end with `def`: the GS5R3 corpus has 77,391 bare `def` tokens, 6,462 `}def`
sequences, and zero occurrences of `}` followed by `;`. Our lexer already had this right.

**Palette index `0xff` is an RLE special value.** We have no `0xff` case in the decoder, yet stored
pixel-byte totals match the generated-header ground truth for all 1,798 validated pairs. There is no
escape at the compression layer.

**`.h` files are "instructions that tell the game how to handle the IMP file".** They are generated C
build headers — `#define DRAGON_MOVE 0` symbol pairs plus build statistics. Our use of them as
validation ground truth is the correct reading.

**`gs5_globals.gs` supplements EXE variables in GS5R3.** The member ships, and its content is exactly
as described: 50 lines of `/NAME{value}def` constants. But `START.GS:34` has its `run` commented out,
with the author's note that he was *"observing the possibility"*. The constants are defined inline in
`START.GS` instead. It is evidence that script `def` **can** shadow native names — confirmed
independently by `START.GS:76` redefining `run` itself — not evidence that GS5R3 ships that override.

**Frame byte `+1` is a delay and the dword at `+8` is a checksum.** *(snv's 2011 forum post only;
the later IMP Studio port drops both guesses and agrees with us.)* Both are snv's guesses, marked
with question marks in his own template. They are our hotspot count and hotspot-array pointer, and
the dword dereferences to well-formed 6-byte `(id, i16 x, i16 y)` records whose totals match the
generated-header hotspot statistic for 1,798 pairs with zero mismatches.

### Confirmed, and useful

**The transparency index is a header field, not a hardcoded 0.** `IMP-Studio-Help.txt` and the
accompanying Python port of `lomut`'s `imp.c` document header byte 3 as `ColorKey`, *"transparency
index (usually 0)"*. Measured across a 300-file sample: **94 files carry a nonzero key**, and on those
sprites palette slot 0 never occurs in the pixel data while the key index is the frame's most common
value. We were hardcoding 0 and rendering those backgrounds opaque. Fixed; see
[native asset stage](native-asset-stage.md). This is the single most valuable thing the survey found.

**The palette is stored BGRA and swapped to RGB on load.** **Refuted 2026-09-17** — entries are stored **blue, red, green, pad**, measured in the running engine. We accepted this claim, so our decoder inherited the same red/green swap. It had appeared to confirm our channel order, which we had
been unable to test.

**Frame type 4 with size 0 is also a duplicate**, pointing back at a shared bitmap. Matches our
`0x04` shared-pixels handling exactly, independently derived.

**Frame `Size` may be 0 because the RLE is self-terminating.** This is the same distinction as our
two record variants, one with explicit stored sizes and one whose payload length is implicit.

**Frame byte `+1` and the dword at `+8` are hotspot fields.** IMP Studio names them `HSType` and
`HSpot`, with `HSType` 0 meaning the dword holds a packed XY pair and 2/3/4 meaning it is an offset
to a hotspot struct. That is structurally identical to our reading, which treats the byte as a
hotspot count and the dword as either two `i16` origins when the count is 0 or a pointer to that many
6-byte records otherwise. Two independent derivations converged on the same layout from opposite
directions.

The open question in that paragraph — *"whether the byte is a type tag or a count"* — is now
**settled as a count**, by ozz in thread 2176 and independently by measurement here. The consequence
for the dword at `+8` is that it is **overloaded**: when the count is zero those four bytes are the
`origin_x`/`origin_y` placement pair, and otherwise they are a `u32` offset to the record array. See
[the hotspot mechanism](#the-hotspot-mechanism-thread-2176) below and [hotspots](hotspots.md).

**Palette index 1 is the shadow.** This resolves our open "secondary mask" question. Our own
observation of *"a separate pure-red index for a 1,651-pixel silhouette beneath the creature"*
independently corroborates it. Compositing is keyed by palette **index**, not by colour; the green
and red RGB values in slots 0 and 1 are incidental art-tool choices. This retires the "not a single
universal chroma key" framing.

**Our "cycle" is a facing — a direction — and facings are ordered clockwise.** Clockwise ordering is
an untested community claim, but the naming is better than ours and is adopted in prose.

**Frame type `0x08` is a duplicate back-reference whose offset is a frame number, and duplicates
encode animation delay so must not be stripped.** Our parser already does exactly this. Since snv's
"delay?" byte is refuted, duplicate repetition is the only timing mechanism either side can find.

**The RLE algorithm matches ours exactly**, including the `+3` run-length bias on the positive branch
and the signed-byte negative branch. Confirmed term by term. Our decoder additionally terminates on
the frame's packed size rather than the file-level virtual canvas, which is what the bytes support.

**`getdifficultylevel` is native, and difficulty is a 0/1/2 index.** GS5R3 adds a script-side
`/INSANE_LEVEL 3 def`, implying the native constants are 0, 1, 2. Two idioms dominate: table indexing
`[25 50 75]getdifficultylevel get` and arithmetic `getdifficultylevel add`.

**MPQ repack requires a specific ruleset.** Per orzie: `.gs` members must be packed *"with ruleset
like Diablo 1's (Options - Implode+Encrypt(0x00010100); Compression - IMPLODE)"*, or the game reports
`gs.mpq file is corrupt`. Ladik's MPQ Editor corrupts portraits on repack; use WinMPQ to edit
existing archives and Ladik's only to create new ones. **This matters the first time we write
anything back** and is recorded now so it is not rediscovered painfully.

### Open, worth testing

**Maps carry a 4-byte compression header that vanishes on oversized maps.** *Partly refuted on
measurement — see [map format](map-format.md).* The field is not a version number: across 365
installed maps it takes 20+ values in `0x3f`-`0x6f`, independent of geometry, clustering by file
family. A tileset selector fits better. The claim's testable half, that oversized maps omit the field
entirely, remains open.

Original claim: eyesodilated, July 2026:
*"Lords of Magic stores a 4-byte compression header in every map. It's basically just a version
number and reserved space. However, when maps exceed the original maximum size, that header
disappears. That's why some maps and save files become corrupted, and why the game may display
'TRASHBIN.'"* This is an independent name for the unknown `metadata` field at offset `0x00` in
[map format](map-format.md), plus a falsifiable prediction. Parked in [issue #22](https://github.com/jake-bliss/lords-of-magic-modding/issues/22).

**Auto-calc combat resolution.** Boaster's formula: each army's collective **barter value** is scaled
by a **Total Army Factor** derived from Team Points accumulated over three tiers — highest
individual, combined total, and average — comparing level, attack-versus-armour (using whichever of
melee or ranged attack is higher), and armour-versus-attack. Team Points are multiplied by the ratio
of army sizes. In the unmodded game a solo champion's barter value was multiplied by `1 / size of
opposing army`, which GS5 and GSZ removed. Relevant if we ever model combat; not yet checked against
`gs\AUTOCALC5.gs`.

**A Game Script Manual existed.** Boaster sold it as a donation-gated PDF from 2011. Its table of
contents covers system symbols, mathematics/operands/procedures, real versus integer stats, and unit,
artifact and spell editing. Never published; only the contents listing survives publicly.

## Independent map corpus

Eight community maps were downloaded from Mantera's site on 2026-09-16 and parsed with zero failures
— the first maps this project has tested that did not ship with an installed profile. One,
`Feuerundeis.scn`, is **160x160**, a dimension absent from all three installs and outside the
previously documented set. See [map format](map-format.md).

## Cross-check against IMP Studio, 2026-09-16

`impstudio.py` is stdlib-only with no network, subprocess, or `eval` use, and writes only to explicit
output paths, so it was audited and then run read-only against all 1,800 IMP members extracted from
the GS5R3 `imp.mpq` into a temporary directory.

### Round-trip: 1,800 of 1,800 byte-identical

Their `verify` command passes on every file. **Read carefully, this is a weaker result than it
sounds.** `serialize_passthrough` copies the original bytes and re-packs only the header and frame
headers over them, writing back the same values it read. It therefore proves the **byte layout** —
that no field is mis-sized or misaligned anywhere in 1,800 files — but not the semantic naming, since
two `u1` fields misread as one `u2` would round-trip identically. Their help text is honest about
this ("with no edits it is byte-identical"). The semantics of `ColorKey` were established separately,
by measuring pixel usage.

It is still a genuine independent confirmation of the header layout we share.

### Decode coverage: they reach 91.6% of non-duplicate frames, we reach all of them

Their `scan` command over the same 1,800 files, counting only non-duplicate frames since duplicates
are skipped by design:

| Type | Files | Non-duplicate frames | They decoded | Coverage |
| --- | ---: | ---: | ---: | ---: |
| 8-bit RLE (9) | 957 | 35,798 | 35,798 | 100% |
| 8-bit raw (8) | 606 | 1,064 | 1,064 | 100% |
| 1-bit RLE (25) | 27 | 804 | 804 | 100% |
| **4-bit RLE (57)** | **188** | **3,388** | **0** | **0%** |
| Unclassified | 10 | 88 | 0 | 0% |
| **Parse error** | **12** | — | — | — |
| **Total** | 1,800 | 41,142 | 37,666 | 91.6% |

Where their decoder runs, it agrees with ours completely — 100% on all three implemented types. The
gap is the two categories it does not attempt:

- **Type 57, 4-bit RLE: 188 files, 3,388 frames, zero decoded.** Their documentation states plainly
  that `lomut` never implemented this path, so there was nothing faithful to port. Our decoder reads
  it: exporting frame 0 of `aura\agx01aa.imp` produces a correct 21x44 indexed PNG with a `tRNS`
  chunk and recognizable art.
- **Twelve files crash their parser** with `index out of range` — four `building\*lad*`, two
  `imp\flag*`, two `missile\*bps`, and four `units\imp\*` members. Our parser reads all twelve;
  `imp\flagblue.imp` for instance resolves to two sequences of 1x1 frames. These are not among our
  ten known validation mismatches.

**Conclusion.** On the formats both tools implement, two independent decoders agree exactly, which is
strong mutual corroboration. Our coverage is strictly larger: every one of the 1,800 members parses
and expands here, including 188 files and 3,388 frames no public tool decodes. Their advantage
remains write-back, editing, and map repair, which we do not attempt at all.

## The hotspot mechanism, thread 2176

Thread 2176 is 115 posts spanning 2014-07-30 to 2026-06-13 — eyesodilated, Boaster, orzie and **ozz**
— and it is the origin of the "512x512 hotspot" problem recorded in issue #1, now **closed**: the placement was measured and is writable with `--set-imp-placement` (see [hotspots](hotspots.md)). It matters for two
reasons: it states the mechanism precisely, and the statement is testable against our corpus.

### The nine-year workaround ladder, and why it never closed

eyesodilated tried to add new unit sprites and found each newly compiled unit selectable across a
512x512 region of the battlefield. The fixes he found each broke the previous one:

| Fix | Consequence |
| --- | --- |
| Crop frames to their minimum extent | frames wobble between animations |
| Re-centre each frame on the original (256,256) | hotspot region grows back |
| Pad the frame bottom so the unit sorts in front of scenery | health bar floats far above the unit |
| Lower `/health_bar_y` to `-35` in the unit's `.gs` | works, but per-unit and manual |

This ladder is the reason the problem looks like a sizing problem. It is not.

### The actual mechanism, per ozz, 2023

**`lomut` never writes hotspot data at all.** Recompiled IMPs come back with `HSType = 0`, so the
engine reads a packed XY displacement from a field that instead holds a stale pointer. Cropping
frames only shrinks the damage; it never restores the missing records. Every downstream symptom —
wobble, health-bar placement, depth sorting — follows from the absent hotspot array, which is why
fixing any one of them by hand re-broke another.

ozz's reading of the frame record, which matches ours field for field:

- `+1` `HSType` — the **number** of hotspot records for this frame, not a type tag.
- `+8` `HSpot` — a packed XY displacement when `HSType` is 0, otherwise a **file offset** to that
  many records, stored near end of file and padded to an 8-byte boundary.
- Each record is `(id: u16, x: i16, y: i16)`. **Record index 0 is engine-reserved** and holds the draw placement: `getimphotspot` (`0x0049BF90`) and `enumimphotspots` (`0x0049C1D0`) both begin their walk at index 1, so no script can read it.

He also notes that **snv's `imp.c` hotspot struct is wrong**: it hardcodes two hotspot sets when the
count is variable. That defect is inherited by every tool ported from it.

### Verified here

Measured across all 1,800 IMP members of the GS5R3 `imp.mpq`, then spot-checked by an independent
hex parse that does not share code with the Rust decoder:

**Hotspot record layout — confirmed.** `liwiza.imp` frame 0, the file ozz hex-dumped, holds
`00 00 00 00 e8 ff 07 00 00 00 dd ff` at its `HSpot` target: types 0 and 7 with `i16` displacements,
exactly as described. Our 6-byte `(u16 id, i16 x, i16 y)` record reads it correctly.

**Record count per frame reaches 9**, not the 2 that `imp.c` assumes, nor the 5 that ozz had
observed:

| Records per frame | Frames |
| ---: | ---: |
| 2 | 24,412 |
| 3 | 2,621 |
| 4 | 1,287 |
| 5 | 190 |
| 6 | 212 |
| 7 | 27 |
| 8 | 12 |
| 9 | 10 |

**Types 0 and 7 are near-universal** — 28,661 and 28,183 occurrences — corroborating ozz's "present
in all units". **The near-universal "type 0" is record 0, the engine-reserved draw placement rather
than a hotspot** — see [hotspots](hotspots.md). Types 1 through 6 and 8 appear in the hundreds to low
thousands. Ids 9, 10 and 16 also occur and are *outside* the nine-value vocabulary, not part of it.

**`lomse.exe` defines 19 `*_HOTSPOT`-shaped constants, not 10 — but only 11 are IMP hotspot types.** (Corrected 2026-09-16: the eight `BOLT_HOTSPOT_S0..D3` names are field indices into a bolt definition record, values 15-22, not type tags. The real vocabulary is nine values, 0 through 8, and their numbers are now read from the exe's constant table at `0x00560108`.) ozz's list came from a code comment; the
binary's string table is authoritative:

```
NO_HOTSPOT 0            CURSOR_HOTSPOT 1        MISSILE_ORIGIN_HOTSPOT 1
SPELL_ORIGIN1_HOTSPOT 2 SPELL_ORIGIN2_HOTSPOT 3 SPELL_ORIGIN3_HOTSPOT 4
SPELL_ORIGIN4_HOTSPOT 5 FLAP_OFFSET_HOTSPOT 6   MISSILE_TARGET_HOTSPOT 7
SPELL_TARGET_HOTSPOT 7  STREAMER_HOTSPOT 8

; NOT hotspot types -- bolt-record field indices, read from the exe 2026-09-16:
BOLT_HOTSPOT_S0..S3 = 15..18   BOLT_HOTSPOT_D0..D3 = 19..22
MISSILE_HOTSPOT = 14           (block ends BOLT_SPELLDEF_ID 23, BOLT_RESULT_PROC 24)
```

Those extra names do **not** explain the observed ids 10 and 16. The vocabulary is nine values, 0 through 8; ids 9, 10, 16, 106, 136, 138, 143 and 190 are genuinely outside it. See [hotspots](hotspots.md#hotspot-types). The engine
also exports the natives **`getimphotspot`** and **`enumimphotspots`**, so hotspots are reachable
from GameScript directly — relevant to issue #5. Both start their walk at record index 1, though, so record 0
(the draw placement) is unreachable from script.

**ozz asked whether the hotspot comment block is in the original `aura.gs`. It is not, but the names
are real.** Shipped `aura.gs` is a single line with no `;` comments anywhere. The identifiers are
live in it: `NO_HOTSPOT`, `SPELL_TARGET_HOTSPOT`, `SPELL_ORIGIN1_HOTSPOT` and
`SPELL_ORIGIN2_HOTSPOT` are all passed to `addauratype`. So his list is genuine engine vocabulary,
just incomplete and sourced from a comment that the shipped scripts do not carry.

**Mirroring — confirmed.** ozz: byte 1 of the sequence record is the mirror flag, and any value
`>= 128` mirrors. Across 4,629 sequences:

- **No unmirrored sequence has more than 2 facings.** Unmirrored sequences are overwhelmingly
  single-facing (1,230 of 1,286).
- **All 28 of the 33-facing sequences are flagged mirrored**, matching his account of arrows storing
  33 facings and mirroring to 64 directions.
- Unit sequences with 5 facings are mirrored in all 2,234 cases.
- Byte 3 is `0x01` in 4,623 of 4,629 sequences; byte 4 is `0xff` in all 4,629. ozz's "4th byte is
  always 1" holds to 99.87%.

### Open: hotspot types outside the engine's vocabulary

`units\imp\eacr5a.imp` uses types 106, 136, 138 and 143 on all 110 frames and carries **neither type
0 nor type 7**, which every other unit has on every frame. `units\imp\aiwm1b.imp` uses type 190 on 25
frames alongside a normal type 0.

This is **not** a decoder defect. An independent hex parse confirms the bytes: `eacr5a` frame 0 holds
`88 00 fe ff 07 00 6a 00 ff ff ea ff 8a 00 fe ff ea ff` at its `HSpot` target, and the pattern
repeats regularly across all 110 frames rather than degrading as corruption would. Both files also
pass `--validate-imp` against their generated headers, so frame counts and pixel-byte totals agree.
The bytes are real and we read them faithfully; what the values *mean* is unexplained. Worth putting
to ozz, who has spent the most time in this structure.

## Prior art we did not know about

The board hosts a working toolchain that overlaps our Stage 1 scope:

- **`lomut`** (snv, 2011) — CLI for IMP extract/compile, LBM/PCX conversion, MPQ extract/create.
  Known defects: recompiled sprites fail to mirror, and hotspot areas absorb transparent regions.
- **Sprite and Archive Utility (`sau`)** (SNV) — multi-format retro archive converter listing both
  Lords of Magic `.imp` and Blizzard `.mpq` among roughly 80 formats.
- **Lords of Magic SE Utility Suite** (eyesodilated, July 2026) — `ImpSweets v1.8`,
  `MapEditor v0.19`, and a portrait injector. Claims direct MPQ reads without WinMPQ or `lomut`,
  preservation of the IMP properties `lomut` dropped, map/save repair by rebuilding the missing
  4-byte header, and SVG import. Reports built-in portrait animation effects in static LBM portraits.

`IMP Studio` was obtained on 2026-09-16 as Python and HTML source and read, not run. It describes
itself as a byte-for-byte port of `lomut`'s `imp.c` and is the best format documentation located so
far — better than the 2011 forum post, because it is executable and annotated. Its stated limits are
informative: **sprite type 57 (4-bit RLE) is not decoded, because `lomut` never implemented it**. Type
57 is 188 files and 3,388 frames of the full 1,800-member corpus, which no public
tool reads. The `.exe` tools remain undownloaded and unrun. Their existence does not reduce the value of an independent, tested,
cross-platform decoder, but it does mean **we are not the only party decoding these formats**, and
their author has solved at least one problem we have open.

## What changed here as a result

- `spikes/asset-viewer/src/png_export.rs` now writes a `tRNS` chunk marking the header's colour-key index (`ImpSprite::color_key`)
  transparent. Exported indexed PNGs were previously fully opaque, silently losing the transparency
  key that the interactive viewer already honoured. Covered by a test.
- `spikes/asset-viewer/src/main.rs` renames `secondary_mask` to `shadow` and documents that
  compositing is keyed by palette index rather than colour.
- `spikes/asset-viewer/src/imp.rs` gains `back_reference_frame_count`, which counts only `0x08`
  frames. **Validating that against the generated header's "Duplicate bitmaps found" statistic
  instead of the combined count raises corpus failures from 10 to 112**, which proves that statistic
  counts `0x04` shared-pixel frames too and that our existing conflation is correct. The hypothesis
  was tested and refuted; the counter is retained for analysis and the finding recorded in the field
  doc comment.

`ImpCycle` has since been renamed `ImpFacing` throughout the code, docs, and CLI output; sequence
rows in `--describe-imp` now cross-reference `facing:N` rather than `cycle:N`.

## How to use this document

Treat every community statement as a hypothesis with a named source, not as specification. The two
refutations above both came from the mod's own author describing his own code, and both would have
propagated into our documentation unchecked. Note what later execution showed about one of them: the
`extra_strong?` body he quoted behaves exactly as he said, so he was describing real code that did
not ship rather than misremembering how the game works. That distinction matters, and the accurate
claim is the narrower one — a source can be describing a different build than the one you installed. Where community
material and our corpus disagree, the corpus wins and the disagreement is recorded.
