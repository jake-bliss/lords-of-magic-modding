# Map and Scenario Format

## Status

**Header, cell grid, terrain-atlas lookup, and dominant 49-byte record milestone complete.** The native Rust parser bounds-checks every installed `.scn`, `.smp`, and `.lgd` file without modifying it. It renders the standard world map from original terrain art and structurally decodes all files in the dominant trailing-record family. The 52-/53-byte families and exact meanings of several object fields remain under investigation.

**An attended engine run on 2026-09-17 wrote maps with values chosen in advance and read them back.** It confirmed the tile-index reading by construction, produced the engine's terrain-type-to-tile table, and **refuted two claims this document previously asserted**: the cell storage order (it is packed y-major, not X-major) and the meaning of tag bit `0x00800000` (it does not mark a forced texture; its meaning is Unknown). Both refutations, and the reasoning that produced the wrong claims, are kept below.

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
| `+0` | 4 | `tag` | Low bits **are** the tile-atlas slot (Observed in gameplay, 2026-09-17). Bit `0x00800000` is set in part of the corpus; its meaning is **Unknown**. Bits `10..22` are unused corpus-wide |
| `+4` | 4 | `value_bits` / `value` | Every value is a finite little-endian `f32` in `0..20`; grayscale rendering produces coherent world relief |

### Cell storage order — packed y-major

**Observed in gameplay, 2026-09-17. This corrects a previous claim.** Cells are stored packed and
y-major: `cell_index = y × width + x`. Consecutive cells are one display row.

This document previously asserted `cell_index = x × height + y`, and the parser, the renderer and a
regression test all implemented it. The proof that it is wrong: a probe placed three terrain sprites
at `(20,30)`, `(21,30)` and `(20,31)` on a fresh 64x64 map — coordinates chosen so that the two
candidate encodings share no value — and the saved records carry cell indexes **1940, 1941 and
2004**, which is exactly `y × 64 + x`. X-major would have written 1310, 1374 and 1311.

**Why the wrong reading survived: every shipped map is square.** World maps are 128x128 and special
maps 48x48, so the two encodings are exact transposes that no shipped file can distinguish, and the
regression test that "prevents the earlier transposed rendering" had been fitted to a 2x2 fixture
that agreed with both. Tests covering this now use deliberately **non-square** synthetic maps, which
is the reusable lesson: *a fixture shaped like the corpus cannot catch a bug the corpus hides.*

Which operand is *x* is a separate question, because operand order and storage order are exact
transposes of each other and the byte data alone cannot separate them. It was settled from the
probe's screen capture: `map2screen` gives screen-x proportional to `(x − y)` and screen-down
proportional to `(x + y)`, so a painted run with the **first** operand varying must travel down and
to the **right**, and one with the second varying must travel down and to the left. Both painted
bands in the capture run down-right, so the first operand is x.

The rendered map preview transposes relative to every capture taken before 2026-09-17. That is the
correction, not a regression.

A July 2026 community report describes the first word as *"a 4-byte compression header in every
map ... basically just a version number and reserved space"* which *"disappears"* when a map exceeds
the original maximum size, corrupting maps and saves and producing a `TRASHBIN` display.

**Both halves are now refuted.** The "version number" reading was refuted by measurement across the
corpus (below). The "disappears when oversized" reading was refuted on 2026-09-17 by generating a
512x512 map in the running engine and parsing it — see [Oversized maps keep the
header](#oversized-maps-keep-the-header).

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

### Oversized maps keep the header

**Observed in gameplay, 2026-09-17.** Three maps were generated by the shipped engine and saved,
then parsed:

| File | Bytes | `[0x00]` | Declared | `16 + w*h*8` | Trailing | Records | Footer |
| --- | ---: | ---: | --- | ---: | ---: | ---: | --- |
| `zz128.scn` | 132,272 | `0x6f` | 128x128x8 | 131,088 | 1,184 | 24 | `01000000` |
| `zz256.scn` | 525,488 | `0x6f` | 256x256x8 | 524,304 | 1,184 | 24 | `01000000` |
| `zz512.scn` | 2,098,352 | `0x6f` | 512x512x8 | 2,097,168 | 1,184 | 24 | `01000000` |

The 512x512 map carries the same 16-byte prefix as the others and the same `0x6f` at `0x00`. Every
file is exactly `16 + width x height x 8 + 1,184` bytes, so nothing downstream is shifted, and the
parser reads all three with no oversized code path.

The engine also reported `mapw 512 maph 512` for the live map, so it accepted the size rather than
clamping it, and all three saves returned the same value as the 128 control.

**The 128 and 256 rungs are controls, and they are what make this a result.** Both sizes exist in
the shipped corpus, so the generated files can be checked against what the game ships: `0x6f` is
inside the `0x6c`-`0x6f` band world `.scn` files occupy, the prefix matches, the trailing section is
the dominant 49-byte family with its 8-byte frame, and footer `1` is one of the three observed
values. The generator writes what the game writes.

Two further findings fall out:

- **`0x6f` now appears at 512x512 too**, alongside 32, 48, 64, 128 and 256 — another size the word
  is indifferent to. All three files came from one generator with one tile set, which is what the
  tileset-selector hypothesis predicts.
- **Cell indexing stays consistent at 512.** All 24 records in each file are in bounds under the
  same packing; record 0 of `zz512.scn` is cell 241,316, which is `471 × 512 + 164`. Because these
  generated maps are square like every shipped map, this rung could not — and did not — distinguish
  the two candidate packings; the 2026-09-17 probe did, and under the corrected reading that record
  sits at `(x, y) = (164, 471)` rather than the `(471, 164)` originally recorded here. What the rung
  does show is that the convention does not change when the index stops fitting in 16 bits.

The identical 24-record placement set at every size — same instance ids from 200 up, same sprite-type
sequence, differing only in cell — is the random map generator placing a keep, a leader and a great
temple for each of eight faiths. It makes the record layout demonstrably size-independent.

The real failure behind the community report is elsewhere: `gs\edit\mapgen.gs` line 192 warns that
maps over 128 in a dimension break **random dungeon placement** outside GS5R3. That is a script
concern, not a header one.

### How the map was generated

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

This is implemented as `LOM_PROBE=mapsize`, and it is a ladder rather than a single 512. It
generates 128, then 256, then 512, because **128 and 256 both exist in the shipped corpus**: a
generated file at those sizes can be compared against one the game itself wrote. If the generated
128 does not parse identically to a shipped 128, the generator is not a faithful writer and nothing
the 512 says about the header can be trusted. The control comes first, and a unit test enforces
that order. The probe also logs `mapw` and `maph` after each generation, so a silent clamp cannot be
mistaken for a successful oversized map.

See [the probe harness](agent-handoff.md#engine-probe-harness--this-is-the-reusable-part) for how it
is installed and, more importantly, for why writing into the loose `map/` directory needs more care
than writing into the archives.

The second word is strongly inferred to be elevation or height. The shipped tile-definition comments state that `1000` represents `1.0` in the map model, but exact runtime units and interpolation remain unverified.

Script-side elevation is a **float**. The shipped random map generator in `gs\rmg.gs` passes `0.11`,
`0.5`, `1.0`, `2.0`, `6.3` and `-1.0` to `paintelevation` (`x y terrain elevation`) and `setelevation`
(`x y elevation`), and reads back through `getelevation` (`x y`). All three take coordinate pairs,
not the packed locations that `getterrainspritelocation` returns. A 2020 forum note that "the previous
elevation of 1.0 is effective to 10" describes a change to the **GSZ map editor's** UI, not to the
engine, and must not be folded into the measured `map2screen` z coefficient.

## Terrain tile lookup

**Observed in gameplay, 2026-09-17: the low tag bits are the tile-atlas slot, exactly.** A probe
forced seven cells to slots 0, 1, 2, 48, 96, 392 and 623 with the editor's `forcetexture`, saved the
map, and every one round-tripped byte-exact — including both ends of the range. This upgrades the
claim from inferred-by-correlation to observed-by-construction.

The low tag bits directly index the atlas declared by the active `.til` file. For the standard world map:

- `tilesb01.til` declares `tilesb01.lbm`, a 16×39 atlas of 32×32 tiles (624 slots);
- its definitions cover 617 tile slots and 11 terrain types;
- masking `0x00800000` from all 1,258,496 corpus cells produces 603 distinct indices, all in `0..623`;
- rendering `URAK.scn` through those indices produces a coherent, correctly oriented world containing connected oceans, snow, forest/grass, and desert regions.

### Tag bit `0x00800000` — **Refuted** as a forced-texture flag

**Refuted in gameplay, 2026-09-17.** This document previously said: *the flag appears only in `.smp`
files, 27,448 cells across 146 files; many 48×48 maps contain exactly 188 flagged cells, the size of
their perimeter; combined with the editor's separate `setterrain` and `forcetexture` operations,
this is strong evidence that `0x00800000` means a forced texture.* That reasoning is preserved here
so nobody re-derives it from the same corpus shape.

It is wrong. A probe ran `392 clearmap` on a fresh 64x64 map, which calls `forcetexture` on every
one of its 4,096 cells, then forced seven more cells individually, then saved. **Not one saved cell
has the bit set** — zero of 4,096. Forcing a texture does not set this bit, so the bit does not mean
"forced texture". Its meaning is **Unknown** again, and it stays on the open list for
[issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4).

What still holds is the masking, which is independent of the meaning: with `0x00800000` masked out,
every corpus cell indexes a tile in `0..623`, and without it the flagged cells do not. The decoder
keeps the raw tag and exposes `MapCell::tile_index()` plus a deliberately meaning-free
`MapCell::high_flag_set()`; the constant is `CELL_TAG_HIGH_FLAG`, renamed from
`CELL_TAG_FORCED_TEXTURE` so the code no longer asserts something that was measured false.

### `forcetexture` and `setterrain` are not the same operation

**Observed in gameplay, 2026-09-17.** Both editor operations write a tile, but they differ in
footprint, which is why both exist:

- `forcetexture x y slot` writes **exactly one cell**. The probe's forced row at `y = 8` changed
  seven cells and nothing at `y = 7` or `y = 9`. (Six of the seven differ from the cleared base
  tile; the seventh was forced to the base tile itself.)
- `setterrain x y type` writes the cell **plus blended transition tiles into its 8-neighbourhood**.
  The probe's `setterrain` run along `y = 12`, `x = 8..28`, changed rows **11, 12 and 13** across
  `x = 7..29` — one cell beyond the painted run on every side.

### Terrain types and their tiles

**Observed in gameplay, 2026-09-17.** Running `setterrain` with each type `0..=10` and reading the
saved cells back gives the engine's terrain-type-to-tile table. Type names are **Documented** in
`gs\maplib.gs`:

| Terrain type | Tile slot painted | `gs\maplib.gs` names |
| ---: | ---: | --- |
| 0 | 175 | `tt_dirt`, `tt_rough` |
| 1 | 392 | `tt_water` |
| 2 | 111 | `tt_desert`, `tt_sand` |
| 3 | 159 | `tt_mountain` |
| 4 | 207 | `tt_happy`, `tt_meadow` |
| 5 | 255 | `tt_ice`, `tt_snow` |
| 6 | 15 | `tt_land`, `tt_plains` |
| 7 | 303 | `tt_swamp` |
| 8 | 351 | `tt_lava` |
| 9 | 459 | `tt_road` |
| 10 | 469 | `tt_impassible`, `tt_impassable` |

The table lives next to the map code as data, in `spikes/asset-viewer/src/map.rs`
(`TERRAIN_TYPES`, `terrain_type_base_tile`, `base_tile_terrain_type`), with a unit test on the exact
values rather than only this prose.

The **inverse** direction was measured in the same run: `getterrain` on cells whose *tile* had been
forced and whose terrain type was never set answered slot 0 → type 0, 1 → 6, 2 → 6, 48 → 0, 96 → 0,
392 → 1, 623 → 9. Two of those slots (48, 96) are not any type's painted base tile and still answer
with a type.

Both directions together establish that **a cell's terrain type is derived from its tile index
through the tileset, not stored in the cell** — which is consistent with tag bits `10..22` being
unused across the whole corpus. The samples are recorded as `OBSERVED_TILE_TERRAIN_TYPES`.

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

The `49-byte records + 8 fixed bytes` family is now structurally decoded in 196 files containing 16,628 records. Every record retains its complete raw bytes.

**Observed in gameplay, 2026-09-17, by construction:** the section is `u32 record_count`, then
`record_count × 49` bytes of records, then a `u32` footer. The corpus could only show that the
section is `count × 49 + 8` bytes long — it did not say which four of those eight fixed bytes came
first. Saving one map twice settled it: with no sprites the tail is 8 bytes, `00000000 01000000`;
with three sprites it is 155 bytes, `03000000` + 147 + `01000000`. The count leads and the footer
trails. **The footer was `1` in both**, so it is not a sprite-related count, and its meaning stays
Unknown.

| Record offset | Size | Current name | Corpus evidence / confidence |
| ---: | ---: | --- | --- |
| `+0` | 4 | `record_kind` | Always `1` in 16,628 records; observed |
| `+4` | 4 | `record_version` | Always `1`; observed |
| `+8` | 4 | `cell_index` | Always unique and in bounds per file. **Corrected 2026-09-17:** unpacks as `y × width + x`, not X-major |
| `+12` | 4 | `unknown_12` | Always `0xffffffff`; observed |
| `+16` | 4 | `unknown_16` | Always `0`; observed |
| `+20` | 4 | `instance_id` | Unique per file, range `200..1659`. **Observed in gameplay, 2026-09-17:** three sprites on a fresh map got 200, 201, 202 — sequential, starting at 200 |
| `+24` | 4 | `attribute_bits` | Meaning unknown, and the decoder's nibble reading is **suspect**: every probe record carries a plain `0x00000001`, which `attribute_code_candidate()` reports as 0. No replacement reading is asserted |
| `+28` | 4 | `sprite_type` | **Observed in gameplay, 2026-09-17:** all three probe records carry 470, the id `addterrainspritetype` returned in the same keypress. Promoted from `sprite_type_candidate` |
| `+32` | 2 | `marker_32` | Always `0x01ff`; observed |
| `+34` | 4 | `procedure_id_candidate` | `-1` or `0..717`; correlated with `setterrainspriteprocid` usage |
| `+38` | 4 | `unknown_38` | Always `0`; observed |
| `+42` | 4 | `unknown_42` | Always `0xffffffff`; observed |
| `+46` | 3 | `unknown_46` | Always zero; observed |

The footer values observed are `0`, `1`, and `3`. Their meaning is unknown. Record invariants and bounds are enforced by synthetic tests and were revalidated across the complete local map corpus. Candidate semantic names deliberately remain candidates until editor save diffs or runtime behavior prove them.

### The `.scn` / `.smp` split is not a format difference

**Observed in gameplay, 2026-09-17.** The probe saved one identical map state twice, once with
`savescenariomap` and once with `savespecialmap`. The two files are **byte-identical** (sha256
`7744b749…4a3c`). So the two operators are one writer, and the corpus's concentration of 52-byte
trailing records in `.smp` files must be a **content** difference — different object kinds on those
maps — rather than a different serializer. That redirects the remaining half of issue #4: look for
what special maps *contain*, not for a second format.

### Placed sprites round-trip exactly

**Observed in gameplay, 2026-09-17.** The map saved after placing three sprites and then destroying
all three is byte-identical to the map saved before any were placed. Placement and removal leave no
residue in the file, which is what makes a save-diff a trustworthy instrument here.

### Still open on issue #4

[GitHub issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4) now tracks:

- **the meaning of tag bit `0x00800000`** — Unknown again, after "forced texture" was refuted;
- **the 52-/53-byte record families** — now known to be a **content** difference, not a format one,
  since both save operators write identical bytes;
- **the 18 unmatched tails** and the one ambiguous file;
- **the header word at `0x00`** — our engine-generated maps say `0x6f`, shipped `URAK.scn` says
  `0x6c`; the tileset-selector hypothesis is unproven;
- **the trailing footer**, which stayed `1` across an empty and a populated save and so is not a
  count of anything the probe changed;
- **the attribute field at `+24`**, whose upper-nibble reading is suspect;
- exact procedure-identifier semantics at `+34`.

A controlled-save attempt reached the cloned Wine profile and launched the Map Editor integration, but macOS accessibility controls prevented reliable programmatic interaction with its Wine window. No map file was changed. The save-diff experiment remains parked rather than substituting guessed field meanings.

## Commands

```sh
cd spikes/asset-viewer
cargo build --release

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
whether the `0x00800000` flag is set, and the elevation word — for the whole grid or for an
inclusive `X0 Y0 X1 Y1` rectangle. `--diff-maps` prints the two headers, every differing cell, and
the first differing byte of the trailing section with a hex window either side; it compares the
tails as raw bytes from their own starts, because assuming a record size would beg the question the
diff is being used to answer. Together they are the readback half of an engine probe: writing
chosen values from the running game is only useful if they can be read out again.

With a tile definition and atlas, the viewer starts in terrain-art mode. Press `C` to cycle through diagnostic cell tags and candidate elevation. The terrain view proves tile selection and orientation, but its 8×8-per-cell overview is not yet a faithful recreation of the original renderer's full-size terrain composition.

## Confidence

- **Observed in a local binary:** all 365 files have a 16-byte prefix, declared 8-bit depth, a complete `width × height × 8` cell grid, and a bounded trailing section; all 196 exact 49-byte-family files and 16,628 records satisfy the decoded bounds and invariants.
- **Observed in gameplay (2026-09-17):** the low tag bits are the tile-atlas slot exactly, for seven forced slots spanning `0..623`; cells are packed `y × width + x`; the terrain-type-to-tile table above; terrain type is derived from the tile through the tileset, not stored in the cell; `forcetexture` writes one cell while `setterrain` also blends its 8-neighbourhood; the trailing section is `count`, records, `footer`; `sprite_type` at `+28` is the terrain sprite type id; `instance_id` starts at 200 and increments; `savescenariomap` and `savespecialmap` write identical bytes; sprite placement and removal round-trip byte-exactly.
- **Observed in gameplay (2026-09-17), earlier run:** a 512x512 map generated by the shipped engine carries the same 16-byte prefix as a 128, so the header does not disappear on oversized maps.
- **Corrected:** cell and record coordinates, previously documented and implemented as X-major (`x × height + y`). Every shipped map is square, so the corpus could not falsify it.
- **Refuted:** tag bit `0x00800000` as a forced-texture flag. Forcing textures into 4,096 cells set it in none of them.
- **Inferred:** the second word is elevation; record `+34` is a procedure identifier; the header word at `0x00` is a tileset selector.
- **Unknown:** the first header word, elevation units, the meaning of tag bit `0x00800000`, the trailing footer, the attribute field at `+24`, and the remaining record families.
