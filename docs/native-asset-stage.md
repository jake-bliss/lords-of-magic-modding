# Native Asset Layer — Stage 1

## Status

**In progress; archive inventory, IMP inspection/export, terrain-atlas rendering, and the dominant map-object record milestone are complete.** The native Rust tool can read the five core GS5R3 archives without modifying them, recover their public filenames, classify all 9,804 members, probe common standard formats, fully decode the observed PBM image corpus, and decode the pixels, frame references, sequences, cycles, origins, and hotspot records of all 1,800 IMP sprite binaries. It also bounds all 365 installed `.scn`, `.smp`, and `.lgd` files, resolves standard map cells through original terrain art, and structurally decodes 16,628 placed-sprite records.

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
- 16-byte sequence, 8-byte cycle, and 16-byte frame records;
- a 256-entry BGRA palette;
- maximum dimensions, sequences, cycles, logical frames, and frame dimensions;
- explicit sequence-to-cycle and cycle-to-frame ranges, retained with their still-unknown raw metadata fields;
- action labels recovered from generated-header `#define` values cover 4,649 of 4,666 declared sequence slots; aliases are retained, and 1,799 of 1,800 headers provide at least one label;
- six-byte hotspot records padded per frame to an eight-byte boundary;
- direct duplicate-frame references and a compact repeated-cycle representation;
- shared-pixel flag `0x04`, including records that occur inside rather than only at the start of a repeated cycle;
- two observed frame-record variants, one with explicit stored sizes and one whose payload length is implicit;
- a custom packet RLE in which controls below `0x80` repeat the following byte `control + 3` times and controls at or above `0x80` copy `256 - control` literal bytes;
- 8-, 4-, 2-, and 1-bit indexed pixels selected by file-flag bits `0x30`, with both tightly packed and row-padded layouts;
- most-significant-bit-first packing for sub-byte pixels, which produces recognizable output across representative sprites;
- complete pixel expansion, frame-reference resolution, and sequence/cycle traversal for all 1,800 observed binaries.

The shared-pixel correction removed 27 false origin records, bounded the remaining origin ranges to X `-66..70` and Y `-207..77`, and improved exact generated-header matches. Across the corpus, 15,725 logical frames carry signed origins and 28,771 carry 64,432 six-byte hotspot records. The hotspot bytes consistently decode as a candidate unsigned ID followed by signed X/Y offsets, with observed coordinate ranges X `-115..123` and Y `-232..86`. Exact coordinate and ID behavior remains a reference-comparison task in [issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1).

For example, `units\imp\chcr5a.imp` contains seven named actions (`MOVE`, `STAND`, `DEFEND`, `GET_HIT`, `DIE`, `CORPSE`, and `MELEE_ATTACK`), five cycles per action, and 170 logical frames. The five cycles are likely directional views, but that interpretation and the remaining sequence/cycle metadata have not yet been confirmed against the original executable.

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

The native viewer displays individual frames, follows duplicate/repeated references, navigates within a cycle or between cycles and actions, and can autoplay the current cycle at a fixed scale. Representative 8-bit unit art is recognizable, which strongly supports the byte-level decoder. In one creature frame, green index 0 fills the background while a distinct pure-red index forms a 1,651-pixel silhouette beneath the creature; an inspected 1-bit aura asset similarly uses green and red as its only two colors. This is evidence for separate background and mask/compositing channels, not a single universal chroma key. The viewer therefore offers clean-preview, mask, and raw-palette modes. Exact mask meaning, origins, hotspot meaning, and animation timing still need comparison against the original executable; the decoder preserves all source palette indices and colors unchanged.

The CLI can export any resolved logical frame as an 8-bit indexed PNG. Its synthetic decode-back test verifies exact palette bytes and palette-index pixels, and a real GS5R3 export was independently identified as a 165×127 indexed PNG. Export uses create-new semantics so it cannot silently replace an existing file. This is a lossless inspection format, not yet a game-compatible IMP reimport or archive-writing pipeline.

## Evidence and confidence

- **Observed:** all 9,804 core members are readable and classified; all 1,377 PBMs and 1,800 IMP binaries pass their bounded decoders; every IMP exposes bounded sequence/cycle/frame ranges; representative 8-bit IMP frames are visually recognizable; red and green occupy distinct palette indices/masks in inspected sprites.
- **Inferred:** IMP file-flag depth bits select 1/2/4/8-bit packing, sub-byte pixels are most-significant-bit first, palette index 0 is the background channel, palette index 1 is a secondary engine mask, and common five-cycle action groups represent directions. These interpretations explain the corpus and visible output but are not yet an original-engine specification.
- **Unknown:** how red masks are blended or recolored, how origins and hotspots affect placement, what the sequence/cycle metadata fields mean, how timing is selected, and whether exceptional metadata cases use additional sharing rules.

## Map/scenario findings

The loose installed map corpus contains 20 `.scn`, 337 `.smp`, and eight `.lgd` files. All 365 pass the bounded parser. Each file declares width, height, an observed depth of 8, and one eight-byte record per cell in X-major order. Treating the second word as little-endian `f32` yields finite values from 0 to 20 and coherent relief. The first word resolves to an original tile-atlas index plus an observed `0x00800000` forced-texture flag; `URAK.scn` now renders as a coherent, correctly oriented world through `tilesb01.til` and `tilesb01.lbm`.

All 26 recovered `.til` definitions parse and explicitly bind atlas geometry, 32×32 tile dimensions, terrain types, and tile indices. The dominant trailing family is also decoded structurally: 196 files contain 16,628 exact 49-byte placed-sprite records. Cell indices are bounded and unique per file, and candidate instance, sprite-type, and procedure fields are exposed without discarding raw bytes. The 52-/53-byte families and exact object-field behavior remain open. See the [map-format record](map-format.md) and [issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4).

## Latest verification

Verified on 2026-09-12:

- 29 Rust library tests and four viewer tests pass;
- strict Clippy (`-D warnings`) passes for all targets;
- all three repository Python tests pass;
- a fresh read-only scan classifies all five GS5R3 core archives with zero probe failures;
- all 1,800 IMP payloads decode with bounded sequence/cycle ranges;
- exact IMP/header validation matches 1,788 of 1,798 pairs; the ten remaining paired disagreements and four orphan names remain intentionally reported;
- all 365 loose map/scenario/component files pass; all 196 exact 49-byte-family files decode 16,628 bounded records;
- all 26 tile-set definitions parse, and a real `URAK.scn` terrain preview exports as a correctly oriented 1024×1024 RGBA PNG;
- indexed-PNG export preserves synthetic palette indices and palette bytes, succeeds on a real 165×127 frame, and refuses overwrite.

## Test strategy and gates

The implementation uses three layers of evidence:

1. Tiny synthetic unit fixtures cover endianness, chunk bounds, ByteRun1 scanline behavior, IMP tables, sequence/cycle ranges, navigation boundaries, RLE packets, all four packed pixel depths, hotspots, duplicate references, repeated cycles, indexed-PNG preservation, BMP metadata, WAVE chunks, and the platform-dependent StormLib enumeration ABI.
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
- [x] Named action and cycle navigation with cycle-scoped playback.
- [x] Non-overwriting, indexed-PNG export for individual logical frames.
- [x] Generated-header parser and corpus cross-validator.
- [x] Typed IMP origin/hotspot candidates and shared-pixel records inside cycles.
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
