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
target/release/lom-asset-viewer --dump-map-cells MAP.scn
target/release/lom-asset-viewer --dump-map-cells MAP.scn 8 8 20 8
target/release/lom-asset-viewer --diff-maps BEFORE.scn AFTER.scn
target/release/lom-asset-viewer --view-map '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
target/release/lom-asset-viewer --view-map MAP.scn tilesb01.til tilesb01.lbm
target/release/lom-asset-viewer --export-map-preview MAP.scn tilesb01.til tilesb01.lbm /tmp/map-preview.png
```

`--dump-map-cells` prints one line per cell — `x`, `y`, packed index, raw tag, masked tile index,
whether the unexplained `0x00800000` flag is set, and the elevation word — for the whole grid, or for
an inclusive `X0 Y0 X1 Y1` rectangle. `--diff-maps` prints both headers, every differing cell, and
the first differing byte of the trailing section with a hex window either side, comparing the tails
as raw bytes rather than as assumed records. They are the readback half of an engine probe: writing
chosen values from the running game only proves something if they can be read out again.

## Writing maps

```sh
target/release/lom-asset-viewer --map-roundtrip '/path/to/Lords of Magic Special Edition/English/map'
target/release/lom-asset-viewer --map-set-tile      IN.scn X Y TILE_SLOT   OUT.scn
target/release/lom-asset-viewer --map-set-terrain   IN.scn X Y TERRAIN     OUT.scn
target/release/lom-asset-viewer --map-set-elevation IN.scn X Y VALUE       OUT.scn
target/release/lom-asset-viewer --map-fill-terrain  IN.scn TERRAIN         OUT.scn
target/release/lom-asset-viewer --map-paint-terrain IN.scn X0 Y0 X1 Y1 TERRAIN OUT.scn TILESET.til [--seed N]
target/release/lom-asset-viewer --map-place-sprite  IN.scn X Y SPRITE_TYPE OUT.scn
target/release/lom-asset-viewer --map-sprite-types
target/release/lom-asset-viewer --map-transition-rings
target/release/lom-asset-viewer --map-remove-sprite IN.scn INSTANCE_ID     OUT.scn
```

Everything here rests on one property: **an unedited map re-encodes to the exact bytes it was read
from.** `--map-roundtrip` asserts it over the installed corpus — 365 checked, 365 byte-identical,
16,628 placed-sprite records rebuilt from their typed fields, 0 failures. Run it first.

When **editing existing data**, fields whose meaning is still Unknown — the header word at `0x00`,
the trailing footer, the record attribute field at `+24`, tag bit `0x00800000`, and the entire
trailing section of every family this project has not decoded — are **copied, never minted**.
Placing a *new* sprite is the exception: a record that did not exist has to get its bytes from
somewhere, and one of the nine it mints (`+24`) contradicts the corpus reading of that field. See
[map format](../../docs/map-format.md#writing-maps). That is what lets the writer be correct
while the format is only partly solved, and it is also why there is no create-a-map-from-nothing
mode: three of those fields would have to be invented. Generate in the shipped GS5R3 editor, which
makes maps from 32 to 1024 in steps of 32, then edit here.

`TERRAIN` is a number `0..10` or a `gs\maplib.gs` name with or without its `tt_` prefix, so `1`,
`tt_water` and `water` are the same thing.

`SPRITE_TYPE` is likewise **a name or a raw id** — `castle1` works, and a near miss suggests
alternatives. The names come from the engine's own `terrainsprites` dict, dumped on 2026-09-17, and
are **profile-specific**: ids are assigned in script execution order, so a different script set
shifts them. `--map-sprite-types` lists the table and both it and named placement say so.

`--map-set-terrain` reproduces the editor's `forcetexture`: **one cell**, hard edge. The engine's
`setterrain` also blends transition tiles into the 8-neighbourhood, and **that ring is now
measured** for all eleven backgrounds — one offset table plus a per-background anchor. See
[the transition rings](../../docs/map-format.md#setterrain-transition-tiles-one-offset-table-one-anchor-per-background)
and `--map-transition-rings`.

A painted region's **interior** is a random draw from its terrain's tile family and cannot be
reproduced by any writer; the ring can.

A removed `instance_id` **is** reissued by the next invocation — the high-water mark that holds it
back cannot be persisted, because the format has nowhere to put one. If anything outside the map
references a sprite by id, do not remove-then-place.

The loose `map/` directory has no backup, so there is no in-place mode: every command takes an
explicit output path, refuses to write over its input by canonical path, opens the output
`create_new`, and re-parses the encoded bytes to read the edit back before anything reaches disk.

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

**Both placement forms obey the same rule.** A frame with a zero hotspot count carries its placement
as the origin pair; a frame with hotspot records carries it in **record 0**, which is engine-reserved
and unreadable from script. Record 0 was confirmed by measurement in the running engine on
2026-09-16. Use `--hotspot 0` for that form. `examples/imp_placement_survey.rs` reports the corpus
split, and `examples/shift_record0.rs` shifts record 0 across every frame of a sprite.

One caveat: the record-0 measurement went through the terrain-sprite draw path, so a unit-specific
constant in the *anchor* is not ruled out. The sign and the centre-relative form are settled.

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
| IMP pairs matching their generated header exactly | 1,795 / 1,800 |
| Remaining pairs, each a value-pinned exception | 5 |
| Loose `.scn`/`.smp`/`.lgd` grids parsed | 365 / 365 |
| Tile-set definitions parsed | 26 / 26 |
| Dominant 49-byte map records decoded | 16,628 in 196 files |
| Release-mode archive enumeration | ~0.20 s |
| Release-mode full PBM scan/decode | ~1.0 s |

The viewer displayed `lbm\ACTIONS5.lbm` as a 612×120 image with 256 palette entries and ByteRun1 compression. The full scan found one image whose final compressed packet crosses a scanline boundary; matching the format's scanline semantics resolved it and is covered by a regression test.

The IMP decoder handles both observed frame-record variants, the custom packet RLE, 8/4/2/1-bit indexed pixels, row padding, direct duplicates, `0x04` shared-pixel records inside an ordinary frame-record array, typed placement candidates, and explicit sequence/facing/frame ranges. `--validate-imp` reports **zero failures** over the 1,800 paired members: 1,795 match on every statistic, and the remaining five sit in the value-pinned `IMP_VALIDATION_EXCEPTIONS` table alongside two value-pinned orphan catalog notes. Anything not covered by those exact recorded numbers is still reported as a failure. See the [Stage 1 record](../../docs/native-asset-stage.md) for the exact scope and remaining gates.

## What the tool proves

- StormLib can be isolated behind a small safe-facing Rust API and can read the real game archive without extraction.
- The primary observed picture format is straightforward enough to implement and validate natively.
- SDL3 is adequate for immediate 2D inspection and nearest-neighbor presentation.
- All observed IMP payloads can be bounded and expanded into recognizable indexed sprite frames.
- Asset tooling can deliver value independently of a complete engine rewrite.

## What it does not prove

- The rendering is not yet behaviorally equivalent to the game. A visible bright-green color in the atlas suggests an engine-level chroma-key rule that is not represented by the PBM header's masking field.
- IMP rendering is not yet behaviorally equivalent to the game. The clean preview hides palette indices 0 and 1 as the inferred background and secondary-mask channels; the other viewer modes expose them. Placement is settled — see [hotspots](../../docs/hotspots.md). What still needs reference comparison is the shadow-index blend, the chroma-key rule, and sequence timing and facing direction ([issue #2](https://github.com/jake-bliss/lords-of-magic-modding/issues/2)).
- BMP/WAVE currently have metadata probes. Map/scenario/component grids, standard terrain lookup, and the dominant 49-byte trailing family are decoded; 52-/53-byte map records, fonts, and video are not decoded.
- The viewer recreates its streaming texture while drawing; caching is a production optimization, not a spike requirement.
- There is no thumbnail grid, search UI, batch/GUI export, or general asset reimport. IMP **placement** write-back exists (`--set-imp-placement`); pixel and frame reimport do not. MPQ member replacement is proven in `examples/mpq_replace.rs` but is not a supported CLI command.
- A successful asset decoder does not reduce the much larger uncertainty in the GameScript host, simulation, AI, saves, or multiplayer.

## Code map

- `src/mpq.rs` — manual StormLib FFI and read-only archive/member ownership.
- `src/pbm.rs` — bounded IFF chunk parsing, palette conversion, PBM row handling, and ByteRun1 decoding.
- `src/imp.rs` — bounds-checked IMP tables, palette, RLE and packed-pixel decoding, hotspot and duplicate/repeated-frame structures, and generated-header validation.
- `src/map.rs` — bounded common header/cell-grid parsing, packed `y * width + x` coordinates, terrain tags, the measured terrain-type-to-tile table, and 49-byte placed-sprite records for SCN/SMP/LGD files.
- `src/tile.rs` — parser for `.til` atlas geometry, terrain types, and the full eight-column neighbour constraints, plus the constraint matcher `--map-paint-terrain` re-tiles from.
- `examples/parse_all_tilesets.rs` — parse every `.til` in a directory and report atlas size, terrain-id range and any row that fails to declare all eight constraints. Reading columns the parser used to discard can only *add* failure modes for `--view-map`, so this is the check that it has not: 26 parsed, 0 failed, 0 incomplete on the GS5R3 set. Takes a path, because no tileset is committed.
- `src/gamescript.rs` — bounded GameScript lexer, procedure diagnostics, name inventory, and static `run` references.
- `src/gamescript_vm.rs` — experimental bounded value stack, dictionaries, procedures, core operators, and structured execution failures.
- `src/png_export.rs` — lossless indexed IMP-frame PNG and RGBA map-preview output.
- `src/asset.rs` — content-first classification and typed format metadata.
- `src/main.rs` — CLI inventory, extraction, validation, and SDL3 viewer.
- `build.rs` — local native-library search and runtime paths for the Apple Silicon spike.

## Sensible next slice

Expand the experimental GameScript VM only far enough to classify and run additional engine-light utilities, then add read-only module loading with structured unknown-name traces. The controlled Map Editor save diff remains parked because macOS accessibility controls prevented reliable automation of Wine's editor window; remaining map variants and original-engine comparisons stay in explicit [GitHub issues](https://github.com/jake-bliss/lords-of-magic-modding/issues) rather than being encoded as assumptions.
