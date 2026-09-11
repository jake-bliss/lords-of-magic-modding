# Native Asset Layer — Stage 1

## Status

**In progress; archive inventory and IMP pixel-decoding milestone complete.** The native Rust tool can read the five core GS5R3 archives without modifying them, recover their public filenames, classify all 9,804 members, probe common standard formats, fully decode the observed PBM image corpus, and decode the pixels and frame references of all 1,800 IMP sprite binaries.

This is useful tooling now, but it is not yet the lossless asset layer promised by Stage 1. Maps, scenarios, verified transparency/origin semantics, export, and cross-platform packaging remain open.

## Component boundaries

| Component | Responsibility | Must not own |
| --- | --- | --- |
| MPQ adapter | Read-only archive open, enumeration, external listfile loading, member reads | Asset interpretation or archive writes |
| Format decoders | Bounds-checked parsing of PBM, IMP, BMP, and WAVE structures | Game-specific compositing or simulation |
| Asset probe | Content-first classification and typed metadata | Rendering state |
| CLI | Inventory, catalog, inspect, extract, validation, and viewer entry points | Format parsing logic |
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
| `pic.mpq` | 1,406 | 1,377 PBM, 2 BMP, 26 text, 1 listfile | 0 |
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
- six-byte hotspot records padded per frame to an eight-byte boundary;
- direct duplicate-frame references and a compact repeated-cycle representation;
- two observed frame-record variants, one with explicit stored sizes and one whose payload length is implicit;
- a custom packet RLE in which controls below `0x80` repeat the following byte `control + 3` times and controls at or above `0x80` copy `256 - control` literal bytes;
- 8-, 4-, 2-, and 1-bit indexed pixels selected by file-flag bits `0x30`, with both tightly packed and row-padded layouts;
- most-significant-bit-first packing for sub-byte pixels, which produces recognizable output across representative sprites;
- complete pixel expansion and frame-reference resolution for all 1,800 observed binaries.

The validator pairs generated headers with binaries and compares independently recorded sequence, frame, duplicate, raw-pixel, hotspot, and stored-pixel statistics where applicable:

| IMP validation check | Result |
| --- | ---: |
| Same-stem header/binary pairs | 1,798 |
| Exact structural matches | 1,784 (99.2%) |
| Bounded metadata mismatches | 14 |
| Orphan catalog entries | 4 |

All stored-pixel byte totals now agree with the generated headers. The remaining paired mismatches concern duplicate-frame counts, raw logical-pixel totals, and one logical-frame count; they remain failing validation cases until understood. Four public-catalog stems have only one member of the expected `.imp`/`.h` pair.

| Remaining disagreement | Count |
| --- | ---: |
| Generated duplicate-frame count | 9 |
| Generated raw logical-pixel total | 4 |
| Generated logical-frame count | 1 |
| Missing expected `.imp`/`.h` counterpart | 4 |

The native viewer displays individual frames, follows duplicate/repeated references, and can autoplay them at a fixed scale. Representative 8-bit unit art is recognizable, which strongly supports the byte-level decoder. In one creature frame, green index 0 fills the background while a distinct pure-red index forms a 1,651-pixel silhouette beneath the creature; 1-bit aura assets similarly use green and red as their only two colors. This is evidence for separate background and mask/compositing channels, not a single universal chroma key. The viewer therefore offers clean-preview, mask, and raw-palette modes. Exact mask meaning, origins, hotspot meaning, and animation timing still need comparison against the original executable; the decoder preserves all source palette indices and colors unchanged.

## Test strategy and gates

The implementation uses three layers of evidence:

1. Tiny synthetic unit fixtures cover endianness, chunk bounds, ByteRun1 scanline behavior, IMP tables, RLE packets, all four packed pixel depths, hotspots, duplicate references, repeated cycles, BMP metadata, and WAVE chunks.
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
- [x] Generated-header parser and corpus cross-validator.

## Remaining before Stage 1 is complete

- [ ] Resolve the 14 known IMP metadata mismatches and four catalog-name orphans.
- [ ] Establish palette, chroma-key, hotspot, pivot, and compositing semantics.
- [ ] Parse representative `.scn`, `.smp`, and `.lgd` map/scenario data.
- [ ] Inventory loose WAVE/Smacker resources outside the core archives.
- [ ] Add lossless export and deterministic round-trip tests.
- [ ] Add searchable browsing, cached textures, animation controls, and export to the GUI.
- [ ] Make native-library discovery and packaging portable across macOS, Windows, and Linux.

The next implementation slice should validate IMP chroma keys, origins, hotspots, and sequence timing against the original executable, then add lossless frame export. That will turn the current recognizable rendering into a defensible sprite-format specification.
