# Rust Asset/MPQ Tool

## Outcome

The initial spike succeeded and is now evolving into the Stage 1 native asset layer. A native 64-bit Rust program opens the installed game's MPQs through a narrow read-only StormLib wrapper, loads an external filename catalog, classifies and inspects members, decodes IFF `FORM PBM` images and IMP sprite frames, probes BMP/WAVE metadata, and displays images and animations through SDL3.

The repository contains no game assets. Commands below require a local, legally obtained installation.

## Prerequisites

The current build targets Apple Silicon Homebrew:

```sh
brew install rust stormlib sdl3
```

Rust can instead be installed with `rustup`. The current `build.rs` expects StormLib and SDL3 under `/opt/homebrew/opt`; portable dependency discovery is tracked as remaining Stage 1 work.

## Build and test

```sh
cd spikes/asset-viewer
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

## Use

Set paths to local archives without copying them into the repository:

```sh
PIC_MPQ='/path/to/Lords of Magic Special Edition/English/pic.mpq'
IMP_MPQ='/path/to/Lords of Magic Special Edition/English/imp.mpq'

cd ../..
scripts/fetch-lom-listfile.sh
LISTFILE="$PWD/artifacts/reference-listfiles/lords-of-magic.txt"
cd spikes/asset-viewer

target/release/lom-asset-viewer --list "$PIC_MPQ"
target/release/lom-asset-viewer --catalog "$PIC_MPQ"
target/release/lom-asset-viewer --scan "$PIC_MPQ"
target/release/lom-asset-viewer --inspect "$PIC_MPQ" 'LBM\ACTIONS5.lbm'
target/release/lom-asset-viewer --extract "$PIC_MPQ" 'LBM\ACTIONS5.lbm' /tmp/actions5.lbm
target/release/lom-asset-viewer --validate-imp "$IMP_MPQ" --listfile "$LISTFILE"
target/release/lom-asset-viewer --view-imp "$IMP_MPQ" 'units\imp\chcr5a.imp' --listfile "$LISTFILE"
target/release/lom-asset-viewer --export-imp-frame "$IMP_MPQ" 'units\imp\chcr5a.imp' 155 /tmp/chcr5a-frame-155.png --listfile "$LISTFILE"
target/release/lom-asset-viewer "$PIC_MPQ" 'LBM\ACTIONS5.lbm'
```

`--extract` and `--export-imp-frame` use create-new semantics and refuse to overwrite an existing output. IMP frame export writes an 8-bit indexed PNG with the source palette indices and RGB palette intact; it does not yet reimport PNGs into the game format. Add `--listfile "$LISTFILE"` to any command when public names are needed. To inventory all five archives in one pass, use [`scripts/inventory-native-assets.sh`](../../scripts/inventory-native-assets.sh).

In the PBM archive viewer:

- Right, Down, or Space selects the next decodable PBM member.
- Left or Up selects the previous one.
- Escape or closing the window exits.

In the IMP frame viewer:

- Right or Left selects the next or previous visible logical frame within the current cycle.
- Down or Up selects the next or previous cycle within the current action.
- Page Down or Page Up selects the next or previous action sequence.
- Space toggles cycle-scoped animation at the current provisional 100 ms frame interval.
- C cycles between clean preview, visible mask, and raw-palette modes.
- Escape or closing the window exits.

When a paired generated `.h` member is available, the window title shows its recovered action name. Cycle direction and the provisional frame interval still require confirmation against the original executable.

Member matching is case-insensitive because the archive catalog and Windows game paths do not have reliable case consistency.

## Measured result

Tested against the installed GS5R3 profile on 2026-09-11:

| Check | Result |
| --- | ---: |
| Archive entries | 1,406 |
| Readable entries | 1,406 |
| IFF `FORM PBM` images | 1,377 |
| Successfully decoded PBMs | 1,377 |
| Core archive members classified | 9,804 / 9,804 |
| IMP binaries structurally parsed | 1,800 / 1,800 |
| IMP binaries pixel-expanded without decoder errors | 1,800 / 1,800 |
| Same-name IMP pairs matching known statistics | 1,784 / 1,798 |
| Release-mode archive enumeration | ~0.20 s |
| Release-mode full PBM scan/decode | ~1.0 s |

The viewer displayed `lbm\ACTIONS5.lbm` as a 612×120 image with 256 palette entries and ByteRun1 compression. The full scan found one image whose final compressed packet crosses a scanline boundary; matching the format's scanline semantics resolved it and is covered by a regression test.

The IMP decoder handles both observed frame-record variants, the custom packet RLE, 8/4/2/1-bit indexed pixels, row padding, direct duplicates, repeated cycles, and explicit sequence/cycle/frame ranges. Generated headers provide 4,649 names for 4,666 declared action sequences. The unresolved cases are reported as failures by `--validate-imp`; they cover 14 metadata/statistical mismatches and four orphan public-catalog names. See the [Stage 1 record](../../docs/native-asset-stage.md) for the exact scope and remaining gates.

## What the tool proves

- StormLib can be isolated behind a small safe-facing Rust API and can read the real game archive without extraction.
- The primary observed picture format is straightforward enough to implement and validate natively.
- SDL3 is adequate for immediate 2D inspection and nearest-neighbor presentation.
- All observed IMP payloads can be bounded and expanded into recognizable indexed sprite frames.
- Asset tooling can deliver value independently of a complete engine rewrite.

## What it does not prove

- The rendering is not yet behaviorally equivalent to the game. A visible bright-green color in the atlas suggests an engine-level chroma-key rule that is not represented by the PBM header's masking field.
- IMP rendering is not yet behaviorally equivalent to the game. The clean preview hides palette indices 0 and 1 as the inferred background and secondary-mask channels; the other viewer modes expose them. Their exact compositing semantics, origins, hotspots, and sequence timing still need reference comparisons.
- BMP/WAVE currently have metadata probes; maps, scenarios, fonts, and video are not decoded.
- The viewer recreates its streaming texture while drawing; caching is a production optimization, not a spike requirement.
- There is no thumbnail grid, search UI, batch/GUI export, editing, IMP reimport, or MPQ writing.
- A successful asset decoder does not reduce the much larger uncertainty in the GameScript host, simulation, AI, saves, or multiplayer.

## Code map

- `src/mpq.rs` — manual StormLib FFI and read-only archive/member ownership.
- `src/pbm.rs` — bounded IFF chunk parsing, palette conversion, PBM row handling, and ByteRun1 decoding.
- `src/imp.rs` — bounds-checked IMP tables, palette, RLE and packed-pixel decoding, hotspot and duplicate/repeated-frame structures, and generated-header validation.
- `src/png_export.rs` — lossless indexed-PNG output for resolved IMP frames.
- `src/asset.rs` — content-first classification and typed format metadata.
- `src/main.rs` — CLI inventory, extraction, validation, and SDL3 viewer.
- `build.rs` — local native-library search and runtime paths for the Apple Silicon spike.

## Sensible next slice

Compare representative IMP frames and animations with the original executable to establish chroma-key, origin, hotspot, cycle-direction, and timing semantics. Then probe representative map/scenario members while keeping game-specific rendering rules above the format decoder.
