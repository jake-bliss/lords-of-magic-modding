# Native Asset Layer — Stage 1

## Status

**In progress; archive discovery and structural inventory milestone complete.** The native Rust tool can read the five core GS5R3 archives without modifying them, recover their public filenames, classify all 9,804 members, probe common standard formats, fully decode the observed PBM image corpus, and parse the table structure of all 1,800 IMP sprite binaries.

This is useful tooling now, but it is not yet the lossless asset layer promised by Stage 1. Sprite pixels, maps, scenarios, transparency semantics, export, and cross-platform packaging remain open.

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
- two observed frame-record variants, one with explicit stored sizes and one whose exact payload length requires decoding.

The validator pairs generated headers with binaries and compares independently recorded sequence, frame, duplicate, raw-pixel, hotspot, and stored-pixel statistics where applicable:

| IMP validation check | Result |
| --- | ---: |
| Same-stem header/binary pairs | 1,798 |
| Exact structural matches | 1,783 (99.2%) |
| Bounded mismatches | 15 |
| Orphan catalog entries | 4 |

The mismatches cluster around additional shared-frame conventions, unusual pixel depth/layout, and one differing logical-frame count. They remain failing validation cases until understood. We do not treat successful bounds-checking of all 1,800 binaries as proof that their pixel encoding is decoded.

## Test strategy and gates

The implementation uses three layers of evidence:

1. Tiny synthetic unit fixtures cover endianness, chunk bounds, ByteRun1 scanline behavior, IMP tables, hotspots, duplicate references, repeated cycles, BMP metadata, and WAVE chunks.
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
- [x] Generated-header parser and corpus cross-validator.

## Remaining before Stage 1 is complete

- [ ] Decode both IMP pixel-storage variants and display individual animation frames.
- [ ] Resolve the 15 known IMP structural mismatches and four catalog-name orphans.
- [ ] Establish palette, chroma-key, hotspot, pivot, and compositing semantics.
- [ ] Parse representative `.scn`, `.smp`, and `.lgd` map/scenario data.
- [ ] Inventory loose WAVE/Smacker resources outside the core archives.
- [ ] Add lossless export and deterministic round-trip tests.
- [ ] Add searchable browsing, cached textures, animation controls, and export to the GUI.
- [ ] Make native-library discovery and packaging portable across macOS, Windows, and Linux.

The next implementation slice is IMP pixel decoding plus a single-frame/animation viewer. It directly attacks the largest remaining image-format uncertainty while preserving value as a standalone modding tool.
