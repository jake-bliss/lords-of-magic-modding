# Native Asset Layer — Stage 1

## Status

**In progress; archive inventory, IMP inspection/export, terrain-atlas rendering, and the dominant map-object record milestone are complete.** The native Rust tool can read the five core GS5R3 archives without modifying them, recover their public filenames, classify all 9,804 members, probe common standard formats, fully decode the observed PBM image corpus, and decode the pixels, frame references, sequences, facings, origins, and hotspot records of all 1,800 IMP sprite binaries. It also **writes sprite placement back** into a loose IMP (`--set-imp-placement`, with `--hotspot 0` for the record-bearing form) and solves the placement a re-cropped frame needs (`--imp-placement-for`). It also bounds all 365 installed `.scn`, `.smp`, and `.lgd` files, resolves standard map cells through original terrain art, and structurally decodes 21,117 placed-object records across the six record layouts all 365 maps use.

This is useful tooling now, but it is not yet the lossless asset layer promised by Stage 1. Full map/scenario semantics, the shadow-blend half of compositing, reimport/repacking, and cross-platform packaging remain open; placement is settled (see [hotspots](hotspots.md)).

## Component boundaries

| Component | Responsibility | Must not own |
| --- | --- | --- |
| MPQ adapter | Read-only archive open, enumeration, external listfile loading, member reads | Asset interpretation or archive writes |
| Format decoders | Bounds-checked parsing of PBM, IMP, BMP, and WAVE structures | Game-specific compositing or simulation |
| Asset probe | Content-first classification and typed metadata | Rendering state |
| CLI | Inventory, catalog, inspect, extract, indexed-frame export, validation, sprite-placement solve and write-back (`--imp-placement-for`, `--set-imp-placement`), and viewer entry points | Format parsing logic |
| SDL viewer | Native presentation of decoded pixels | Archive or decoder policy |

StormLib remains behind a small unsafe FFI boundary. The rest of the crate consumes safe Rust-owned names and byte buffers.

## Reproduce the local inventory

The repository contains no original game data. Fetch the public name list, then point the inventory at a legally obtained installation:

```sh
scripts/fetch-lom-listfile.sh
scripts/inventory-native-assets.sh \
  '/path/to/Lords of Magic Special Edition/English' \
  'artifacts/native-stage1-local'
```

`fetch-lom-listfile.sh` downloads Ladislav Zezula's public Lords of Magic listfile over its original HTTP endpoint only after pinning both the ZIP and extracted-file SHA-256 hashes. It refuses to overwrite an unrecognized local file. Generated inventories and the fetched listfile live under ignored `artifacts/` paths.

## Measured GS5R3 corpus

Measured on the preserved local GS5R3 profile on 2026-09-11:

| Archive | Entries | Classified contents | Probe failures |
| --- | ---: | --- | ---: |
| `pic.mpq` | 1,406 | 1,377 PBM, 2 BMP, 26 tile-set definitions, 1 listfile | 0 |
| `special.mpq` | 1,218 | 1,218 WAVE | 0 |
| `gs.mpq` | 1,700 | 1,689 GameScript, 7 empty, 2 text, 1 URL, 1 listfile | 0 |
| `imp.mpq` | 3,600 | 1,800 IMP binaries, 1,800 generated C headers | 0 |
| `sndfx.mpq` | 1,880 | 1,880 WAVE | 0 |
| **Total** | **9,804** | | **0** |

The 3,098 WAVE members are probed for RIFF chunks, encoding, channels, sample rate, bit depth, data length, and duration. BMP members are probed for dimensions, bit depth, and compression. Full sample/pixel conversion is not yet implemented for those formats because mature platform-independent decoders can likely be reused later.

## IMP findings

The paired generated `.h` files provide unusually valuable ground truth. The current parser has established:

- a 32-byte file header with palette and sequence-table offsets;
- 16-byte sequence, 8-byte facing, and 16-byte frame records;
- a 256-entry palette stored **blue, red, green, pad** (measured in the engine, 2026-09-17);
- maximum dimensions, sequences, facings, logical frames, and frame dimensions;
- explicit sequence-to-facing and facing-to-frame ranges, retained with their still-unknown raw metadata fields;
- action labels recovered from generated-header `#define` values cover 4,649 of 4,666 declared sequence slots; aliases are retained, and 1,799 of 1,800 headers provide at least one label;
- six-byte hotspot records padded per frame to an eight-byte boundary;
- direct duplicate-frame references and a compact repeated-facing representation;
- shared-pixel flag `0x04`, carried by individual records inside an ordinary frame-record array (a facing is never a repetition of one record — see the 2026-09-17 correction below);
- two observed frame-record variants, one with explicit stored sizes and one whose payload length is implicit;
- a custom packet RLE in which controls below `0x80` repeat the following byte `control + 3` times and controls at or above `0x80` copy `256 - control` literal bytes;
- 8-, 4-, 2-, and 1-bit indexed pixels selected by file-flag bits `0x30`, with both tightly packed and row-padded layouts;
- most-significant-bit-first packing for sub-byte pixels, which produces recognizable output across representative sprites;
- complete pixel expansion, frame-reference resolution, and sequence/facing traversal for all 1,800 observed binaries.

The shared-pixel correction removed 27 false origin records, bounded the remaining origin ranges to X `-66..70` and Y `-207..77`, and improved exact generated-header matches. Across the corpus, 15,725 logical frames carry signed origins and 28,771 carry 64,432 six-byte hotspot records (**stale, 2026-09-17**: measured before that day's frame-table fix, which swallowed 29 records across five files; a spot re-measurement over the 1,798 stem-paired sprites reads 15,677 / 28,800 / 64,492, a different population, so these are pending re-measurement rather than replaced). **Blast radius, measured 2026-09-17 over all 1,800 sprites in the archive:** exactly **five files and 29 frame records** had a facing whose first record was `0x04` followed by a genuine one, and they are precisely the five that were failing validation — `aicr3b`, `chcr3b`, `chwmmb`, `ficr3b`, `ficr5b`. So the validator was a complete detector of this bug, the totals above are wrong by at most those 29 records, and the placement rule in [hotspots](hotspots.md) is untouched: it was measured on frames of sprites that are not among the five. The hotspot bytes decode as `id: u16` then `x: i16`, `y: i16`. Frame record bytes `+8..+12` are **overloaded**: with a zero count byte at `+1` they are the `origin_x`/`origin_y` placement pair, otherwise a `u32` offset to `count` 6-byte records, of which **record 0 is engine-reserved and holds the draw placement**. Offsets show with observed coordinate ranges X `-115..123` and Y `-232..86`.

The ID is a **hotspot type**, and the numbers are now read directly out of `lomse.exe`'s constant table at `0x00560108` (8-byte `{name, value}` pairs): `NO_HOTSPOT` 0, `CURSOR_HOTSPOT` 1, `MISSILE_ORIGIN_HOTSPOT` 1, `SPELL_ORIGIN1..4_HOTSPOT` 2-5, `FLAP_OFFSET_HOTSPOT` 6, `MISSILE_TARGET_HOTSPOT` 7, `SPELL_TARGET_HOTSPOT` 7, `STREAMER_HOTSPOT` 8. **Corrected 2026-09-16:** the vocabulary is eleven names over nine distinct values 0-8, not nineteen. The `BOLT_HOTSPOT_S0..S3`/`D0..D3` names are field indices into a bolt definition record (values 15-22, in a block ending `BOLT_SPELLDEF_ID` 23 and `BOLT_RESULT_PROC` 24), not IMP hotspot types; `MISSILE_HOTSPOT` 14 likewise. Every value 0-8 appears in the corpus. Record **0** is engine-reserved and holds the **draw placement**, which is why its tag reads `NO_HOTSPOT`; `getimphotspot` and `enumimphotspots` both begin their walk at record 1. Ids 9, 10, 16, 106, 136, 138, 143 and 190 are genuinely outside the vocabulary - see [community research](community-research.md#the-hotspot-mechanism-thread-2176). Placement semantics are settled: `top_left = anchor + placement - (width >> 1, height >> 1)`, added and centre-relative. See [hotspots](hotspots.md).

For example, `units\imp\chcr5a.imp` contains seven named actions (`MOVE`, `STAND`, `DEFEND`, `GET_HIT`, `DIE`, `CORPSE`, and `MELEE_ATTACK`), five facings per action, and 170 logical frames. The five facings are likely directional views, but that interpretation and the remaining sequence/facing metadata have not yet been confirmed against the original executable.

The validator pairs generated headers with binaries and compares independently recorded sequence, frame, duplicate, raw-pixel, hotspot, and stored-pixel statistics where applicable. Pairing is by lowercased stem first; a member the stem left unmatched then falls back to the sequence name the header *declares*.

| IMP validation check | Result |
| --- | ---: |
| Header/binary pairs, by stem | 1,798 |
| Header/binary pairs, by declared sequence name | 2 |
| Exact matches on every statistic | 1,795 (99.7%) |
| Named, value-pinned exceptions | 5 |
| Unexplained failures | 0 |
| Documented orphan catalog entries | 2 |

**Observed in a local binary (2026-09-17).** `--validate-imp` reports zero failures. The five
members that cannot match exactly each carry an entry in `IMP_VALIDATION_EXCEPTIONS`
(`spikes/asset-viewer/src/imp.rs`) recording its class, its reason, and the exact measured numbers.
The waiver is value-pinned: an exception applies only when the observed disagreements are exactly
the recorded ones, so any decoder change that moves a number, drops a disagreement or adds one
re-fails the member. The two orphan catalog notes are value-pinned the same way and their members
are read and re-measured, so a truncated or substituted file at a catalogued name re-fails instead
of being accepted on its name. No bounds check is relaxed — every member is still fully parsed and
every statistic still compared.

**Corrected 2026-09-17 (first correction).** `ImpSprite::validate_against` used `?` on each
comparison in a fixed order, so a file reported only its *first* disagreement. It now collects all
of them. That alone changed the picture: five files previously described as disagreeing on the
duplicate tally alone in fact also disagreed on raw-pixel, hotspot and stored-pixel bytes.

**Corrected 2026-09-17 (second correction) — five of the ten exceptions were our own decoder bug.**
The decoder read `source[frame_table_offset] & 0x04` — the shared-pixel flag of a facing's *first*
frame record — and, when it was set, treated the whole facing as a repetition of that one record:
every frame slot in the facing pointed at the same 16 bytes, and the bounds check for the full
`facing_frames * 16` table was skipped so nothing noticed. A facing's frame table is in fact an
ordinary array of records in which only the first may carry `0x04`. Measured record flags:
`aicr3b` sequence 2 facing 0 is `[04 00]`; `chwmmb` sequence 5 facings 0-4 are each
`[04 00 00 00 00 00]`. Every record after the first was silently dropped, with its pixels and its
hotspot array:

| Member | Swallowed records | Swallowed raw pixels | Swallowed hotspot bytes |
| --- | ---: | ---: | ---: |
| `units\imp\aicr3b` | 1 | 12 | 24 |
| `units\imp\ficr3b` | 1 | 12 | 24 |
| `units\imp\ficr5b` | 1 | 30 | 16 |
| `units\imp\chcr3b` | 1 | 1,443 | 16 |
| `units\imp\chwmmb` | 25 | 12,865 | 400 |

Those are exactly the deltas the five `HeaderPredatesDeduplication` exceptions waived, on all three
quantities, for all five files. With the frame table read as an array, `validated` went 1,790 to
1,795, `validated_with_exception` went 10 to 5, `validation_failures` stayed 0 and nothing else
moved. All five now match their headers on every statistic.

**Refuted — "the header predates a deduplication pass".** It was previously recorded here as
**Observed** that `Duplicate bitmaps found` counts duplicates among the build tool's *input*
bitmaps and that the written file dedupes further. There is no evidence for that. The numbers it
explained were produced by the decoder bug above, and the explanation was fitted to them.

Why it was believed, and why that was not enough:

- The corpus inequality `binary_duplicates >= header_duplicates` held on 1,797 of the 1,798 stem
  pairs, the sole exception being `units\imp\orcr4b`. It still does — but it is equally consistent
  with the headers simply being right, which they now are. (The figure was previously quoted as
  1,799 of 1,800, and in the source as 1,797 of 1,798, which cannot both be right: the larger
  denominator silently included the two declared-name fallback pairs, where the header is a
  *foreign* one and agreement is not evidence about a build tool's own output. The validator now
  prints `dedup_compared_stem_pairs` as an explicit denominator and counts fallback pairs
  separately as `dedup_foreign_header_pairs`.)
- The hotspot arithmetic appeared to corroborate it: with
  `header_distinct = frames - header_duplicates`, the header's hotspot total equalled
  `binary_hotspot_bytes / binary_distinct * header_distinct` on four of the five files. Three of
  those four "predictions" were arithmetically forced: where the gap is one bitmap and the file's
  hotspot arrays are uniformly sized, that expression *has* to land, so it confirmed nothing. Only
  `chwmmb`, with a 25-bitmap gap, carried information — and the swallowed-record measurement
  explains it too.
- The one file the arithmetic missed, `chcr3b`, was absorbed by asserting that its extra input
  bitmap "carried a 16-byte hotspot array rather than this file's usual 32". That was a property
  claimed of a bitmap that is not in the archive: an epicycle protecting the hypothesis from its
  own counterexample. It has been deleted.

The failure that matters is not the wrong hypothesis. It is that an inference was written down as
**Observed**, which is the evidence class this repository reserves for measurement, and that is what
let it stand as settled for as long as it did.

**Observed in a local binary — the archive's `.h` members are not reliably their own.** Of the
1,800 generated headers, **602 declare a sequence name other than their stem**, and **388 of them
fall into 115 groups of byte-identical files** (17 `missile\*.h` members are one shared file
declaring `spl01ap`; 21 share another). A `.h` in this archive is a build artefact that was freely copied, so it cannot be assumed
to describe the `.imp` beside it. That reframes the remaining five disagreements:

- **`missile\lsp01ap`** — the header's 540,672 raw bytes is exactly `33 * 128 * 128`, a uniform
  uncropped canvas, but the file's maximum frame size is 60x83 and its 33 frames are cropped. The
  structure otherwise agrees exactly. `missile\lsp01apa.h` declares the same sequence `lsp01ap` and
  validates at 31,804. Inferred: the header predates the crop pass.
- **`units\imp\lifitam`, `units\imp\lifitbm`, `units\imp\lifitfm`** — structure agrees exactly
  (2 sequences, 35 frames, 0 duplicates, 0 hotspot bytes); only the pixel totals differ.
  `lifitfm.h` is byte-identical to `lifitam.h` and declares sequence `LIFITAM` (**Observed**), and
  the two `.imp` members measure identically, so `lifitfm` fails exactly as `lifitam` does. Inferred:
  the art was revised without regenerating the `.h`.
- **`units\imp\orcr4b`** — the single pair in the archive where the file holds *fewer* duplicates
  than the header claims (0 against 50), and the only one whose frame count disagrees (92 against
  86). Its structure matches its sibling `units\imp\orcr4a` exactly — 7 sequences, 92 frames, 0
  duplicates, 1,472 hotspot bytes — while the header's 86/50/576 signature matches **no** member in
  the archive. Inferred: the art was rebuilt from the `orcr4a` source and the `.h` was never
  regenerated.

**Not proven.** No `.imp` in the archive measures 170,700, 49,014, 540,672 or the `orcr4b` header's
22,957 raw bytes, so no member can be pointed at as the true owner of any of those four statistics.
"The header is stale" remains an inference from structural agreement, not a demonstrated copy —
except for `lifitfm`, where the byte-identical header is direct evidence.

**Corrected 2026-09-17 — the fallback now requires a unique match.** A declared sequence name is
not a key: the 1,800 headers declare only **1,370 distinct sequence names**, 155 names are declared
by more than one header, those 155 cover 585 headers, and `deaura` alone is declared by 32
(Observed in a local binary). The fallback used to take the first candidate it found, and the same
was true of the sprite-basename index, which was last-wins over 5 colliding `.imp` basenames. Both
now keep every candidate, prefer one in the member's own directory, and report ambiguity as a
failure rather than resolving it. `ambiguous_pairings` is 0 on the shipped archive — the two pairs
we make are each uniquely supported — so the result is unchanged and the method is no longer
arbitrary where it happens not to matter.

**Observed in a local binary — the four orphans are naming artefacts, not archive gaps.** The
archive holds 3,600 members and `1798 * 2 + 4 == 3600`, so nothing is missing. Consulting the
header's declared sequence name resolves two of the four, and both then validate on every statistic:

| Orphan | Resolution |
| --- | --- |
| `imp\fleemark.h` | declares `UNMRKA`; validates against `imp\unmrka.imp` |
| `units\imp\dewmhb.imp` | named by `units\imp\chwmcbm.h`, which declares `DEWMHB` |

Both fallback pairs reuse a partner that already has a stem pair of its own: `imp\unmrka.imp` also
pairs with `imp\unmrka.h`, and `units\imp\chwmcbm.h` also pairs with `units\imp\chwmcbm.imp`. The
fallback did not find these two an unclaimed partner, because there is none — it identified which
*existing* member each one describes. That is why `matched_pairs` counts 1,800 while the archive
holds 3,600 members: every member is still accounted for exactly once as a stem pair, a fallback
pair, or an orphan, and two members serve in two pairs each. The near-duplicate art makes this
consistent rather than contradictory — `chwmcbm.imp` and `dewmhb.imp` measure identically, as do
`unmrka.imp` and `fleemarka.imp` — so one header describes both members of each pair truthfully.

The other two cannot pair and carry catalog notes in `IMP_ORPHAN_NOTES` instead. `aura\lsp01ea.h`
declares `SPL01EA`, which has no `.imp` in the archive, and is byte-identical to `aura\fsp03aa.h`
whose `.imp` matches its statistics exactly — a stray header copy. `imp\fleemarka.imp` measures
identically to `imp\unmrka.imp` (1 sequence, 13 frames, 0 duplicates, 27,054 raw, uncompressed) and
no header declares sequence `FLEEMARKA` — an art copy shipped without a header.

**Refuted (retained).** Because the community specification defines only frame types `0x00` and
`0x08`, while we additionally fold our `0x04` shared-pixel frames into the same counter, it looked
plausible that "Duplicate bitmaps found" counts only true `0x08` back-references. Validating against
a `0x08`-only count raises corpus failures from 0 to **107** (re-measured 2026-09-17 after the
frame-table fix; before the fix the same substitution read 10 to 112). That statistic therefore counts both
flags and our existing conflation is correct. `ImpSprite::back_reference_frame_count` retains the
separate `0x08` tally for analysis.

The native viewer displays individual frames, follows duplicate/repeated references, navigates within a facing or between facings and actions, and can autoplay the current facing at a fixed scale. Representative 8-bit unit art is recognizable, which strongly supports the byte-level decoder. In one creature frame, index 0 fills the background while a distinct second index forms a 1,651-pixel silhouette beneath the creature; an inspected 1-bit aura asset similarly uses those two colours alone. Index 0 is pure **red** and index 1 pure **green** (corrected 2026-09-17; the earlier naming came through a decoder that swapped the two). This is evidence for separate background and mask/compositing channels, not a single universal chroma key. The viewer therefore offers clean-preview, mask, and raw-palette modes.

A community specification located on 2026-09-16 states the rule directly: **the transparency index is
a header field and palette index 1 is the shadow**, keyed by index rather than by colour.

**Header byte 3 is that colour key, and we were not reading it.** In a 300-file sample from the GS5R3
`imp.mpq`, 94 files carry a nonzero colour key. On those sprites palette slot 0 is typically green
but **never appears in the pixel data at all**, while the colour-key index is the most common value
in the frame — it is the background. Keying transparency on a hardcoded 0 therefore rendered roughly
a third of the corpus with an opaque background and masked nothing. The parser now exposes
`ImpSprite::color_key`, and both the viewer and the PNG export honour it. Validation figures are
unchanged, because this affects presentation rather than structural decoding. Our own
observation of a silhouette *beneath* a creature independently corroborates "shadow" — better called
**translucency**, since it is measured as a 50% blend. The RGB in those slots is ignored by the
engine, proved by rewriting index 1 to magenta and seeing no change, which is why colour-keying
never generalised. `secondary_mask` is renamed `shadow` in the viewer accordingly. The same source
names what this project originally called a "cycle" a **facing**, ordered clockwise. The clockwise
claim is untested, but the naming is better than ours and has been adopted throughout the code, the
docs, and the `--describe-imp` output, where sequence rows now cross-reference `facing:N` rather than
`cycle:N`. See [community research](community-research.md). Exact mask meaning and animation timing still need comparison against the original executable; placement semantics and the hotspot type vocabulary were settled on 2026-09-16 (see [hotspots](hotspots.md)); the decoder preserves all source palette indices and colors unchanged.

The CLI can export any resolved logical frame as an 8-bit indexed PNG. Its synthetic decode-back test verifies exact palette bytes and palette-index pixels, and a real GS5R3 export was independently identified as a 165×127 indexed PNG. Exports now carry a `tRNS` chunk marking the header's colour-key index transparent; before 2026-09-16 every exported frame was fully opaque, silently losing the transparency key. Export uses create-new semantics so it cannot silently replace an existing file. This is a lossless inspection format. Placement fields can now be written back into a loose IMP with `--set-imp-placement`; full IMP reimport and archive writing remain unimplemented.

## Evidence and confidence

- **Observed:** all 9,804 core members are readable and classified; all 1,377 PBMs and 1,800 IMP binaries pass their bounded decoders; every IMP exposes bounded sequence/facing/frame ranges; representative 8-bit IMP frames are visually recognizable; red and green occupy distinct palette indices/masks in inspected sprites.
- **Inferred:** IMP file-flag depth bits select 1/2/4/8-bit packing, sub-byte pixels are most-significant-bit first, common five-facing action groups represent directions, which a community tool attributes to five stored facings plus engine mirroring. These interpretations explain the corpus and visible output but are not yet an original-engine specification.
- **Observed:** header byte 3 is a transparency colour key, nonzero in 94 of 300 sampled files; on those files palette index 0 is absent from the pixel data entirely.
- **Observed:** across the full 1,800-member corpus the file types are 9 (8-bit RLE, 957), 8 (8-bit raw, 606), 57 (4-bit RLE, 188), 25 (1-bit RLE, 27), 10 unclassified, and 12 that crash the community parser. An independent community decoder reaches 100% of non-duplicate frames on types 8, 9, and 25 — exact agreement with ours — but 0% on type 57 and on the 12 crash cases, for 91.6% overall against our 100%. Type 57 alone is 188 files and 3,388 frames that no public tool decodes.
- **Documented:** a community specification agrees with our header offsets, record sizes, and RLE algorithm exactly, including the `control + 3` bias; its claim that the palette is stored BGRA and swapped to RGB is **refuted** — entries are stored blue, red, green, pad, measured in the running engine on 2026-09-17, and our implementation agreed with the claim and was therefore also wrong; it names palette index 1 the shadow and our facings facings. Its guesses at a per-frame delay byte and a checksum dword are refuted by our hotspot decoding, which matches generated-header ground truth for all 1,800 pairs.
- **Documented, since corrected:** the hotspot ID is a type tag; the vocabulary is nine values 0-8, not 19 names (see above), and frame byte `+1` is a **count** of hotspot records rather than a type tag — settled by ozz on the board and confirmed here by measurement. Sequence-record byte 1 is a mirror flag: values `>= 128` mirror, and no unmirrored sequence in the corpus has more than two facings. See [community research](community-research.md#the-hotspot-mechanism-thread-2176).
- **Observed:** the engine draws a frame at `top_left = anchor + placement - (width >> 1, height >> 1)` — the stored pair is the vector from the anchor to the **centre** of the frame, in screen pixels with `+y` down, and it is **added**. Measured in the running engine on 2026-09-16; see [hotspots](hotspots.md).
- **Unknown:** how the shadow index is blended or recolored, what the remaining sequence/facing metadata fields mean, and whether exceptional metadata cases use additional sharing rules. Animation timing appears to be carried solely by duplicate-frame repetition, since no delay field survives scrutiny on either side.

### The draw placement (record 0) is not derivable from frame geometry

**Naming corrected 2026-09-16:** this section measured hotspot type **0**, which is `NO_HOTSPOT` and is the engine-reserved **draw placement**. `CURSOR_HOTSPOT` is type 1. The measurement stands; only the label was wrong, and its subject turns out to be the more important one.

Record 0, tagged `NO_HOTSPOT`, is present on essentially every unit frame and is described on the modding
board as the anchor the other hotspots hang from. Whether it can be *derived* decides whether a
rebuilt sprite can ever be correct, because the community's standing workaround for the
"512x512 hotspot" problem is to crop each frame to its minimum extent and re-centre the frames
against each other — which assumes the anchor is a function of frame size.

Measured with `tools/hotspot_geometry.py` over all 28,447 unit frames that carry a record 0,
fitting each axis against the matching frame dimension:

| Axis | Fit | Raw spread | Spread left after the fit |
| --- | --- | ---: | ---: |
| x | `-0.021 x width + 0.99`, median 0 | 8.87 px | **8.84 px** |
| y | `-0.298 x height - 1.67` | 14.55 px | **10.03 px** |

**Horizontally the anchor is independent of frame width** — the slope is effectively zero and the
median is exactly 0, so sprites are centred on the anchor by convention, and the 8.8 px of spread is
per-frame art, not geometry.

**Vertically the fit explains about half the variance and leaves 10 px standing.** Frame height
predicts roughly a third of the hotspot's y, which is what you would expect from taller sprites
having their feet further down, but the residual is far too large for the anchor to be a function of
the frame box.

So the draw placement is **per-frame authored data**: where the artist put that sprite's feet in that
pose. That is the mechanical reason the board's crop-and-re-centre approach kept producing wobble and
a drifting health bar and never converged — it reconstructs a value that is not reconstructible from
the cropped image. It also explains why `lomut` omitting the hotspot array is unrecoverable rather
than merely inconvenient: the information is gone, not mislaid.

The missile/spell-target hotspot (type 7) sits `(+2.0, -10.7)` from the type-0 draw placement on average across
28,159 frames, so projectiles are aimed at the body rather than the feet.

### LBM export

`--export-pbm ARCHIVE MEMBER OUTPUT.png` writes an LBM out as an indexed PNG, preserving palette
indices and the 256-entry palette exactly. Transparency follows the file's own BMHD: a `tRNS` chunk
is written only when `masking` is 2, the IFF value for "has a transparent colour", keyed on the
declared `transparent_color` index.

**Measured across all 1,044 LBM members of the installed archives: every one has `masking = 0`.**
No shipped LBM declares a transparent colour, so the `tRNS` path never fires on real data and is
covered by synthetic tests only. Recording that here because the tempting "fix" for an LBM that
looks opaque is to key transparency on index 0 — which is exactly the bug that was removed from IMP
export, where the key is a header field and is nonzero in 94 of 300 sampled sprites.

## Map/scenario findings

The loose installed map corpus contains 20 `.scn`, 337 `.smp`, and eight `.lgd` files. All 365 pass the bounded parser. Each file declares width, height, an observed depth of 8, and one eight-byte record per cell in packed y-major order (`y × width + x`, corrected 2026-09-17 from X-major). Treating the second word as little-endian `f32` yields finite values from 0 to 20 and coherent relief. The first word resolves to an original tile-atlas index — confirmed by forcing seven slots in the running engine on 2026-09-17 — plus a `0x00800000` flag whose meaning is Unknown (the forced-texture reading was refuted by the same run); `URAK.scn` renders as a coherent world through `tilesb01.til` and `tilesb01.lbm`, which shows the masked tag indexes real terrain art — but says nothing about orientation, since a transposed world map is equally coherent. The render transposes relative to every preview exported before 2026-09-17.

All 26 recovered `.til` definitions parse and explicitly bind atlas geometry, 32×32 tile dimensions, terrain types, and tile indices. The trailing record section is also decoded structurally, in **all 365 files**: 21,117 records across six layouts — 47, 48, 49, 52 and 53 bytes, with a footer in two of them. Cell indices are bounded and unique per file, instance ids are unique and increasing per file, and candidate instance, sprite-type, and procedure fields are exposed without discarding raw bytes. **Corrected 2026-09-17:** this previously described three families plus 18 unmatched tails; there are six layouts and nothing unmatched. Exact object-field behavior remains open, and every tail word outside the head is constant within its layout, so the corpus cannot say what it means. See the [map-format record](map-format.md) and [issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4).

## Latest verification

Verified on 2026-09-16 (the full corpus scan itself dates from 2026-09-12):

- 83 Rust library tests and 11 CLI/viewer tests pass;
- strict Clippy (`-D warnings`) passes for all targets;
- all 21 repository Python tests pass;
- a fresh read-only scan classifies all five GS5R3 core archives with zero probe failures;
- all 1,800 IMP payloads decode with bounded sequence/facing ranges;
- exact IMP/header validation matches 1,795 of 1,800 pairs; the five remaining paired disagreements and two orphan names are value-pinned catalog entries, and anything else re-fails;
- all 365 loose map/scenario/component files pass; all 365 resolve to one of six record layouts and decode 21,117 bounded records;
- all 26 tile-set definitions parse, and a real `URAK.scn` terrain preview exports as a coherent 1024×1024 RGBA PNG (coherent, not *correctly oriented* — a transposed world map is equally coherent, and the orientation is unverified);
- indexed-PNG export preserves synthetic palette indices and palette bytes, succeeds on a real 165×127 frame, and refuses overwrite.

## Test strategy and gates

The implementation uses three layers of evidence:

1. Tiny synthetic unit fixtures cover endianness, chunk bounds, ByteRun1 scanline behavior, IMP tables, sequence/facing ranges, navigation boundaries, RLE packets, all four packed pixel depths, hotspots, duplicate references, shared-pixel records inside a record array, indexed-PNG preservation, BMP metadata, WAVE chunks, and the platform-dependent StormLib enumeration ABI.
2. User-local corpus tests scan all five archives, require every member to be readable/classifiable, and cross-check IMP binaries against their generated headers. No copyrighted fixture enters Git.
3. Visual comparison checks representative UI, portrait, map, and animation output against the original executable before renderer behavior is considered faithful.

Stage 1 can pass only when common assets round-trip losslessly, unknown variants are bounded and documented, and representative rendering agrees on palette, transparency, coordinates, and frame sequencing.

## Completed in this milestone

- [x] Read-only MPQ access behind a narrow Rust wrapper.
- [x] External listfile ingestion and reproducible hash-pinned fetch.
- [x] List, catalog, scan, inspect, and non-overwriting extract commands.
- [x] Content-first classification of all five core archives.
- [x] Lossless decode of all 1,377 observed PBM images.
- [x] Metadata probes for BMP and WAVE.
- [x] Bounds-checked structural parser for all 1,800 IMP binaries.
- [x] Pixel decoding for both IMP record variants and all observed packed depths.
- [x] Individual-frame viewer with duplicate resolution and animation controls.
- [x] Named action and facing navigation with facing-scoped playback.
- [x] Non-overwriting, indexed-PNG export for individual logical frames.
- [x] Generated-header parser and corpus cross-validator.
- [x] Typed IMP origin/hotspot candidates and shared-pixel records inside facings.
- [x] Bounded header/cell-grid parser and diagnostic elevation viewer for all 365 loose map files.
- [x] Decode packed map coordinates, standard tile-atlas indices, and the `0x00800000` tag flag (meaning still Unknown).
- [x] Parse all 26 recovered `.til` definitions and render/export original-art terrain overviews.
- [x] Structurally decode and validate all 16,628 records in the dominant 49-byte placed-sprite family.
- [x] Structurally decode the other five record layouts — 47, 48, 52 and 53 bytes — bringing the corpus to 21,117 records in 365 of 365 files, and object editing to all 365 maps.

## Remaining before Stage 1 is complete

- [ ] Resolve the five known IMP metadata mismatches and two catalog-name orphans ([issue #3](https://github.com/jake-bliss/lords-of-magic-modding/issues/3)).
- [x] Establish palette, chroma-key, hotspot, and placement semantics ([issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1), closed — see [hotspots](hotspots.md)).
- [ ] Measure the shadow-index blend and resolve the palette channel order, the two compositing questions issue #1 left behind.
- [ ] Verify IMP direction and timing metadata ([issue #2](https://github.com/jake-bliss/lords-of-magic-modding/issues/2)).
- [ ] Prove the candidate object-field semantics, and find out what the six layouts' constant tail words mean — which this corpus cannot answer, because each is constant within its layout ([issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4)).
- [ ] Inventory loose WAVE/Smacker resources outside the core archives.
- [ ] Add batch export, full IMP reimport, and deterministic game-format round-trip tests. Placement write-back is done.
- [ ] Add searchable browsing, cached textures, animation controls, and export to the GUI.
- [ ] Make native-library discovery and packaging portable across macOS, Windows, and Linux.

The controlled Map Editor save diff remains parked in issue #4 after macOS accessibility controls prevented reliable Wine-window automation. The parallel GameScript track now has a complete lexical/vocabulary scan and a first stack/dictionary interpreter checkpoint; its next bounded slice is read-only module loading and host-call classification. The original-engine-only IMP presentation work stays parked in issues #2–#4 and #22 rather than being encoded as assumptions.
