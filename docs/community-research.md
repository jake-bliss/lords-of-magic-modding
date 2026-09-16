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

**Frame byte `+1` is a delay and the dword at `+8` is a checksum.** Both are snv's guesses, marked
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

**The palette is stored BGRA and swapped to RGB on load.** Confirms our channel order, which we had
been unable to test.

**Frame type 4 with size 0 is also a duplicate**, pointing back at a shared bitmap. Matches our
`0x04` shared-pixels handling exactly, independently derived.

**Frame `Size` may be 0 because the RLE is self-terminating.** This is the same distinction as our
two record variants, one with explicit stored sizes and one whose payload length is implicit.

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
[map format](map-format.md), plus a falsifiable prediction. Parked in issue #4.

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
57 is 103 of our 300-file sample, so our sub-byte decoder covers a third of the corpus that no public
tool reads. The `.exe` tools remain undownloaded and unrun. Their existence does not reduce the value of an independent, tested,
cross-platform decoder, but it does mean **we are not the only party decoding these formats**, and
their author has solved at least one problem we have open.

## What changed here as a result

- `spikes/asset-viewer/src/png_export.rs` now writes a `tRNS` chunk marking palette index 0
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

Deferred: renaming `ImpCycle` to `ImpFacing` touches over 100 sites across two modules and is a
mechanical change better done on its own. The equivalence is documented in
[native asset stage](native-asset-stage.md).

## How to use this document

Treat every community statement as a hypothesis with a named source, not as specification. The two
refutations above were both from the mod's own author about his own code, and both were wrong in
ways that would have propagated into our documentation had they not been checked. Where community
material and our corpus disagree, the corpus wins and the disagreement is recorded.
