# Map and Scenario Format

## Status

**Header, cell grid, terrain-atlas lookup, and dominant 49-byte record milestone complete.** The native Rust parser bounds-checks every installed `.scn`, `.smp`, and `.lgd` file without modifying it. It renders the standard world map from original terrain art and structurally decodes all files in the dominant trailing-record family. The 52-/53-byte families and exact meanings of several object fields remain under investigation.

The corpus is the working GS5R3 profile, including its supplied custom maps. No original map data or rendered captures are stored in Git.

## Observed prefix

Every one of the 365 inspected files begins with the same little-endian prefix:

| Offset | Size | Current name | Evidence |
| ---: | ---: | --- | --- |
| `0x00` | 4 | `metadata` | Varies by file; meaning unknown. A 2026 community claim names this a compression header holding a version number plus reserved space — see below |
| `0x04` | 4 | `width` | 32, 48, 64, 128, 160, or 256 in the observed corpus |
| `0x08` | 4 | `height` | Matches the same bounded cell dimensions |
| `0x0c` | 4 | `bits_per_pixel` | Always 8 in the observed corpus |
| `0x10` | `width × height × 8` | Cell grid | Exactly one eight-byte record per cell |

The cell record currently preserves both words without assigning engine behavior:

| Cell offset | Size | Current name | Evidence |
| ---: | ---: | --- | --- |
| `+0` | 4 | `tag` | Low bits select a tile-atlas slot; observed high flag `0x00800000` marks forced texture candidates |
| `+4` | 4 | `value_bits` / `value` | Every value is a finite little-endian `f32` in `0..20`; grayscale rendering produces coherent world relief |

The cells are stored X-major: `cell_index = x × height + y`. This is consistent across placed-object coordinates and the Map Editor scripts. The viewer converts this storage order to normal display rows; a regression test prevents the earlier transposed rendering.

A July 2026 community report describes the first word as *"a 4-byte compression header in every
map ... basically just a version number and reserved space"* which *"disappears"* when a map exceeds
the original maximum size, corrupting maps and saves and producing a `TRASHBIN` display.

**Measured on 2026-09-16, that reading does not survive.** Across all 365 installed map files the word
takes more than twenty distinct values spanning `0x3f`-`0x6f`, and it is independent of geometry:
`0x6f` occurs at 32x32, 48x48, 64x64, 128x128 and 256x256, while 48x48 files alone carry a dozen
different values. A version number would not vary that way. The values instead cluster by file
family - every `ORLIBR*`, `ORTGIL*` and `FIVILG*` sub-map is `0x4f`, and world `.scn` files are
`0x6c`-`0x6f` - which makes a **tileset or terrain-set selector** the better hypothesis, and would
also close the separate "header-to-tileset selection" unknown recorded below. Neither reading is
proven. The community claim's testable half is untouched: an oversized map should *omit* the field
and shift every later offset by four bytes. Recorded in [issue #22](https://github.com/jake-bliss/lords-of-magic-modding/issues/22) and in
[community research](community-research.md).

### The oversized map can be generated here

That test was parked because no map larger than 256x256 was available. It no longer needs one from
outside. **Observed in the archive**, the shipped GS5R3 map editor generates maps from 32 to 1024 in
steps of 32: `gs\edit\mapgen.gs` holds `map_width` and `map_height` in units of 32 at map zoom and
offers presets of 32, 512 and 1024. Its only call into the generator, at line 306, also fixes the
operand order as width first:

```
newmapdict begin map_width 32 mul map_height 32 mul end make_custom_random_map
```

`gs\rmg.gs` defines `make_custom_random_map`, which pops height then width and ends in `mw mh
newmap`. `gs\hotkey.gs` line 562 gives the save operator, one string operand. So the whole
experiment is one hotkey body:

```
512 512 make_custom_random_map
"map/zz512.scn" savescenariomap pop
```

It writes one new file to `map/` and modifies no archive. `mapgen.gs` line 192 warns that maps with
a dimension greater than 128 break random dungeons outside GS5R3, which is independent support for
the *phenomenon* the community claim describes without saying anything about its mechanism.

The second word is strongly inferred to be elevation or height. The shipped tile-definition comments state that `1000` represents `1.0` in the map model, but exact runtime units and interpolation remain unverified.

Script-side elevation is a **float**. The shipped random map generator in `gs\rmg.gs` passes `0.11`,
`0.5`, `1.0`, `2.0`, `6.3` and `-1.0` to `paintelevation` (`x y terrain elevation`) and `setelevation`
(`x y elevation`), and reads back through `getelevation` (`x y`). All three take coordinate pairs,
not the packed locations that `getterrainspritelocation` returns. A 2020 forum note that "the previous
elevation of 1.0 is effective to 10" describes a change to the **GSZ map editor's** UI, not to the
engine, and must not be folded into the measured `map2screen` z coefficient.

## Terrain tile lookup

The low tag bits directly index the atlas declared by the active `.til` file. For the standard world map:

- `tilesb01.til` declares `tilesb01.lbm`, a 16×39 atlas of 32×32 tiles (624 slots);
- its definitions cover 617 tile slots and 11 terrain types;
- masking `0x00800000` from all 1,258,496 corpus cells produces 603 distinct indices, all in `0..623`;
- rendering `URAK.scn` through those indices produces a coherent, correctly oriented world containing connected oceans, snow, forest/grass, and desert regions.

The flag appears only in `.smp` files: 27,448 cells across 146 files. Many 48×48 maps contain exactly 188 flagged cells, the size of their perimeter. Combined with the editor's separate `setterrain` and `forcetexture` operations, this is strong evidence that `0x00800000` means a forced texture rather than another tile-index bit. The decoder retains the raw tag while exposing masked index and flag accessors.

All 26 recovered `.til` members parse as bounded text definitions. They declare an atlas name, grid dimensions, 32×32 tile size, terrain types, and tile-to-terrain relationships. The repository does not include those proprietary definitions or images.

## Independent community corpus

Eight community maps were downloaded on 2026-09-16 from Mantera's site into ignored `artifacts/` and
parsed with no failures. They are the first map corpus this project has tested that did not ship with
an installed profile:

| Map | Dimensions | Records | Footer |
| --- | --- | ---: | ---: |
| `Feuerundeis.scn` | 160x160 | 1,056 | 0 |
| `Mumm-Ra2005.scn` | 128x128 | 684 | 0 |
| `Mumm-Ra2005b.scn` | 128x128 | 698 | 0 |
| `Permeon.scn` | 256x256 | 1,345 | 0 |
| `URAKpartII.scn` | 128x128 | 583 | 1 |
| `Van Lezing.scn` | 128x128 | 2,243 | 0 |

`Feuerundeis.scn` is **160x160**, a dimension absent from every installed profile and outside the
previously documented 32/48/64/128/256 set. The parser accepted it unchanged, which is evidence the
bounds are genuinely data-driven rather than fitted to the shipped corpus.

## Corpus result

Verified on 2026-09-12:

| Kind | Files | Dimensions |
| --- | ---: | --- |
| `.lgd` legend scenario | 8 | 128×128 |
| `.scn` map scenario | 20 | 32×32 (1), 64×64 (1), 128×128 (17), 256×256 (1) |
| `.smp` map component | 337 | 48×48 (336), 64×64 (1) |
| **Total** | **365** | **0 parse failures** |

The trailing bytes begin immediately after the cell grid. The first trailing word is retained as `trailing_head_u32`; in many files it behaves like a record count. Exact-length comparisons identify these candidate families:

| Kind | 49-byte records + 8 fixed bytes | 52-byte records + 4 fixed bytes | 53-byte records + 4 fixed bytes | Unknown/ambiguous |
| --- | ---: | ---: | ---: | ---: |
| `.lgd` | 5 | 0 | 1 | 2 |
| `.scn` | 15 | 0 | 5 | 0 |
| `.smp` | 176 | 144 | 0 | 17 |

## Dominant 49-byte placed-sprite family

The `49-byte records + 8 fixed bytes` family is now structurally decoded in 196 files containing 16,628 records. The section is a little-endian count, `count × 49` records, and a four-byte footer. Every record retains its complete raw bytes.

| Record offset | Size | Current name | Corpus evidence / confidence |
| ---: | ---: | --- | --- |
| `+0` | 4 | `record_kind` | Always `1` in 16,628 records; observed |
| `+4` | 4 | `record_version` | Always `1`; observed |
| `+8` | 4 | `cell_index` | Always unique and in bounds per file; X-major coordinates verified |
| `+12` | 4 | `unknown_12` | Always `0xffffffff`; observed |
| `+16` | 4 | `unknown_16` | Always `0`; observed |
| `+20` | 4 | `instance_id` | Unique per file, range `200..1659`; role strongly inferred |
| `+24` | 4 | `attribute_bits` | Only upper nibble varies; codes `0..11` and `15`; meaning unknown |
| `+28` | 4 | `sprite_type_candidate` | Range `0..441`, 234 values; correlated with editor `addterrainsprite` usage |
| `+32` | 2 | `marker_32` | Always `0x01ff`; observed |
| `+34` | 4 | `procedure_id_candidate` | `-1` or `0..717`; correlated with `setterrainspriteprocid` usage |
| `+38` | 4 | `unknown_38` | Always `0`; observed |
| `+42` | 4 | `unknown_42` | Always `0xffffffff`; observed |
| `+46` | 3 | `unknown_46` | Always zero; observed |

The footer values observed are `0`, `1`, and `3`. Their meaning is unknown. Record invariants and bounds are enforced by synthetic tests and were revalidated across the complete local map corpus. Candidate semantic names deliberately remain candidates until editor save diffs or runtime behavior prove them.

The 52-/53-byte families, 18 unknown layouts and one ambiguous file, header-to-tileset selection, attribute nibble, footer, and exact sprite/procedure semantics remain tracked in [GitHub issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4).

A controlled-save attempt reached the cloned Wine profile and launched the Map Editor integration, but macOS accessibility controls prevented reliable programmatic interaction with its Wine window. No map file was changed. The save-diff experiment remains parked rather than substituting guessed field meanings.

## Commands

```sh
cd spikes/asset-viewer
cargo build --release

target/release/lom-asset-viewer --scan-map-dir '/path/to/Lords of Magic Special Edition/English/map'
target/release/lom-asset-viewer --inspect-file '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
target/release/lom-asset-viewer --describe-map '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
target/release/lom-asset-viewer --view-map '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
target/release/lom-asset-viewer --view-map MAP.scn tilesb01.til tilesb01.lbm
target/release/lom-asset-viewer --export-map-preview MAP.scn tilesb01.til tilesb01.lbm /tmp/map-preview.png
```

With a tile definition and atlas, the viewer starts in terrain-art mode. Press `C` to cycle through diagnostic cell tags and candidate elevation. The terrain view proves tile selection and orientation, but its 8×8-per-cell overview is not yet a faithful recreation of the original renderer's full-size terrain composition.

## Confidence

- **Observed:** all 365 files have a 16-byte prefix, declared 8-bit depth, a complete `width × height × 8` cell grid, and a bounded trailing section.
- **Observed:** low tag values resolve into the declared tile atlas and produce coherent original-art terrain; X-major indexing matches placed-sprite cells and editor iteration order.
- **Observed:** all 196 exact 49-byte-family files and 16,628 records satisfy the decoded bounds and invariants.
- **Inferred:** the second word is elevation; tag bit `0x00800000` represents forced texture; record `+20`, `+28`, and `+34` are instance, sprite-type, and procedure identifiers.
- **Unknown:** the first header word, elevation units, tag flags beyond the observed forced-texture bit, remaining record families, and exact behavior of candidate object fields.
