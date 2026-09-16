# Native Asset Layer — Stage 1

## Status

**In progress; archive inventory, IMP inspection/export, terrain-atlas rendering, and the dominant map-object record milestone are complete.** The native Rust tool can read the five core GS5R3 archives without modifying them, recover their public filenames, classify all 9,804 members, probe common standard formats, fully decode the observed PBM image corpus, and decode the pixels, frame references, sequences, facings, origins, and hotspot records of all 1,800 IMP sprite binaries. It also bounds all 365 installed `.scn`, `.smp`, and `.lgd` files, resolves standard map cells through original terrain art, and structurally decodes 16,628 placed-sprite records.

This is useful tooling now, but it is not yet the lossless asset layer promised by Stage 1. Full map/scenario semantics, verified compositing/origin behavior, reimport/repacking, and cross-platform packaging remain open.

## Component boundaries

| Component | Responsibility | Must not own |
| --- | --- | --- |
| MPQ adapter | Read-only archive open, enumeration, external listfile loading, member reads | Asset interpretation or archive writes |
| Format decoders | Bounds-checked parsing of PBM, IMP, BMP, and WAVE structures | Game-specific compositing or simulation |
| Asset probe | Content-first classification and typed metadata | Rendering state |
| CLI | Inventory, catalog, inspect, extract, indexed-frame export, validation, and viewer entry points | Format parsing logic |
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
- a 256-entry BGRA palette;
- maximum dimensions, sequences, facings, logical frames, and frame dimensions;
- explicit sequence-to-facing and facing-to-frame ranges, retained with their still-unknown raw metadata fields;
- action labels recovered from generated-header `#define` values cover 4,649 of 4,666 declared sequence slots; aliases are retained, and 1,799 of 1,800 headers provide at least one label;
- six-byte hotspot records padded per frame to an eight-byte boundary;
- direct duplicate-frame references and a compact repeated-facing representation;
- shared-pixel flag `0x04`, including records that occur inside rather than only at the start of a repeated facing;
- two observed frame-record variants, one with explicit stored sizes and one whose payload length is implicit;
- a custom packet RLE in which controls below `0x80` repeat the following byte `control + 3` times and controls at or above `0x80` copy `256 - control` literal bytes;
- 8-, 4-, 2-, and 1-bit indexed pixels selected by file-flag bits `0x30`, with both tightly packed and row-padded layouts;
- most-significant-bit-first packing for sub-byte pixels, which produces recognizable output across representative sprites;
- complete pixel expansion, frame-reference resolution, and sequence/facing traversal for all 1,800 observed binaries.

The shared-pixel correction removed 27 false origin records, bounded the remaining origin ranges to X `-66..70` and Y `-207..77`, and improved exact generated-header matches. Across the corpus, 15,725 logical frames carry signed origins and 28,771 carry 64,432 six-byte hotspot records. The hotspot bytes consistently decode as a candidate unsigned ID followed by signed X/Y offsets, with observed coordinate ranges X `-115..123` and Y `-232..86`.

The ID is now identified: it is a **hotspot type**, drawn from the 19 `*_HOTSPOT` constants in `lomse.exe`'s string table (`CURSOR_HOTSPOT`, `MISSILE_ORIGIN_HOTSPOT`, `SPELL_ORIGIN1..4_HOTSPOT`, `MISSILE_TARGET_HOTSPOT`, `SPELL_TARGET_HOTSPOT`, `FLAP_OFFSET_HOTSPOT`, `STREAMER_HOTSPOT`, `MISSILE_HOTSPOT`, `BOLT_HOTSPOT_D0..D3`, `BOLT_HOTSPOT_S0..S3`, plus `NO_HOTSPOT`). Types 0 and 7 occur on nearly every unit frame (28,661 and 28,183 occurrences); record counts per frame run from 2 to 9. The engine also exports the natives `getimphotspot` and `enumimphotspots`. Two files carry types outside that vocabulary — see [community research](community-research.md#the-hotspot-mechanism-thread-2176). Placement semantics remain a reference-comparison task in [issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1).

For example, `units\imp\chcr5a.imp` contains seven named actions (`MOVE`, `STAND`, `DEFEND`, `GET_HIT`, `DIE`, `CORPSE`, and `MELEE_ATTACK`), five facings per action, and 170 logical frames. The five facings are likely directional views, but that interpretation and the remaining sequence/facing metadata have not yet been confirmed against the original executable.

The validator pairs generated headers with binaries and compares independently recorded sequence, frame, duplicate, raw-pixel, hotspot, and stored-pixel statistics where applicable:

| IMP validation check | Result |
| --- | ---: |
| Same-stem header/binary pairs | 1,798 |
| Exact structural matches | 1,788 (99.4%) |
| Bounded metadata mismatches | 10 |
| Orphan catalog entries | 4 |

All stored-pixel byte totals now agree with the generated headers. The remaining paired mismatches concern duplicate-frame counts, raw logical-pixel totals, and one logical-frame count; they remain failing validation cases until understood. Four public-catalog stems have only one member of the expected `.imp`/`.h` pair.

| Remaining disagreement | Count |
| --- | ---: |
| Generated duplicate-frame count | 5 |
| Generated raw logical-pixel total | 4 |
| Generated logical-frame count | 1 |
| Missing expected `.imp`/`.h` counterpart | 4 |

One hypothesis for the five duplicate-frame disagreements has been **tested and refuted**. Because the
community specification defines only frame types `0x00` and `0x08`, while we additionally fold our
`0x04` shared-pixel frames into the same counter, it looked plausible that the generated header's
"Duplicate bitmaps found" statistic counts only true `0x08` back-references. Validating against a
`0x08`-only count raises corpus failures from 10 to **112**. That statistic therefore counts both
flags and our existing conflation is correct. `ImpSprite::back_reference_frame_count` retains the
separate `0x08` tally for analysis. The five disagreements have another cause; the repeated-facing
heuristic remains the leading suspect for the logical-frame and raw-pixel cases.

The native viewer displays individual frames, follows duplicate/repeated references, navigates within a facing or between facings and actions, and can autoplay the current facing at a fixed scale. Representative 8-bit unit art is recognizable, which strongly supports the byte-level decoder. In one creature frame, green index 0 fills the background while a distinct pure-red index forms a 1,651-pixel silhouette beneath the creature; an inspected 1-bit aura asset similarly uses green and red as its only two colors. This is evidence for separate background and mask/compositing channels, not a single universal chroma key. The viewer therefore offers clean-preview, mask, and raw-palette modes.

A community specification located on 2026-09-16 states the rule directly: **the transparency index is
a header field and palette index 1 is the shadow**, keyed by index rather than by colour.

**Header byte 3 is that colour key, and we were not reading it.** In a 300-file sample from the GS5R3
`imp.mpq`, 94 files carry a nonzero colour key. On those sprites palette slot 0 is typically green
but **never appears in the pixel data at all**, while the colour-key index is the most common value
in the frame — it is the background. Keying transparency on a hardcoded 0 therefore rendered roughly
a third of the corpus with an opaque background and masked nothing. The parser now exposes
`ImpSprite::color_key`, and both the viewer and the PNG export honour it. Validation figures are
unchanged, because this affects presentation rather than structural decoding. Our own
observation of a pure-red silhouette *beneath* a creature independently corroborates "shadow". The
green and red RGB values in those slots are incidental art-tool choices, which is why colour-keying
never generalised. `secondary_mask` is renamed `shadow` in the viewer accordingly. The same source
names what this project originally called a "cycle" a **facing**, ordered clockwise. The clockwise
claim is untested, but the naming is better than ours and has been adopted throughout the code, the
docs, and the `--describe-imp` output, where sequence rows now cross-reference `facing:N` rather than
`cycle:N`. See [community research](community-research.md). Exact mask meaning, origins, hotspot meaning, and animation timing still need comparison against the original executable; the decoder preserves all source palette indices and colors unchanged.

The CLI can export any resolved logical frame as an 8-bit indexed PNG. Its synthetic decode-back test verifies exact palette bytes and palette-index pixels, and a real GS5R3 export was independently identified as a 165×127 indexed PNG. Exports now carry a `tRNS` chunk marking the header's colour-key index transparent; before 2026-09-16 every exported frame was fully opaque, silently losing the transparency key. Export uses create-new semantics so it cannot silently replace an existing file. This is a lossless inspection format, not yet a game-compatible IMP reimport or archive-writing pipeline.

## Evidence and confidence

- **Observed:** all 9,804 core members are readable and classified; all 1,377 PBMs and 1,800 IMP binaries pass their bounded decoders; every IMP exposes bounded sequence/facing/frame ranges; representative 8-bit IMP frames are visually recognizable; red and green occupy distinct palette indices/masks in inspected sprites.
- **Inferred:** IMP file-flag depth bits select 1/2/4/8-bit packing, sub-byte pixels are most-significant-bit first, common five-facing action groups represent directions, which a community tool attributes to five stored facings plus engine mirroring. These interpretations explain the corpus and visible output but are not yet an original-engine specification.
- **Observed:** header byte 3 is a transparency colour key, nonzero in 94 of 300 sampled files; on those files palette index 0 is absent from the pixel data entirely.
- **Observed:** across the full 1,800-member corpus the file types are 9 (8-bit RLE, 957), 8 (8-bit raw, 606), 57 (4-bit RLE, 188), 25 (1-bit RLE, 27), 10 unclassified, and 12 that crash the community parser. An independent community decoder reaches 100% of non-duplicate frames on types 8, 9, and 25 — exact agreement with ours — but 0% on type 57 and on the 12 crash cases, for 91.6% overall against our 100%. Type 57 alone is 188 files and 3,388 frames that no public tool decodes.
- **Documented:** a community specification agrees with our header offsets, record sizes, and RLE algorithm exactly, including the `control + 3` bias; it confirms the palette is stored BGRA and swapped to RGB, which resolves our open channel-order question in favour of the current implementation; it names palette index 1 the shadow and our facings facings. Its guesses at a per-frame delay byte and a checksum dword are refuted by our hotspot decoding, which matches generated-header ground truth for all 1,798 pairs.
- **Documented:** the hotspot ID is a type tag from a 19-constant engine vocabulary, and frame byte `+1` is a **count** of hotspot records rather than a type tag — settled by ozz on the board and confirmed here by measurement. Sequence-record byte 1 is a mirror flag: values `>= 128` mirror, and no unmirrored sequence in the corpus has more than two facings. See [community research](community-research.md#the-hotspot-mechanism-thread-2176).
- **Unknown:** how the shadow index is blended or recolored, how hotspot coordinates translate to screen placement, what the remaining sequence/facing metadata fields mean, and whether exceptional metadata cases use additional sharing rules. Animation timing appears to be carried solely by duplicate-frame repetition, since no delay field survives scrutiny on either side.

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

The loose installed map corpus contains 20 `.scn`, 337 `.smp`, and eight `.lgd` files. All 365 pass the bounded parser. Each file declares width, height, an observed depth of 8, and one eight-byte record per cell in X-major order. Treating the second word as little-endian `f32` yields finite values from 0 to 20 and coherent relief. The first word resolves to an original tile-atlas index plus an observed `0x00800000` forced-texture flag; `URAK.scn` now renders as a coherent, correctly oriented world through `tilesb01.til` and `tilesb01.lbm`.

All 26 recovered `.til` definitions parse and explicitly bind atlas geometry, 32×32 tile dimensions, terrain types, and tile indices. The dominant trailing family is also decoded structurally: 196 files contain 16,628 exact 49-byte placed-sprite records. Cell indices are bounded and unique per file, and candidate instance, sprite-type, and procedure fields are exposed without discarding raw bytes. The 52-/53-byte families and exact object-field behavior remain open. See the [map-format record](map-format.md) and [issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4).

## Latest verification

Verified on 2026-09-12:

- 29 Rust library tests and four viewer tests pass;
- strict Clippy (`-D warnings`) passes for all targets;
- all three repository Python tests pass;
- a fresh read-only scan classifies all five GS5R3 core archives with zero probe failures;
- all 1,800 IMP payloads decode with bounded sequence/facing ranges;
- exact IMP/header validation matches 1,788 of 1,798 pairs; the ten remaining paired disagreements and four orphan names remain intentionally reported;
- all 365 loose map/scenario/component files pass; all 196 exact 49-byte-family files decode 16,628 bounded records;
- all 26 tile-set definitions parse, and a real `URAK.scn` terrain preview exports as a correctly oriented 1024×1024 RGBA PNG;
- indexed-PNG export preserves synthetic palette indices and palette bytes, succeeds on a real 165×127 frame, and refuses overwrite.

## Test strategy and gates

The implementation uses three layers of evidence:

1. Tiny synthetic unit fixtures cover endianness, chunk bounds, ByteRun1 scanline behavior, IMP tables, sequence/facing ranges, navigation boundaries, RLE packets, all four packed pixel depths, hotspots, duplicate references, repeated facings, indexed-PNG preservation, BMP metadata, WAVE chunks, and the platform-dependent StormLib enumeration ABI.
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
- [x] Decode X-major map coordinates, standard tile-atlas indices, and the forced-texture tag candidate.
- [x] Parse all 26 recovered `.til` definitions and render/export original-art terrain overviews.
- [x] Structurally decode and validate all 16,628 records in the dominant 49-byte placed-sprite family.

## Remaining before Stage 1 is complete

- [ ] Resolve the ten known IMP metadata mismatches and four catalog-name orphans ([issue #3](https://github.com/jake-bliss/lords-of-magic-modding/issues/3)).
- [ ] Establish palette, chroma-key, hotspot, pivot, and compositing semantics ([issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1)).
- [ ] Verify IMP direction and timing metadata ([issue #2](https://github.com/jake-bliss/lords-of-magic-modding/issues/2)).
- [ ] Decode the remaining 52-/53-byte and unknown map tails; prove the candidate 49-byte object-field semantics ([issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4)).
- [ ] Inventory loose WAVE/Smacker resources outside the core archives.
- [ ] Add batch export, IMP reimport, and deterministic game-format round-trip tests.
- [ ] Add searchable browsing, cached textures, animation controls, and export to the GUI.
- [ ] Make native-library discovery and packaging portable across macOS, Windows, and Linux.

The controlled Map Editor save diff remains parked in issue #4 after macOS accessibility controls prevented reliable Wine-window automation. The parallel GameScript track now has a complete lexical/vocabulary scan and a first stack/dictionary interpreter checkpoint; its next bounded slice is read-only module loading and host-call classification. The remaining 52-/53-byte tails and original-engine-only IMP presentation work stay parked in issues #1–#4 rather than being encoded as assumptions.
