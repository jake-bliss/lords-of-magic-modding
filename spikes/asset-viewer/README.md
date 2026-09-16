# Rust Asset/MPQ Tool

## Outcome

The initial spike succeeded and is now evolving into the Stage 1 native asset layer. A native 64-bit Rust program opens the installed game's MPQs through a narrow read-only StormLib wrapper, loads an external filename catalog, classifies and inspects members, decodes IFF `FORM PBM` images, IMP sprite frames, tile-set definitions, and map records, probes BMP/WAVE metadata, and displays images, animations, and terrain through SDL3.

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
GS_MPQ='/path/to/Lords of Magic Special Edition/English/gs.mpq'
LOMSE_EXE='/path/to/Lords of Magic Special Edition/English/lomse.exe'

cd ../..
scripts/fetch-lom-listfile.sh
LISTFILE="$PWD/artifacts/reference-listfiles/lords-of-magic.txt"
cd spikes/asset-viewer

target/release/lom-asset-viewer --list "$PIC_MPQ"
target/release/lom-asset-viewer --catalog "$PIC_MPQ"
target/release/lom-asset-viewer --scan "$PIC_MPQ"
target/release/lom-asset-viewer --scan-gamescript "$GS_MPQ" --listfile "$LISTFILE" --exe "$LOMSE_EXE"
target/release/lom-asset-viewer --probe-gamescript "$GS_MPQ" 'gs\standard.gs' --listfile "$LISTFILE" --eval '3 5 min 3 5 max'
target/release/lom-asset-viewer --inspect "$PIC_MPQ" 'LBM\ACTIONS5.lbm'
target/release/lom-asset-viewer --extract "$PIC_MPQ" 'LBM\ACTIONS5.lbm' /tmp/actions5.lbm
target/release/lom-asset-viewer --validate-imp "$IMP_MPQ" --listfile "$LISTFILE"
target/release/lom-asset-viewer --describe-imp "$IMP_MPQ" 'units\imp\chcr5a.imp' --listfile "$LISTFILE"
target/release/lom-asset-viewer --view-imp "$IMP_MPQ" 'units\imp\chcr5a.imp' --listfile "$LISTFILE"
target/release/lom-asset-viewer --export-imp-frame "$IMP_MPQ" 'units\imp\chcr5a.imp' 155 /tmp/chcr5a-frame-155.png --listfile "$LISTFILE"
target/release/lom-asset-viewer "$PIC_MPQ" 'LBM\ACTIONS5.lbm'
target/release/lom-asset-viewer --scan-map-dir '/path/to/Lords of Magic Special Edition/English/map'
target/release/lom-asset-viewer --inspect-file '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
target/release/lom-asset-viewer --describe-map '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
target/release/lom-asset-viewer --view-map '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
target/release/lom-asset-viewer --view-map MAP.scn tilesb01.til tilesb01.lbm
target/release/lom-asset-viewer --export-map-preview MAP.scn tilesb01.til tilesb01.lbm /tmp/map-preview.png
```

## Writing sprite placement

The engine draws a frame as `top_left = anchor + placement - (width >> 1, height >> 1)`, measured in
the running engine on 2026-09-16. The placement pair is the vector from the anchor to the **centre**
of the frame, in screen pixels with `+y` down, and it is added.

Solve for the value a re-cropped frame needs — here `palm1b.imp` frame 0, 53x53 at `(9, -20)`, padded
by 4 pixels on every side:

```sh
target/release/lom-asset-viewer --imp-placement-for 61 61 320 180 303 134
# placement	13	-16
```

Each axis shifts by half the added pixels, because the convention is centre-relative.

Write it back into a loose IMP:

```sh
target/release/lom-asset-viewer --set-imp-placement in.imp 0 13 -16 out.imp
target/release/lom-asset-viewer --set-imp-placement in.imp 0 5 -40 out.imp --hotspot 0
```

Frame record bytes `+8..+12` are overloaded: a frame carries **either** an origin pair **or** a
pointer to hotspot records, never both. Use `--hotspot TYPE` for the second form; `--describe-imp`
shows which a frame has. The writer keeps the file length identical, re-parses before writing,
refuses a placement it cannot read back, refuses to overwrite an existing output, refuses a duplicate
frame's origin, and warns when several frames share the record or the hotspot array being written.

**Scope.** The rule above was measured against frames carrying the *origin pair*. Which hotspot type
the engine uses as the draw anchor for record-bearing frames is **not yet established** — see the
research log. `examples/imp_placement_survey.rs` reports the corpus split.

`--extract`, `--export-imp-frame`, and `--export-map-preview` use create-new semantics and refuse to overwrite an existing output. IMP frame export writes an 8-bit indexed PNG with the source palette indices and RGB palette intact. Map preview export writes an RGBA overview using the original terrain atlas at 8×8 output pixels per map cell. Neither path reimports PNGs into the game format. Add `--listfile "$LISTFILE"` to any command when public names are needed. To inventory all five archives in one pass, use [`scripts/inventory-native-assets.sh`](../../scripts/inventory-native-assets.sh).

In the PBM archive viewer:

- Right, Down, or Space selects the next decodable PBM member.
- Left or Up selects the previous one.
- Escape or closing the window exits.

In the IMP frame viewer:

- Right or Left selects the next or previous visible logical frame within the current facing.
- Down or Up selects the next or previous facing within the current action.
- Page Down or Page Up selects the next or previous action sequence.
- Space toggles facing-scoped animation at the current provisional 100 ms frame interval.
- C facings between clean preview, visible mask, and raw-palette modes.
- Escape or closing the window exits.

When a paired generated `.h` member is available, the window title shows its recovered action name. Facing direction and the provisional frame interval still require confirmation against the original executable.

The IMP title and `--describe-imp` report raw sequence/facing metadata plus candidate origin or hotspot values. In the map viewer, `C` switches among original terrain artwork (when `.til` and atlas paths are supplied), candidate elevation, and stable diagnostic tag colors. The terrain mode proves atlas selection and orientation, but its overview is not yet a recreation of the original renderer's full-size terrain composition.

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
| Same-name IMP pairs matching known statistics | 1,788 / 1,798 |
| Loose `.scn`/`.smp`/`.lgd` grids parsed | 365 / 365 |
| Tile-set definitions parsed | 26 / 26 |
| Dominant 49-byte map records decoded | 16,628 in 196 files |
| Release-mode archive enumeration | ~0.20 s |
| Release-mode full PBM scan/decode | ~1.0 s |

The viewer displayed `lbm\ACTIONS5.lbm` as a 612×120 image with 256 palette entries and ByteRun1 compression. The full scan found one image whose final compressed packet crosses a scanline boundary; matching the format's scanline semantics resolved it and is covered by a regression test.

The IMP decoder handles both observed frame-record variants, the custom packet RLE, 8/4/2/1-bit indexed pixels, row padding, direct duplicates, shared-pixel records, repeated facings, typed placement candidates, and explicit sequence/facing/frame ranges. The unresolved cases are reported as failures by `--validate-imp`; they now cover ten metadata/statistical mismatches and four orphan public-catalog names. See the [Stage 1 record](../../docs/native-asset-stage.md) for the exact scope and remaining gates.

## What the tool proves

- StormLib can be isolated behind a small safe-facing Rust API and can read the real game archive without extraction.
- The primary observed picture format is straightforward enough to implement and validate natively.
- SDL3 is adequate for immediate 2D inspection and nearest-neighbor presentation.
- All observed IMP payloads can be bounded and expanded into recognizable indexed sprite frames.
- Asset tooling can deliver value independently of a complete engine rewrite.

## What it does not prove

- The rendering is not yet behaviorally equivalent to the game. A visible bright-green color in the atlas suggests an engine-level chroma-key rule that is not represented by the PBM header's masking field.
- IMP rendering is not yet behaviorally equivalent to the game. The clean preview hides palette indices 0 and 1 as the inferred background and secondary-mask channels; the other viewer modes expose them. Their exact compositing semantics, origins, hotspots, and sequence timing still need reference comparisons.
- BMP/WAVE currently have metadata probes. Map/scenario/component grids, standard terrain lookup, and the dominant 49-byte trailing family are decoded; 52-/53-byte map records, fonts, and video are not decoded.
- The viewer recreates its streaming texture while drawing; caching is a production optimization, not a spike requirement.
- There is no thumbnail grid, search UI, batch/GUI export, editing, IMP reimport, or MPQ writing.
- A successful asset decoder does not reduce the much larger uncertainty in the GameScript host, simulation, AI, saves, or multiplayer.

## Code map

- `src/mpq.rs` — manual StormLib FFI and read-only archive/member ownership.
- `src/pbm.rs` — bounded IFF chunk parsing, palette conversion, PBM row handling, and ByteRun1 decoding.
- `src/imp.rs` — bounds-checked IMP tables, palette, RLE and packed-pixel decoding, hotspot and duplicate/repeated-frame structures, and generated-header validation.
- `src/map.rs` — bounded common header/cell-grid parsing, X-major coordinates, terrain tags, and 49-byte placed-sprite records for SCN/SMP/LGD files.
- `src/tile.rs` — parser for `.til` atlas geometry, terrain types, and tile relationships.
- `src/gamescript.rs` — bounded GameScript lexer, procedure diagnostics, name inventory, and static `run` references.
- `src/gamescript_vm.rs` — experimental bounded value stack, dictionaries, procedures, core operators, and structured execution failures.
- `src/png_export.rs` — lossless indexed IMP-frame PNG and RGBA map-preview output.
- `src/asset.rs` — content-first classification and typed format metadata.
- `src/main.rs` — CLI inventory, extraction, validation, and SDL3 viewer.
- `build.rs` — local native-library search and runtime paths for the Apple Silicon spike.

## Sensible next slice

Expand the experimental GameScript VM only far enough to classify and run additional engine-light utilities, then add read-only module loading with structured unknown-name traces. The controlled Map Editor save diff remains parked because macOS accessibility controls prevented reliable automation of Wine's editor window; remaining map variants and original-engine comparisons stay in explicit [GitHub issues](https://github.com/jake-bliss/lords-of-magic-modding/issues) rather than being encoded as assumptions.
