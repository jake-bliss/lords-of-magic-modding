# Map and Scenario Format

## Status

**Header, cell grid, terrain-atlas lookup, and the placed-object record section are complete.** The native Rust parser bounds-checks every installed `.scn`, `.smp`, and `.lgd` file without modifying it. It renders the standard world map from original terrain art and structurally decodes the trailing record section of **all 365** installed maps -- **21,117 records across six layouts**, each rebuilt from typed fields with no gap. Object editing works on all 365 maps rather than 196. The exact meanings of several object fields remain under investigation, and so does what the six layouts *are*: the corpus shows them as a version ladder, which is Inferred, not Observed.

**An attended engine run on 2026-09-17 wrote maps with values chosen in advance and read them back.** It confirmed the tile-index reading by construction, produced the engine's terrain-type-to-tile table, and **refuted two claims this document previously asserted**: the cell storage order (it is packed y-major, not X-major) and the meaning of tag bit `0x00800000` (it does not mark a forced texture; its meaning is Unknown). Both refutations, and the reasoning that produced the wrong claims, are kept below.

**A map writer landed on 2026-09-17, and the engine accepts what it writes.** Every installed map
re-encodes to the exact bytes it was read from -- 365 of 365, with all 21,117 placed-sprite records
rebuilt from their typed fields (16,628 when the writer landed; the other five record layouts were
decoded later the same day). An attended run then handed the running game seven maps and it
loaded **all seven**: a shipped map re-encoded by this project, an edited one, one with a sprite
placed, one created from nothing, and one that is **non-square**. The two created-from-nothing maps
re-saved **byte-identically**. See [Writing maps](#writing-maps) and [Engine
acceptance](#engine-acceptance-measured).

**`setterrain` is reproduced from shipped data, not from a measurement.** The `.til` tilesets declare
a constraint on all eight neighbours of every atlas slot, and painting is: set the region's terrain,
then re-select a tile for every disturbed cell. That reproduces the engine on 2,084 of 2,084
deterministic cells across the `terrainrings` captures, and it works on shipped world maps, where the
earlier offset-table version refused everywhere. A painted region's interior remains a random draw
the engine makes and no writer can reproduce. See [the tileset declares the whole blend
rule](#the-tileset-declares-the-whole-blend-rule).

The corpus is the working GS5R3 profile, including its supplied custom maps. No original map data or rendered captures are stored in Git.

## Observed prefix

Every one of the 365 inspected files begins with the same little-endian prefix:

| Offset | Size | Engine name | Evidence |
| ---: | ---: | --- | --- |
| `0x00` | 4 | format version | **Observed in a local binary, 2026-09-17.** Read into the scenario object's first dword at `0x005aa12c` by `fread` at `0x004855e5`; the loader branches on it in six places. See [What the loader does](#what-the-loader-does-read-out-of-lomseexe) |
| `0x04` | 4 | `width` | **Observed in a local binary, 2026-09-17.** `fread` at `0x004a5300` into map object `+0x5c`; the allocator at `0x004a4f20` stores it as the grid width |
| `0x08` | 4 | `height` | **Observed in a local binary, 2026-09-17.** `fread` at `0x004a5311` into map object `+0x60` |
| `0x0c` | 4 | **bytes per cell** | **Corrected, 2026-09-17.** Not `bits_per_pixel`. `fread` at `0x004a5341`; the reader multiplies it by 64 at `0x004a535c` to size one 64-cell block read. The writer at `0x004a548a` writes the literal `8` set at `0x004a544b` |
| `0x10` | `width × height × 8` | Cell grid | Read 64 cells at a time by `fread` at `0x004a5364`; written 512 bytes at a time at `0x004a54aa` |

The cell record is **two `u16` fields and one `f32`**, not two `u32`s:

| Cell offset | Size | Engine name | Evidence |
| ---: | ---: | --- | --- |
| `+0` | 2 | tile-atlas slot, signed | **Observed in a local binary, 2026-09-17.** Every *interpreting* read is `movsx r32, word [cell]` (`0x004a5261`, `0x004a4c3d`, `0x004a56d7`, `0x004a5cea`, `0x004a5dce`, `0x004a5e00`, `0x004a5efe`, `0x004a6388`, `0x004a9671`); every write is 16-bit (`0x004a5e08`, `0x004a5f06`, `0x004a50da`). The value goes into the bounds-checked tileset lookup at `0x00508f10`. The one wider access is the whole-cell dword copy at `0x004a9660`/`0x004a9666`, which decodes nothing |
| `+2` | 2 | unidentified signed 16-bit scalar | **Observed in a local binary, 2026-09-17.** 11 reads, every one a `movsx`; 12 writes, every one 16-bit; **never masked anywhere**. `0x00519d00` uses it as `(0x80 - field) * k >> 7`, so `0x80` is an arithmetic **scale base**. `0x004c5cc2` compares it against map object `+0x4c`. — **Inferred, not Observed:** that its range is *closed* at `0..128` and that the corpus's `0x0080` is that range saturated. Nothing disassembled clamps it. See [the field is a signed scalar](#the-2-field-is-a-signed-scalar-not-a-bitfield) |
| `+4` | 4 | elevation, `f32` | **Observed in a local binary, 2026-09-17.** Loaded with `fld dword [cell+4]` at `0x004a59f3`, `0x004a5235`, `0x00455e09`, `0x00457bac`, `0x0053291c`, `0x00532928`, `0x005329cd`, `0x00532abe`, `0x00532aca`, `0x00532b65`; stored with `fstp dword` at `0x00457c13`, `0x00457e25`, `0x00457e5d`. Fifteen FPU sites, no integer arithmetic |

**No bit of a cell is masked, tested or set anywhere in the survey.** See
[The cell is three fields](#the-cell-is-three-fields-and-no-bit-of-it-is-ever-masked).

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

**What that argument does and does not establish.** It rules out `forcetexture`'s first operand
disagreeing with `map2screen`'s first operand — they are the same axis. It cannot establish which of
`map2screen`'s own two operands is the world's *x*, because that labelling was itself decoded and
measured the same way. A global swap of both leaves every number in this document correct and every
`x`/`y` column in `--dump-map-cells` and `--describe-map` consistently mislabelled. Treat the axis
*names* as **Inferred**; the packing itself — `second_operand × width + first_operand` — is
**Observed**, and nothing downstream depends on the naming.

The rendered map preview transposes relative to every capture taken before 2026-09-17. That is the
correction, not a regression.

A July 2026 community report describes the first word as *"a 4-byte compression header in every
map ... basically just a version number and reserved space"* which *"disappears"* when a map exceeds
the original maximum size, corrupting maps and saves and producing a `TRASHBIN` display.

**The "disappears when oversized" half is refuted**, on 2026-09-17, by generating a 512x512 map in
the running engine and parsing it — see [Oversized maps keep the
header](#oversized-maps-keep-the-header).

**The "version number" half was recorded as refuted too, and that was wrong. Corrected
2026-09-17.** The argument below — that the word varies independently of geometry and clusters by
file family, so "a version number would not vary that way" — does not follow: a version varies with
*when a file was built*, which is independent of its geometry and does cluster by family. And there
is now positive evidence for it. The word's 19 observed values partition the six placed-object
record layouts **with no overlap at all**, in order, and the engine's own fresh save stamps the top
of the range while writing the newest layout. That is the correlation, Observed across all 365 files;
reading it as a version, and as the thing the loader uses to pick a layout, stays **Inferred**. See
[the header word partitions the layouts](#the-header-word-partitions-the-layouts). What *is* refuted
is the **tileset-selector** hypothesis the paragraph below prefers: the engine rewrites this word
from its own state on every save, so it is not carrying a per-map tileset choice.

**Measured on 2026-09-16, and the conclusion drawn from it has since been corrected — see above.**
Across all 365 installed map files the word
takes **19** distinct values spanning `0x3f`-`0x6f` (**corrected**: this said "more than twenty"; the
count is 19, re-measured 2026-09-17), and it is independent of geometry: `0x6f` occurs at 32x32,
48x48, 64x64, 128x128 and 256x256, while 48x48 files alone carry 15 different values. A version
number would not vary that way — the 2026-09-16 conclusion, and the one now corrected. The values instead cluster by file
family - every `ORLIBR*`, `ORTGIL*` and `FIVILG*` sub-map is `0x4f`, and world `.scn` files are
`0x6c`-`0x6f` - which was read in 2026-09-16 as making a **tileset or terrain-set
selector** the better hypothesis. Both parts of that have since moved: the tileset reading is
refuted, and the family clustering turned out to be the record-layout partition. Every value in this
range is one of the 19 the layout partition accounts for. The community claim's testable half is untouched: an oversized map should *omit* the field
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
- its definitions cover 617 tile slots and 11 terrain types, **each with a constraint on all eight of
  its neighbours** — which is the whole blend rule, and is
  [what the paint operator reads](#the-tileset-declares-the-whole-blend-rule);
- masking `0x00800000` from all 1,258,496 corpus cells produces 603 distinct indices, all in `0..623`;
- rendering `URAK.scn` through those indices produces a coherent world containing connected oceans, snow, forest/grass, and desert regions. This establishes that the masked values index real terrain art rather than noise. It establishes **nothing about orientation**: a transposed world map is also coherent, which is why this check never caught the X-major error.

### Tag bit `0x00800000` — **Refuted** as a forced-texture flag

**Refuted in gameplay, 2026-09-17.** This document previously said: *the flag appears only in `.smp`
files, 27,448 cells across 146 files; many 48×48 maps contain exactly 188 flagged cells, the size of
their perimeter; combined with the editor's separate `setterrain` and `forcetexture` operations,
this is strong evidence that `0x00800000` means a forced texture.* That reasoning is preserved here
so nobody re-derives it from the same corpus shape.

It is wrong. A probe ran `392 clearmap` on a fresh 64x64 map, which calls `forcetexture` on every
one of its 4,096 cells, then forced seven more cells individually, then saved. **Not one saved cell
has the value** — zero of 4,096. Forcing a texture does not set it, so it does not mean
"forced texture". **Confirmed in a local binary, 2026-09-17:** `forcetexture`'s worker cannot set it
even in principle — its only cell write is a 16-bit store to lane `+0` at `0x004a5f06`, and `+2` is
a different field. Its meaning is **Unknown** again, and it stays on the open list for
[issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4).

**Superseded 2026-09-17 by the disassembly: this was never a bit of the tag.** The paragraph that
stood here said "what still holds is the masking … with `0x00800000` masked out, every corpus cell
indexes a tile in `0..623`, and without it the flagged cells do not". The conclusion was right on
the corpus and the model behind it was wrong. Cell word 0 is **two 16-bit fields**: the tile slot is
the whole low `u16` and `0x00800000` is the value `0x0080` of a separate signed scalar at `+2`. So
the correct mask is the low 16 bits, not "everything except one bit" — the two agree on every
shipped cell only because `0x0080` is the only thing the corpus ever puts in the upper field.
`MapCell::tile_index()` masks `0xffff`; `MapCell::high_flag_set()` is retained as a
deliberately meaning-free predicate on that value. See
[the cell is three fields](#the-cell-is-three-fields-and-no-bit-of-it-is-ever-masked).

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
through the tileset, not stored in the cell** — and the engine agrees: the survey found no field in
a cell other than the tile slot, the `+2` scalar and the elevation. (This used to be supported here
by "tag bits `10..22` are unused across the whole corpus", which rested on the 32-bit-tag model this
branch retired; bits 10–15 are part of the tile slot. The conclusion is unchanged and now rests on
`0x00508f10` instead.) The samples are recorded as `OBSERVED_TILE_TERRAIN_TYPES`.

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

The trailing bytes begin immediately after the cell grid, and the first trailing word is the record
count. **Corrected 2026-09-17:** an earlier version of this document listed three candidate families
-- 49-, 52- and 53-byte records -- plus 18 "unknown/ambiguous" tails, and the parser decoded only the
49-byte one. There are **six** layouts, the 18 were not a separate phenomenon, and all 365 files are
now decoded. See below.

## The six placed-object record layouts

**Observed in a local binary, 2026-09-17.** Every installed map's trailing section is uniform: one
record size throughout, with nothing left over. The section is `u32 record_count`, then
`count × record_size` bytes of records, then a `u32` footer **in two of the six layouts only**.
Exactly one layout accounts for each of the 364 files that hold at least one record; the 365th holds
none.

| Records | Footer | Files | Records | Header word at `0x00` | Extensions |
| ---: | --- | ---: | ---: | --- | --- |
| 47 bytes | none | 6 | 434 | 98 | 4 `.smp`, 2 `.lgd` |
| 47 bytes | `u32` | 3 | 52 | 101 | 3 `.smp` |
| 48 bytes | none | 9 | 222 | 63, 73 | 9 `.smp` |
| 49 bytes | `u32` | 196 | 16,628 | 102, 105-111 | 176 `.smp`, 15 `.scn`, 5 `.lgd` |
| 52 bytes | none | 144 | 1,253 | 76, 79, 81, 87, 89 | 144 `.smp` |
| 53 bytes | none | 6 | 2,528 | 96, 97 | 5 `.scn`, 1 `.lgd` |
| **Total** | | **364** | **21,117** | | |

`chbldg01.smp` is the 365th: a four-byte trailing section holding a zero count, which is what a
footerless layout writes for a map with no objects. Its header word is 76, which the corpus pairs
only with the 52-byte layout, and that is the single place the parser consults the header word --
see [the header word and the layout](#the-header-word-partitions-the-layouts).

### Two lengths that two layouts fit, and what separates them

**Observed in a local binary, 2026-09-17, by enumeration** -- every record count against all six
layouts, not by search. For `count >= 1` there are exactly **two** collisions:

| count | section bytes | layouts that fit | what separates them |
| ---: | ---: | --- | --- |
| 1 | 57 | `49 + footer`, `53 + none` | the **marker word alone**. With one record both strides put it at the same offset, so every other head field is byte-identical between the two readings |
| 4 | 196 | `47 + footer`, `48 + none` | `record_kind` at the wrong stride: the 47-byte reading puts record 1's first word inside record 0's padding, where it is zero |

`CAVWAT02.SMP` is the 196-byte case in the shipped corpus -- four 48-byte records -- and it is
**doubly determined**: the stride head-check and the header word (73) agree independently. **No**
installed map has a count of 1, and the per-layout minimum counts are 4, 4, 6, 6, 11 and 16, so the
57-byte case cannot come from the corpus. It is three `--map-remove-sprite` calls away from any
six-record 53-byte map such as `fire.lgd`, which is why the marker check is load-bearing rather than
belt-and-braces: without it neither candidate can be eliminated, the tail stays raw, and the tool
refuses a map it could edit a moment earlier.

A third, degenerate collision is `count == 0`, where every layout of the same footer width writes
identical bytes. That is the one the header word settles.

**This resolves the "18 unmatched tails" open item.** Nine of the 18 are the 48-byte layout and nine
are the 47-byte one. They looked unmatched because only three record sizes had ever been tried; the
parser now tries all six and validates every record's head against the stride. The one "ambiguous
file" was `chbldg01.smp`, above.

**Where the layouts differ, and where they do not.** All six share a 32-byte head: eight `u32`s
carrying the same eight fields in the same order in all 21,117 records, with the same invariants --
`cell_index` unique and in bounds in every file, `instance_id` unique and strictly increasing in
every file. Past `+32` they split into two shapes, and the split is **by record size**:

| Shape | Sizes | Records | Bytes from `+32` |
| --- | ---: | ---: | --- |
| procedure tail | 47, 49 | 17,114 | `ff 01`, `u32` procedure id, `u32` `0`, `u32` `0xffffffff`, then **1** zero byte (47) or **3** (49) |
| plain tail | 48, 52, 53 | 4,003 | `u32` `0`, `u32` `0`, `u32` `0xffffffff`, `u32` `0`, then `u32` `0xffffffff` (52, 53), then one zero byte (53) |

The two shapes are **not** one shape at an offset. The procedure tail's two-byte `0x01ff` marker sits
where the plain tail has four zero bytes, and their trailing padding differs, so no single shift maps
one onto the other. The leading hypothesis before this measurement was "the 49-byte record plus three
or four extra bytes", and that is **not** what the bytes are: two of the new layouts are *shorter*
than 49, and the plain-tail layouts have no procedure id field at all rather than an extra field.

**What the extra bytes mean is Unknown.** Every byte outside the eight head fields and the procedure
id is constant within its layout across the whole corpus -- zero, or `0xffffffff` -- so there is
nothing to correlate against sprite type, coordinates, footer value or map dimensions. They are
decoded as named, typed, per-layout fields that round-trip exactly and assert nothing. `attribute_bits`
at `+24` is the one head field that varies by shape: the procedure-tail layouts carry the upper-nibble
codes, and all 4,003 plain-tail records carry zero.

### The header word partitions the layouts

**Observed in a local binary, 2026-09-17.** Across all 365 maps the header word at `0x00` partitions
the six layouts with **no overlap at all** -- no value appears with two layouts -- and the partition
is ordered: 63-73, 76-89, 96-97, 98, 101, 102-111 for the 48-, 52-, 53-, 47-no-footer, 47-footer and
49-byte layouts. The engine's own fresh save stamps 111, the top of the range, and writes the 49-byte
layout.

**How thin parts of that partition are, stated rather than glossed.** Support is very uneven, and a
third of the values rest on one file each:

| Layout | Header words, with the number of files carrying each |
| --- | --- |
| 48 + none | 63 (**1**), 73 (8) |
| 52 + none | 76 (2), 79 (140), 81 (**1**), 87 (**1**), 89 (**1**) |
| 53 + none | 96 (5), 97 (**1**) |
| 47 + none | 98 (6) |
| 47 + footer | 101 (3) |
| 49 + footer | 102 (5), 105 (27), 106 (109), 107 (31), 108 (**1**), 109 (**1**), 110 (5), 111 (17) |

Seven of the 19 values -- 63, 81, 87, 89, 97, 108, 109 -- appear in exactly one file. And the
tie-break that matters in practice, `chbldg01.smp`'s, rests on word 76, which occurs in two files,
one of which is `chbldg01.smp` itself. So the partition has no counterexample across 365 files, which
is worth keeping, and it is one file deep in seven places.

**Confirmed, 2026-09-17, and the Inferred reading below is superseded.** The loader does read the
word and does switch on it: the record serialiser at `0x0050da70` gates five field groups on it and
the scenario reader at `0x00485617` gates a sixth. The five thresholds reproduce all six record
sizes exactly — see
[The record size is one struct with five version gates](#the-record-size-is-one-struct-with-five-version-gates).
What is superseded is only the *causation*; the partition itself stands and is now explained.

~~**Inferred:** that the word is a format version and that the loader uses it to choose the record
layout. The correlation is the observation; the causation is not. An attended run has already shown
the engine rewrites this word from its own state on every save rather than carrying the file's, which
is consistent with a version stamp but does not establish that the loader reads it. It is also
consistent with the layouts simply being the output of successive editor builds, which would produce
the same correlation without the loader consulting the word at all.~~

Because it is Inferred, the parser uses it for exactly one thing: choosing a layout for a section
whose record **count is zero**, where the section's length genuinely cannot distinguish the layouts.
Only the 19 values the corpus pairs with one layout resolve; anything else leaves such a section raw
rather than interpolating a version number.

### The dominant 49-byte layout

**Observed in gameplay, 2026-09-17, by construction:** the section is `u32 record_count`, then
`record_count × 49` bytes of records, then a `u32` footer. The corpus could only show that the
section is `count × 49 + 8` bytes long — it did not say which four of those eight fixed bytes came
first. Saving one map twice settled it: with no sprites the tail is 8 bytes, `00000000 01000000`;
with three sprites it is 155 bytes, `03000000` + 147 + `01000000`. The count leads and the footer
trails. **The footer was `1` in both**, so it is not a sprite-related count, and its meaning stays
Unknown.

| Record offset | Size | Current name | Corpus evidence / confidence |
| ---: | ---: | --- | --- |
| `+0` | 4 | `record_kind` | Always `1` in all 21,117 records of all six layouts; observed |
| `+4` | 4 | `record_version` | Always `1`; observed |
| `+8` | 4 | `cell_index` | Always unique and in bounds per file. **Corrected 2026-09-17:** unpacks as `y × width + x`, not X-major |
| `+12` | 4 | `unknown_12` | Always `0xffffffff`; observed |
| `+16` | 4 | `unknown_16` | Always `0`; observed |
| `+20` | 4 | `instance_id` | Unique and strictly increasing per file, range `100..1659`. **Observed in gameplay, 2026-09-17:** three sprites on a fresh map got 200, 201, 202 — sequential, starting at 200. **Corrected 2026-09-17:** the range was recorded as `200..1659`, which the other five layouts falsify — ten files start at 100 (all nine of the 48-byte layout and one of the 52-byte), and one starts at 203. Per-file lowest ids across the 364 files holding records are `{200: 353, 100: 10, 203: 1}` |
| `+24` | 4 | `attribute_bits` | Meaning unknown. **Observed in a local binary:** across 16,628 corpus records only the upper nibble varies, taking codes `0..11` and `15`. **Observed in gameplay, 2026-09-17:** all three probe records carry `0x00000001`, which has low bits set and so *violates* that corpus invariant. The contradiction is the finding. The likelier reading is that a freshly minted, procedure-less sprite writes a record shape the corpus does not contain — not that 16,628 records were misread — so the corpus measurement stands and `attribute_code_candidate()` is retained, flagged, and asserted of nothing |
| `+28` | 4 | `sprite_type` | **Observed in gameplay, 2026-09-17:** all three probe records carry 470, the id `addterrainspritetype` returned in the same keypress. Promoted from `sprite_type_candidate` |
| `+32` | 2 | `marker_32` | Always `0x01ff` in the 47- and 49-byte layouts, and four zero bytes at `+32` in the other three; observed. This is the field that tells the two tail shapes apart, and the parser checks it as a guard -- it is not what resolves a length two layouts fit |
| `+34` | 4 | `procedure_id_candidate` | Present only in the 47- and 49-byte layouts. 273 distinct values across 17,114 records: `-1` in 14,196 of them and then `8..717`; correlated with `setterrainspriteprocid` usage. **Corrected 2026-09-17:** the range was recorded as `-1` or `0..717`; the lowest non-negative value is **8** and `0` appears in no record, which is what makes a marker-less record misread as a procedure tail silent |
| `+38` | 4 | `unknown_38` | Always `0`; observed |
| `+42` | 4 | `unknown_42` | Always `0xffffffff`; observed |
| `+46` | 3 | `unknown_46` | Always zero; observed. **One byte, not three, in the 47-byte layout** -- the padding length is what distinguishes the two procedure-tail layouts |

The footer values observed are `0`, `1`, and `3`, in the two layouts that have one. Their meaning is unknown. Record invariants and bounds are enforced by synthetic tests and were revalidated across the complete local map corpus. Candidate semantic names deliberately remain candidates until editor save diffs or runtime behavior prove them.

### The `.scn` / `.smp` split is not a format difference

**Observed in gameplay, 2026-09-17.** The probe saved one identical map state twice, once with
`savescenariomap` and once with `savespecialmap`. The two files are **byte-identical** (sha256
`7744b749…4a3c`). So the two operators are one writer, and the corpus's concentration of 52-byte
trailing records in `.smp` files is not a `.scn`-versus-`.smp` serializer difference.

**Narrowed 2026-09-17, and the "content difference" reading was too strong.** The record layouts
turn out to differ *structurally*, not in what they contain: the 48-, 52- and 53-byte records have
no procedure-id field where the 47- and 49-byte records do. What the identical-bytes measurement
actually established is that the **current** writer is one writer for both extensions. The six
layouts partition by the header word at `0x00`, ordered, which reads as a version ladder, and the
`.smp` concentration then follows from *when those files were built* rather than from what is on
them. That is Inferred; see [the header word](#the-header-word-partitions-the-layouts).

### Placed sprites round-trip exactly

**Observed in gameplay, 2026-09-17.** The map saved after placing three sprites and then destroying
all three is byte-identical to the map saved before any were placed. Placement and removal leave no
residue in the file, which is what makes a save-diff a trustworthy instrument here.

### Still open on issue #4

[GitHub issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4) now tracks:

- **what the value `0x00800000` means** — it is not a bit: it is `0x0080` in a signed 16-bit scalar at cell offset `+2` (**Observed in a local binary, 2026-09-17**). `resetvisibility` writes that field; **`forcetexture` provably cannot**, because its worker's only write is a 16-bit store to `+0` at `0x004a5f06`. Eleven readers use the field arithmetically and none masks it, so reading it as a *flag* of any kind is retired; reading it as *visibility intensity* remains an inference from the operator's name plus the `(0x80 - field)` arithmetic and is **not** established. What computes the perimeter ring the corpus carries, and whether a `.smp` load-and-save preserves it, are open;
- ~~**the 52-/53-byte record families**~~ — **decoded 2026-09-17.** There are six record layouts,
  not three; all 365 maps decode, 21,117 records rebuild from typed fields, and object editing works
  on all 365. What the extra bytes *mean* is still Unknown: every one of them is constant within its
  layout, so there is nothing in the corpus to correlate them against. The earlier "content
  difference" reading is narrowed above;
- ~~**the 18 unmatched tails** and the one ambiguous file~~ — **resolved 2026-09-17**: nine are the
  48-byte layout, nine the 47-byte one, and the ambiguous file is `chbldg01.smp`, whose section is
  empty. They were never a separate phenomenon; only three record sizes had been tried;
- ~~**the header word at `0x00`**~~ — **settled 2026-09-17**: the engine rewrites it from its own
  state on every save and never reads it back from the file, so whatever selects a tileset, it is
  not this word;
- **the trailing footer**, which stayed `1` across an empty and a populated save and so is not a
  count of anything the probe changed;
- **the attribute field at `+24`**, whose upper-nibble reading is suspect;
- exact procedure-identifier semantics at `+34`.

A controlled-save attempt reached the cloned Wine profile and launched the Map Editor integration, but macOS accessibility controls prevented reliable programmatic interaction with its Wine window. No map file was changed. The save-diff experiment remains parked rather than substituting guessed field meanings.

## Writing maps

**A map writer exists as of 2026-09-17.** It is the first tool in this project that produces game
data rather than describing it, and it is built on one property:

> **An unedited map re-encodes to the exact bytes it was read from.**

`--map-roundtrip` asserts that over the installed corpus: **365 checked, 365 byte-identical, 21,117
placed-sprite records rebuilt from their typed fields, 0 failures.** Run it before trusting an edit.
The record count rose from 16,628 on 2026-09-17 when the other five layouts were decoded; the 4,489
newly counted records are the 47-, 48-, 52- and 53-byte layouts' 169 files, which until then
round-tripped as raw bytes without any field being checked.

Two rules make that possible while most of this format is still Unknown:

1. **When editing existing data, fields whose meaning is unknown are copied, never minted.** The
   header word at `0x00`, the trailing footer, the record attribute field at `+24`, tag bit
   `0x00800000`, every constant word in a record tail, and the whole trailing section of any tail
   this project cannot pin to one layout all survive a round trip untouched. A writer that guessed at them would corrupt maps in ways no test
   here could see.

   **Placing a new sprite is the exception, and it mints every field.** A record that did not exist
   has to get its bytes from somewhere, and **it is minted in the map's own layout** -- a 52-byte map
   gets a 52-byte record, not a 49-byte one.

   In the 47- and 49-byte layouts every minted value but one is invariant across all 17,114
   procedure-tail corpus records. The exception is the `+24` attribute field, written as
   `0x00000001` because that is what the 2026-09-17 probe watched the engine write for a fresh
   sprite — and it **contradicts** the corpus reading of that field, in which only the upper nibble
   varies. Whether the engine accepts a record of this shape is unmeasured.

   In the 48-, 52- and 53-byte layouts every minted value **including** `+24` is that layout's
   corpus constant, and `+24` is zero. That is **Inferred and unmeasured**: the engine run that
   watched a fresh record being written wrote a 49-byte record, so it says nothing about the older
   layouts, and `0x00000001` cannot simply be carried across because no record in any layout was
   ever observed holding it. Between a measured value from a different layout and the only value
   this layout has ever held, a new record takes the latter. An attended run on one of these maps is
   what would settle it.

   Editing an existing sprite mints nothing; placing a new one does.
2. **Records rebuild from typed fields, not from carried-over bytes.** The decoded fields cover
   every byte of a placed-sprite record with no gap, in all six layouts, so
   `PlacedSpriteRecord::to_bytes` reproduces the original exactly *and* an edited field actually
   lands. A record's own tail decides its size, so a record cannot disagree with itself, and a
   section refuses a record of another layout's size rather than writing a count word and a stride
   that disagree. `--map-roundtrip` checks that record by record, which is stricter than comparing whole files: a file can round-trip through its raw
   tail while a field is being written back wrong.

### What the editor can and cannot do

| Operation | Status |
| --- | --- |
| Force a cell's tile-atlas slot | Reproduces `forcetexture` exactly |
| Force a cell to a terrain type's base tile | Reproduces `forcetexture` with the measured terrain table |
| Fill every cell with a terrain type | Reproduces `clearmap`, tile for tile — **including its habit of laying a constraint-violating field.** See below |
| Set a cell's elevation word | Writes the word; its runtime units stay **Inferred** |
| Place or remove a terrain sprite | Works on **all 365** installed maps, in whichever of the six layouts each one uses; round-trips byte-exactly, as the engine's own does. Exercised a file at a time through the CLI: place on a free cell, remove, and all 365 came back byte-identical |
| **Paint a region and re-tile everything it disturbs** | Reproduces `setterrain` from the active `.til`'s declared constraints, except a region's randomly drawn interior. See below |
| Create a map from nothing | **Not offered.** See below |

**`setterrain` is now reproduced from the tileset, not from an offset table.** `--map-paint-terrain
IN X0 Y0 X1 Y1 TERRAIN OUT TILESET.til` sets the region's terrain and then **re-selects a tile for
every cell the paint disturbed** by matching its eight-neighbourhood against the constraints the
`.til` file declares. See [the tileset declares the whole blend
rule](#the-tileset-declares-the-whole-blend-rule). There is no direction table in the code path any
more; the ring falls out of the same mechanism as the region's own perimeter, because in the engine
it always was the same mechanism.

The tileset is a **trailing positional argument**, the shape `--view-map` already uses. It is not
optional in effect: the map file does not record which tileset it was authored against — the header
word at `0x00` was [refuted](#the-header-word-at-0x00-is-engine-output-not-map-input) as a stored
selector — so without one the verb refuses rather than defaulting to a guess.

What it still does **not** reproduce is a **newly painted** region interior. A cell whose eight
neighbours all share its terrain matches that terrain's whole eight-slot interior family, and where
the cell held none of them the engine draws among them at random — the same 3x3 painted twice gave
centre tiles 385 and 390. The tool chooses the lowest matching slot by default, takes `--seed N` for
a different legal draw, and prints the count as `drawn:` on every paint. A byte-exact match against
an engine-painted interior is **impossible**, and no test here claims one.

A cell that *already* holds a valid interior member is a different case and **is** reproducible: it
keeps it. That distinction is not cosmetic — conflating the two produced an over-claim in this
document's own history. See [the three things that decide a
cell](#the-three-things-that-decide-a-cell).

Everything the tileset cannot decide is **refused**, not approximated:

| Case | Why |
| --- | --- |
| No tileset supplied | No cell's terrain can be read, and the map does not say which `.til` it used. The refusal **names** what the gamescript binds this map to, says `no single answer` and lists the candidates when several encounters disagree, and says plainly that it cannot name one when the map is unresolved. It never defaults: the `.til` lives inside `pic.mpq`, so only a name can be resolved from a map path |
| A **shipped** tileset the gamescript does not bind this map to | Painting `aicave.smp` through `tilesa01.til` writes slot 392 into a map whose atlas has 64 slots. Any of a multi-valued map's candidates is accepted; a tileset name that is not one of the 26 shipped members is presumed modded and accepted as-is; and an **unresolved** map refuses nothing, because nothing is known about it |
| A paint whose input and output extensions are different map classes | The output name decides what the game loads the result as. `--map-paint-terrain realm.scn ... battle.smp` was accepted before review found it: a world map written under a combat name, which the tool itself then reported as reading through a different tileset |
| The tileset declares no tiles for the painted terrain | `tilesa01.til` stops at terrain 9 where `tilesb01.til` reaches 10, so this is a real difference between shipped files. It also catches a terrain the file lists and never draws |
| A cell holds a tile the tileset does not declare | The map was authored against a different tileset; `tilesb01.til` also leaves seven slots commented out |
| A cell's tile does not declare all eight constraints | Nothing shipped is like this — all 4,043 `TILE=` rows in all 26 tilesets have 11 fields — so it means a malformed file, and a tile whose rule is unknown must not be painted |
| **No tile of the required terrain accepts a cell's neighbourhood** | The tileset declares no boundary tile for that shape, and the engine writes one anyway — imitating it would be invention. See below |

**A partial road paint refuses, and that refusal is the old ragged-road measurement re-derived.** No
plains tile accepts a diagonal road neighbour, so painting a road rectangle into a field fails with
`no tile of terrain 6 accepts the neighbourhood`. The 2026-09-17 run measured road as *ragged along
every edge* on all seven blending backgrounds — not one tile per direction — and this is the same
fact read out of the file rather than out of an engine session: road's slots describe a **network**,
so a filled rectangle of it has no legal boundary. The error text says the engine "writes one anyway
rather than following its own constraints", which is exactly what the ragged ring was.

A **whole-map** road or impassable paint is different and does succeed: with no boundary anywhere
every cell is an interior, and the paint is a no-op on a field already holding that terrain.

**There is deliberately no whole-map refusal.** An earlier version refused a region covering the map
on the grounds that there was no ring to read a background from. There is no background to read any
more — each cell's terrain comes from its own tile — and a whole-map paint is a legitimate and
useful operation: it is "fill with correct interior tiles". Once the edge is read closed it is also
a no-op on an already-correct map.

**Three refusals from the old offset-table version are gone**, because the tileset answers what the
eleven-slot representative table could not:

- *"A background cell whose tile is not a type's representative tile."* The tileset declares the
  `self` column for 617 of 624 slots. **All 1,258,496 cells of all 365 installed maps hold a tile
  `tilesb01.til` declares** — zero unrecognised, measured 2026-09-17. This refusal never fires on
  the corpus.
- *"Two terrains in the 8-neighbourhood."* Painting next to an existing boundary is what constraint
  matching is *for*; the constraints are written in terms of mixed neighbourhoods.
- *"`tt_road` as the background."* Road backgrounds are decided by the road blocks like any other
  terrain, where the tileset has a tile for the shape.

**On shipped maps this now works, where it used to refuse everywhere.** Measured 2026-09-17 against
`tilesb01.til`, sampling 3x3 plains paints on a stride-8 grid:

| | Maps | Maps accepting at least one paint | Sampled sites succeeding |
| --- | --- | --- | --- |
| `.scn` world maps | 20 | 20 | 4,152 / 5,456 (76%) |
| `.lgd` legend maps | 8 | 8 | 1,252 / 2,048 (61%) |
| `.smp` battle maps, **bound** | 169 | 55 / 57 sampled | 784 / 912 (86.0%), each against its own gamescript tileset |
| `.smp` battle maps, **unresolved** | 168 | — | refused: no tileset can be named |

**The `.smp` rows are what this table could not previously fill.** The 169 bound battle maps are
sampled every 3rd of the sorted list, so head and tail are both covered, and each is painted
through **its own** gamescript tileset with a 3x3 rectangle of that map's own dominant terrain —
terrain ids are tileset-local, so "plains" is not a thing a cave tileset has. 784 of 912 sites
succeed across 55 of the 57 sampled maps. The remaining refusals are the ordinary
`no tile of terrain N accepts the neighbourhood` case that world maps also hit.

*(An earlier version of this table claimed 490/496 = 98.8% for all 337 `.smp` against
`tilesa01.til`. That survey was run through the wrong tileset, and it succeeded so widely precisely
because `tilesa01.til` declares the whole 624-slot atlas and so always has some candidate — a high
success rate against a tileset the game does not use is worse than a refusal, because it writes
slots the map's real atlas cannot show.)*

**A second blocker sat behind the tileset one, and it is fixed here.** `--map-paint-terrain`'s
terrain argument was parsed by the same function `--map-set-terrain` uses, whose ceiling is
`tilesb01.til`'s eleven types. Combat tilesets reach 42, so painting a battle map failed on
`21 is not one of the 11 terrain types` for most of the corpus even with the right tileset in hand —
the sampled success rate was **7.5%** before this and 86.0% after. A paint's ceiling is now the
supplied tileset, which `PaintRefusal::TerrainTypeNotInTileSet` was already enforcing; the
tileset-less verbs keep the eleven-type table, because they have nothing else to check against.
This is the concrete consequence of "terrain ids are tileset-local" that this document had recorded
as a hazard without measuring.

A real example, `URAK.scn` at `(10, 26)..(12, 28)`A real example, `URAK.scn` at `(10, 26)..(12, 28)`: 9 region cells and 16 ring cells written, **24 of
the 25 determined by a single candidate**, one ambiguous (the interior), elevations and trailing
section untouched. The previous version refused this outright.

**Terrain ids are tileset-local, so none may be hardcoded as a global.** `tilesb01.til` declares
0..10; the other tilesets go much further — `cavecry2.til` reaches **42**, `debldg02.til` 39,
`wabldg02.til` 36. Parsing all 26 (`cargo run --example parse_all_tilesets`) gives atlases of 64,
128, 256 and 624 slots and between 10 and 19 declared terrain types each. The eleven-name terrain
table in `map.rs` is `tilesb01.til`'s vocabulary and nothing wider, which independently reinforces
the next point.

**The `.smp` battle maps each use their own tileset, named per encounter in the gamescript.**
See [which tileset a map is read through](#which-tileset-a-map-is-read-through).

**Refuted: "no sampled tileset fits `.smp`; the best had 36% of cells undeclared".** This document
asserted that, and it was wrong twice over. The sample of 15 tilesets it drew from **excluded both
624-slot files**, and it reported the best of what was left. And the number was inverted: 35.41% is
the fraction of `.smp` cells `aibldg01.til` and `eabldg01.til` *declare*, not the fraction they
leave undeclared. The lesson is the sampling one this repository has now been caught by twice —
*validate on a representative sample* — and the reversed sense is why a percentage should be printed
with the noun it counts.

**Also refuted, and this one was the project's own for a few hours: that all 337 `.smp` files are
read through `tilesa01.til`.** The reasoning and the correction are both below, because the way that
conclusion survived matters more than the conclusion did.

### Which tileset a map is read through

**Observed in a local binary, 2026-09-17.** A map file does not record its tileset, and the answer
is not a function of the map's class either.

| Map class | Files | Tileset | Declared by |
| --- | ---: | --- | --- |
| World — `.scn`, `.lgd`, `.map` | 29 | `tilesb01.til` | `maptileset`, once in `START.GS` |
| Combat — `.smp` | 337 | **per encounter**; 15 distinct tilesets across the corpus | `mapfile` + `tileset` in each encounter's dictionary |

**A combat map's tileset belongs to the encounter that loads it, not to the map.** Each encounter
script defines the two as adjacent keys:

```text
/mapfile"map/aicave.smp"def   /tileset"til/aibldg01.til"def
/mapfile"map/fienc1.smp"def   /tileset"til/cavelava.til"def
/mapfile"map/waming.smp"def   /tileset"til/cavewatr.til"def
```

Extracted from all 1,700 `gs.mpq` members — 1,699 extractable, the other being the archive's own
`(listfile)` — by taking every `/mapfile` definition and the `/tileset` definition nearest it in the
same member. Definitions may be string literals or **procedures**, and both are folded in:
`aimult.gs` writes `/tileset{dungeon_id getdungeonstrength 3 le{...}...}` whose three branches all
yield `aibldg01.til`, and `genchaos.gs` writes a `terrainsprites`-keyed selector yielding
`chbldg01.til` or `cavelava.til`. The table is `COMBAT_TILESET_BINDINGS` in `tile.rs`, and
`tools/extract_map_tilesets.py --rust` regenerates it byte-identically, so it is derived data anyone
can check rather than a list to be trusted.

**Comments are honoured, and they matter.** Gamescript members use bare `CR` line endings, so a
`;` really does comment to end of line even though a whole procedure looks like one line to a tool
that splits on `LF` only. An earlier version of this extraction did not strip comments and read
bindings out of **disabled code** in seven members — including `wilderness_land.gs` and
`wilderness_sea.gs`, whose `chcave.smp`/`cavewatr.til` pair is commented out *precisely because*
those are the outside-combat-encounter files. Honouring comments takes the multi-valued count from
26 to **22** and the undeclared-cell count from 880 to **466**.

### A second binding form: the runtime selector

**Observed in a local binary, 2026-09-17. An earlier version of this document said this form did
not exist, which was wrong.** That search looked for `/tilesets[`; the form actually ships as a
procedure plus a **separately named** array:

```text
/tilesets{ ... tiles 0 get ... currentterrainsprite getterrainspritelocation
           8 mod 2 eq{pop tiles 2 get}if ... }/dummy currentdict replace
/tiles["til/cavewatr.til" "til/cavecrys.til" "til/cavelava.til" "til/aibldg01.til"]replace bind def
```

41 members define `/tilesets`, 40 define `/tiles[`, 40 define `/maps[`. So **one** encounter selects
among up to four tilesets at runtime from a sprite's map location — a second and independent reason
a combat map has no single tileset, and a stronger one than the several-encounters case.

**The read is coarse, and it is recorded separately rather than merged.** Every tileset in a
member's `/tiles[...]` is listed for every map in its `/maps[...]`, minus anything already declared,
in `COMBAT_TILESET_ARRAY_CANDIDATES`. The arrays cannot be zipped positionally with confidence: the
`/mapfiles` and `/tilesets` procedures branch on *different* predicates — `4 mod 0`, `4 mod 2`,
`8 mod 7` against `8 mod 2`, `8 mod 4`, `8 mod 6`, `8 mod 7` in `waming.gs`, with different branches
commented out in each — only **31 of 40** members have equal-length arrays, and **5 of 40** disagree
with their own literal pair at index 0. Evaluating the predicates needs
`getterrainspritelocation`, a runtime value.

**Why it is not folded into the declared bindings.** It would buy **no** coverage and cost
precision. All 43 maps named in a `/maps[...]` array are *already* declared by a literal pair, so
the coarse reading resolves **zero** additional maps; and it widens **42 of those 43** across more
than one tileset *rule class*, standing `ruins01.til` (81.3% on `demina.smp`) beside the declared
`debldg01.til` (98.3%) as an equal. Constraint scoring cannot adjudicate — mean best satisfaction is
95.33% declared, 94.58% positionally zipped, 95.57% crossed, all within a point, largely because
many array entries are rule-identical art variants. So this is a judgement, recorded as one.

**Reporting is precise; refusing is permissive.** `--map-tileset-for` prints the declared binding
and, on a separate `also-reachable` line, the coarse set marked as such. The paint gate tests
against the **union**, because a tileset the engine may genuinely reach at runtime must not be
*refused* — refusing the engine's own answer is the bug that shipped on this branch once already.

**One form that really is absent, and one degeneracy.** There is no *randomised* selection left: the
only mention of randomising is a 2025-06-12 changelog comment in `DUNGEONS5.gs` recording that it
was **removed**, *"because randomized tilesets do not play well in multi-player games, whereby a
desync will occur"* — that comment describes a deletion and is not evidence of a live form. And the
`getdungeonstrength`-keyed procedures are **degenerate**: `aimult.gs`, `chmult.gs`, `demult.gs` and
`barrows.gs` each branch on dungeon strength and return the *same* tileset from every branch, so the
conditional is vestigial. One dangling reference exists: `dwl.smp` is paired with `cavetile.til`, and
neither the map nor the tileset ships.

**The same map may have several tilesets, and that is a finding rather than a gap.** 22 of the 172
bound maps are multi-valued: `chcave.smp` is drawn through `cavelava.til`, `chbldg01.til`,
`cavewatr.til`, `ruins01.til` and `cavecrys.til` by five different encounters — three quest
encounters use `ruins01`, `genbeast.gs` uses `cavewatr`. There is no single right answer for such a
map, and `--map-tileset-for` reports `ambiguous:` with every candidate rather than picking one.

**Coverage is partial and is not padded.**

| | Maps |
| --- | ---: |
| Installed `.smp` | 337 |
| Bound by a gamescript encounter | **169** (147 single-valued, 22 multi-valued) |
| No binding found — **unresolved** | **168** |

`--map-paint-terrain` refuses on an unresolved map rather than defaulting. **Scoring cannot rescue
them**: the 26 shipped tilesets collapse to only **16 distinct rule sets**, and 110 of the 168
unresolved maps have *sixteen* tilesets tied within one percentage point of the best fit. There is
nothing for a best-fit rule to discriminate on. What would settle it is the other binding forms, or
a `.gs` member this extraction reads too narrowly.

**The measurement that decides it.** Run `cargo run --release --example smp_tileset_fit`:

| Scored against | Maps | Satisfied |
| --- | ---: | ---: |
| The gamescript's own tileset | 169 | **370,254 / 388,910 = 95.20%** |
| `tilesa01.til`, the retracted rule | 169 | **32,842 / 389,376 = 8.43%** |

Per-map spot checks: `aicave.smp` 84.3% against `aibldg01.til` versus 3.3% through `tilesa01.til`;
`licave.smp` 100% versus 13.4%; `decave.smp` 94.1% versus 21.3%. The gamescript tileset wins on
**166 of 169** maps.

**The control that makes it airtight needs no scoring at all.** `aicave.smp` is 48x48 and uses
exactly the slots `0..63`. `aibldg01.til` — the script's pairing — declares `TILES=16,4`, a 64-slot
atlas. `tilesa01.til` declares 624 slots and points at `tilesb01.lbm`. Painting `aicave.smp` through
`tilesa01.til` writes slot 392 into a map whose atlas has 64 slots. And `pathwoods.smp`, one of the
three maps the scripts *do* pair with `tilesa01.til`, scores 98.5% against it — so the table is not
merely "never `tilesa01`".

### What `combattileset` actually is

**The measurement was right and the inference was wrong, which is the part worth recording.**
`START.GS` line 75 is `"til/tilesa01.til"combattileset`, and `combattileset` really does appear
**exactly once in all 1,700 `gs.mpq` members** — extracted and counted twice, by two parties, with
zero extraction failures. What does not follow is that it is the tileset of the shipped `.smp`
files. A corpus comment in `wilderness_land.gs` and `wilderness_sea.gs` states the rule outright:

```text
; WHEN 'mapfile' AND 'tileset' ARE UNDEFINED, YOU GET AN OUTSIDE COMBAT ENCOUNTER.
```

So `combattileset` is the default for an encounter that names **no** map — the engine generating
open-field combat. An encounter that names a `.smp` names a tileset beside it. `lomse.exe` exports
`setterrainspritemapfileproc` alongside `setterrainspritetilesetproc`, which is the engine asking
the script for both, per encounter.

**How the wrong conclusion survived every check, which is the reusable part.** Two independent
reviewers reproduced every published percentage — 35.41%, 236/263, 47.88% at +336, rank 70 of 624,
8.11%, 94.11% — confirmed the scorer's column order and its `~`/`|` handling, confirmed the shipped
tileset list, extracted every gamescript member, confirmed the single `combattileset` occurrence,
and **endorsed the wrong conclusion.** Every number was right. The inference from them was not.
*Reproducing a figure says nothing about whether it means what you think*, and agreement between two
parties who share an assumption is not evidence about the assumption.

The structural reason no test caught it: **every assertion was against a hardcoded literal**, so the
suite could only fail on the code disagreeing with the constant, never on the constant being wrong.
Four mutations all killed tests while the rule was wrong for the majority of the corpus. The fix is
not a better test of the constant, it is an instrument that reads the corpus —
`examples/smp_tileset_fit.rs`, which now exits non-zero if it scores no maps, if a map class is
missing, or if satisfaction falls below a floor that the retracted rule's 8.43% would trip.

### The filename rule: a real correlation, and still not the mechanism

**Corrected twice, in opposite directions.** An earlier version of this document filed a
per-faith/per-location filename rule as **Refuted** on the grounds that its fit was an artifact of
atlas size. That refutation was wrong — combat maps really do each have their own tileset — and it
is worse than leaving the question open, because a wrong refutation stops the next reader looking.

But the filename is **not** what selects the tileset, and the measured agreement is the reason:

| Measurement over the 263 faith-prefixed `.smp` | Count |
| --- | ---: |
| `{faith}bldg01.til` merely *declares* every slot the map uses | 236 |
| `{faith}bldg01.til` *is* the tileset the gamescript pairs the map with | **41** |

The 236 figure — the one this document published — measures **declaration coverage**, not the
pairing, and that conflation is what made the rule look causal. The scripts routinely cross the
faiths: `licave.smp` pairs with `libldg01.til` *and* `wabldg01.til`, and `eacave.smp` with
`libldg01.til` and `ruins01.til`. A faith-prefix rule would be wrong about 222 of 263 maps.
Both figures are now emitted by the committed instrument rather than asserted here.

**Also corrected: "the other 24 `.til` members are the terrain editor's palette, not map
tilesets."** False. `cavelava.til` alone is named by about 60 `gs\dungeons\…` members, and 15 of
the 26 shipped tilesets appear in combat-map bindings. The narrow claim that nothing feeds them to
`combattileset` is true and verified; the conclusion drawn from it was not.

### Refuted: a constant slot offset

If `.smp` slots indexed the atlas from a different origin, some offset should align the semantics.
Sweeping all 624 offsets over a stride-17 sample of 20 maps gives a **broad hump, not a peak**: the
maximum is 47.88% at offset +336, offset 0 ranks 70th of 624, and the median is 2.77%. The hump has
a cause rather than a meaning — offsets near +336 shift combat-map slots into the three `free move`
blocks at `480..=623`, which are the most permissive region of the atlas: **2.33** wildcard (`*`)
columns per `TILE=` row against **1.67** for slots `0..=479` in `tilesa01.til`, and 2.33 against
1.64 in `tilesb01.til`. A permissive region is not an alignment.

*(An earlier version of this paragraph published 3.00 against 1.92. Those numbers do not reproduce;
two independent measurements agree on 2.33 and 1.67. The conclusion is unaffected and the arithmetic
was not checked.)*

### Withdrawn: the `loadsubmappages` lead

An earlier version of this document offered the two 320x624 pages `til\ttype01.lbm` and
`til\thite01.lbm`, loaded by `loadsubmappages`, as the likely explanation for combat maps satisfying
only 8.11% of their tileset's constraints. **There is no such residue to explain.** The 8.11% was an
artifact of scoring every `.smp` against the wrong tileset; scored against the gamescript's own, the
figure is 95.20%, in the same range as the world maps' 93.43%. The lead was filed against a
non-problem and is withdrawn as an explanation.

The pages themselves are real and their geometry is still worth recording, as an unexplored
observation with no claim attached: both are 320x624 8-bit images, and those dimensions factor onto
the 16x39 tile atlas exactly — 624 = 39 rows x 16, 320 = 16 columns x 20 — giving each of the 624
atlas slots a 20x16 block. The blocks hold terrain-type ids, and slot 392's block is uniformly
water. What reads them is **Unknown**.

**Shipped world maps are not fully consistent with their own tileset, and this is why some paints
refuse.** 5.9% of `.scn` cells and 9.5% of `.lgd` cells hold a tile that does not satisfy its own
declared constraints (**Observed in a local binary, 2026-09-17**; only `Denmor64.scn` and
`Kelmor32.scn` are clean). The failures cluster on roads: a plains cell with road to its west and
south-west has no tile in the plains block that accepts it, so a paint whose region or ring includes
such a cell refuses with `no tile of terrain 6 accepts the neighbourhood`. Whether the authors used
`forcetexture` freely, or the engine only enforces constraints during a paint and not as a stored
invariant, is **Unknown** — but the consequence for a writer is concrete and is the reason the
success rate above is 76% and not 100%.

`--map-set-terrain` is unchanged and still offered: it writes one cell, `forcetexture`-style, and it
is the only one of the two that needs no tileset at all. What remains open on [issue
#4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4) is the interior draw (permanently
unreproducible) and why shipped world maps violate their own constraints. **Which tileset a `.smp`
uses is answered for 169 of 337** — per encounter, from the gamescript's `mapfile`/`tileset` pairs.
What stays open is the remaining **168**, for which no binding has been found and best-fit scoring
cannot discriminate.

**`--map-fill-terrain` and `--map-create` lay a field that violates its own tileset, and this is
known and left alone.** Both use `TERRAIN_TYPES.base_tile`, which for terrains 2, 3, 4, 5, 6, 7 and 8
is the block's index-15 slot — the *shore* tile. Tile 15 is `~6` on all four cardinals, so
`--map-create 9 9 6` writes 81 cells each of which declares that none of its neighbours is plains,
in a field that is entirely plains: a grid of isolated islands by the tileset's own rules. Water is
the accident that looks fine, because its recorded base tile 392 happens to be the interior slot.

It is deliberately **not** changed here. `fill_terrain` reproduces `clearmap`, and `clearmap`
genuinely forces one tile into every cell — that equivalence is what three probe tools rely on to lay
a known background, and "fixing" the fill would silently change what a probe measures. The right
follow-up is a separate `terrain_interior_tile()` accessor for fills, now that a tileset can be
loaded; painting the same rectangle afterwards also repairs it, since a whole-map paint is exactly
"fill with correct interior tiles".

**`TERRAIN_BASE_TILES` stays as it is, and the reason is not the one this document previously gave.**
Its only consumers — `engine_probe.py`, `terrain_rings.py`, `emit_terrain_tables.py` — want *a tile
`getterrain` answers with this terrain* so `clearmap` can lay a background, and both 392 and 63
satisfy that. So there is no live hazard, and changing water's 392 to its anchor 63 would make the
probe's background a constraint-violating field, which is strictly worse. The anomalous rows are
plains, desert, mountain, happy plains, ice, swamp and lava — not water.

**There is a create-from-scratch mode, `--map-create`, and the engine accepts what it produces.**
It was held back until the [`mapload` run](#engine-acceptance-measured) because three fields would
otherwise have to be invented; instead it composes only byte patterns the engine was observed
writing — header word `0x6f` (engine-written at six sizes, and rewritten by the engine on every save
anyway) and the exact eight-byte empty trailing section an empty save produced. It mints nothing.
The engine loaded both a 64x64 and a 96x64 created this way and re-saved them **byte-identically**.

The shipped GS5R3 editor still generates richer maps, from 32 to 1024 in steps of 32, and generating
there and editing here remains the better route for anything with content in it — `--map-create`
makes a uniform grid, not a world.

### Safety

The loose `map/` directory has **no backup**, so:

- every editing command takes an explicit output path and there is **no in-place mode**;
- writing over the input is refused, by canonical path, so `m.scn` and `./m.scn` are both caught;
- the output is opened `create_new`, so an existing file is never truncated;
- the encoded bytes are **re-parsed and the edit read back** before anything is written, exactly as
  `--set-imp-placement` does — a map that cannot be read back is a map that is not emitted;
- a refused edit leaves no partial file behind.

### Editing commands

```sh
lom-asset-viewer --map-roundtrip '/path/to/English/map'
lom-asset-viewer --map-roundtrip MAP.scn

lom-asset-viewer --map-set-tile      IN.scn X Y TILE_SLOT   OUT.scn
lom-asset-viewer --map-set-terrain   IN.scn X Y TERRAIN     OUT.scn
lom-asset-viewer --map-set-elevation IN.scn X Y VALUE       OUT.scn
lom-asset-viewer --map-fill-terrain  IN.scn TERRAIN         OUT.scn
lom-asset-viewer --map-place-sprite  IN.scn X Y SPRITE_TYPE OUT.scn
lom-asset-viewer --map-remove-sprite IN.scn INSTANCE_ID     OUT.scn
```

`TERRAIN` is a number `0..10` or a `gs\maplib.gs` name with or without its `tt_` prefix, so
`1`, `tt_water` and `water` are the same thing. `TILE_SLOT` is a raw atlas index. It is *not* range-checked against the **tileset**, because the
atlas size comes from the active `.til` file and `tilesb01.til`'s 624 slots are one tileset's answer
rather than the format's. It *is* refused above `1024`, as a **conservative guard**: a fat-fingered
`3920` for `392` is the realistic input, and it used to be written straight into the cell.
(**Corrected, 2026-09-17:** this cap used to be justified as the tag word's field width — "corpus
tag bits `10..22` are zero". The engine's tile field is the whole low `u16` and it is signed, so the
format can express far more and only the tileset limits it. The cap stays; its justification was
wrong.)

A new sprite takes the next `instance_id` above every id in the file, starting at 200 on an empty
map.

**A removed id *is* reissued by the next invocation.** Each command reads one file and writes one
file, so the high-water mark that holds a freed id back lives only as long as that process. In
practice:

```
--map-place-sprite  a.scn 0 0 470 b.scn   -> instance:200
--map-place-sprite  b.scn 1 0 470 c.scn   -> instance:201
--map-remove-sprite c.scn 201       d.scn
--map-place-sprite  d.scn 2 0 470 e.scn   -> instance:201   <- reissued, different cell
```

This is a limitation, not a bug to work around: the format has nowhere to persist a high-water mark,
and inventing a field would break the copy-never-mint rule above. It matters because other files
reference objects by id, so a reused id can silently re-point an outside reference at a different
object. **If something outside the map references a sprite by instance id, do not remove-then-place.**

### Worked example

```sh
$ lom-asset-viewer --map-set-terrain base.scn 10 20 water out.scn
note: this writes one cell, like the editor's forcetexture. ...
wrote	out.scn	159173 bytes
cells-changed	1
set-terrain	(10, 20)	terrain:1	tile:392

$ lom-asset-viewer --diff-maps base.scn out.scn
cell	10	20	2570	0x00000100	0x00000188	3	3
cells	differing:1	of:16384
tail	left:28085	right:28085	first-difference:none
```

Cell index 2570 is `20 x 128 + 10`, which is the corrected y-major packing arriving at the byte
level rather than only in the parser.

**Place-then-remove returns the original file byte for byte**, on a real 128x128 shipped map as
well as in fixtures — which is the same behaviour the 2026-09-17 engine probe observed from the
game itself, reproduced by a tool the game never ran.

## The terrain-sprite-type table

**Observed in gameplay, 2026-09-17.** `terrainsprites` is a dict keyed by name — shipped script
reads `terrainsprites /barrow get` — and `forall` enumerated all **197** of its entries: **178** are
a plain name-to-id pair, and the rest are arrays and procedures.

```
0 castle1    43 tower1    184 cyccave    233 llvil
5 cave       46 tree1     193 trllcave   234 ffvil
16 livil     50 wavil     217 cave1      236 devil
17 minec     54 tree4     228 mines      237 lirock
```

**This is why the `sprite_type` field at record `+28` was unusable.** The id is assigned in script
execution order across 536 `addterrainspritetype` call sites in `gs\tree.gs` and `gs\tree2.gs`,
many of them computed at runtime from faith and direction, so nothing in the file format says which
id is a keep and which is a tree. The table is the missing half, and `--map-place-sprite` now takes
a name:

```sh
lom-asset-viewer --map-place-sprite IN.scn 10 20 castle1 OUT.scn
lom-asset-viewer --map-sprite-types
```

A raw id is still accepted and deliberately **not** range-checked against the table: ids above it
are runtime registrations, which is how the probe's own type 470 came to exist, and refusing those
would refuse a legitimate record shape.

**The table is profile-specific.** These ids come from the working GS5R3 script set; a different mod
registers different types in a different order and every id shifts. Re-run the probe against any
profile whose maps you intend to edit. The CLI prints that warning with the table rather than only
here.

The nine array entries — `keep_array`, `vilg_array`, `great_temple_array`, `leader_ttype_array`,
`special_array`, `special_unit`, `special_unit2`, `terrainspritearray`, `combatterrainspritearray` —
are the **per-faith** tables, eight entries each, which is exactly the set the random map generator
was observed placing. Enumerating them is the obvious next probe and would complete the table.

These are identifiers from the game's own scripts, the same class of measurement as the operator and
terrain-type names already recorded here — not shipped content.

## Engine acceptance, measured

**Observed in gameplay, 2026-09-17 (the `mapload` probe).** Round-trip identity shows this
project's writer matches the engine's *writer*. It says nothing about the engine's *reader*, and
until this run no map this project produced had ever been loaded by the game.

`gs\hotkey.gs` supplied the instrument: `loadscenariomap` takes a filename and **returns a
boolean** which the shipped editor tests. Acceptance is a value the engine hands back.

| Rung | File | Loaded | Engine reported | Echo vs input |
| ---: | --- | :---: | --- | --- |
| 0 | engine's own save (control) | yes | 64x64 | 4,096 cells differ — see below |
| 1 | our re-encode of `URAK.scn`, byte-identical to it | yes | 128x128 | 1 byte |
| 2 | that map with three terrain cells changed | yes | 128x128 | 1 byte |
| 3 | that map with a sprite placed | yes | 128x128 | 1 byte |
| 4 | that map with the border bit on an interior 4x4 | yes | 128x128 | 17 bytes |
| 5 | created from nothing, 64x64 | yes | 64x64 | **identical** |
| 6 | created from nothing, **96x64** | yes | **96x64** | **identical** |

Rung 2's three edited cells read back as terrain **1, 8 and 5** — water, lava and snow, exactly what
was written. The engine did not merely accept the file, it read the edits correctly.

Rung 3 matters most for the writer's one honest compromise: the placed-sprite record, including the
minted `+24` attribute whose value contradicts the corpus reading, survived **byte-exactly**.

Rungs 0 and 1 are the controls that make the rest readable. Rung 0 proves `loadscenariomap` works at
all — without it, a rejection at rung 2 could not be told from a broken instrument. Rung 1's bytes
are *equal* to a shipped map's, so anything but success there would have been the harness.

**Non-square maps work.** No shipped or engine-generated map has ever been non-square; the engine
loaded a 96x64 and reported its dimensions back correctly.

### The header word at `0x00` is engine output, not map input

**Observed in gameplay, 2026-09-17.** `URAK.scn` carries `0x6c`. Loading it and saving it straight
back out produced `0x6f` — and `0x6f` is what the engine writes for everything. It does not preserve
what it read.

That is the single differing byte in rungs 1 through 4, and it **retires the "stored tileset
selector" reading in that form**: whatever selects a tileset, it is not this word being carried
through a save. It is also why the created-from-nothing maps came back identical — they were already
written with `0x6f`.

**Sample: one transition, from one editor state.** `0x6c → 0x6f` four times (rungs 1–4, all derived
from `URAK.scn`) and `0x6f → 0x6f` three times (rungs 0, 5, 6). What is established is that the
engine does not preserve what it read. What is **not** separated is whether it always writes `0x6f`
or writes whatever tileset the editor currently holds — nor whether it *reads* `0x6c` to configure
itself and merely serialises a canonical value. The separating experiment is one rung: load a `0x4f`
sub-map, save, and see whether the echo tracks the source.

### `resetvisibility` clears tag bit `0x00800000`

> **Superseded in part, 2026-09-17.** Reading this as a *bit* is retired: it is the value `0x0080` of
> a signed 16-bit field at cell offset `+2`, and `resetvisibility` writes that field as a whole word
> at `0x004a90e0`–`0x004a91a1`. The gameplay measurements below are unaffected and the disassembly
> explains why the two runs looked contradictory — the perimeter write is guarded on map object
> `+8`. See [the field is a signed scalar](#the-2-field-is-a-signed-scalar-not-a-bitfield).

**Observed in gameplay, 2026-09-17.** The two runs bracket the behaviour:

| sequence | cells whose `+2` field holds `0x0080`, out of 4,096 |
| --- | --- |
| `clearmap` then save, **no renderer calls** | **set** |
| `clearmap`, paints, then `rebuild3dmap resetvisibility rendermap refreshdirty`, then save | **clear** |

So something `clearmap` does sets the value, and something on the render path clears it. **There was
never a contradiction** with the earlier "0 of 4,096" reading: that run was measuring the state
after a rebuild.

> **Refuted in a local binary, 2026-09-17: it is not `forcetexture` that sets it.** This paragraph
> used to read "`forcetexture` — which `clearmap` calls on every cell — *does* set the bit". The
> measurement was right and the attribution was wrong. `forcetexture`'s worker at `0x004a5ed0`
> touches exactly two cell operands in the whole function — a 16-bit read at `0x004a5efe` and a
> 16-bit write at `0x004a5f06`, both of lane `+0` — and a 16-bit store to `+0` **cannot** alter
> `+2`. `clearmap` does more than call `forcetexture` on every cell, and the write came from
> something else it does. **Inferred:** the grid initialiser at `0x004a50e6`, which fills every
> cell's `+2` with map object `+0x48`; `newmap` reaches the allocator at `0x004a4f20`, but no trace
> from `clearmap` to the initialiser has been followed.

**Isolated 2026-09-17 to a single call.** Five fresh maps, one renderer call each — fresh because
once the field is zeroed it stays zeroed:

| map | sequence | `+2` holds `0x0080` |
| --- | --- | ---: |
| `zf0.scn` | `clearmap`, save | 4096 / 4096 |
| `zf1.scn` | `clearmap`, `rebuild3dmap`, save | 4096 / 4096 |
| **`zf2.scn`** | `clearmap`, **`resetvisibility`**, save | **0 / 4096** |
| `zf3.scn` | `clearmap`, `rendermap`, save | 4096 / 4096 |
| `zf4.scn` | `clearmap`, `refreshdirty`, save | 4096 / 4096 |

**`resetvisibility` is the one.** Not `rebuild3dmap`, which an earlier draft of this section
proposed — that hypothesis was wrong and the isolation says so.

**What the operator's name suggests, and what it does not establish.** `resetvisibility` zeroing a
per-cell value invites reading it as visibility state, and that would reframe the corpus pattern
neatly — the value sits on exactly the perimeter ring of 146 `.smp` files, which reads very
differently as visibility than as a texture flag.

But that is an inference **from the operator's name**, and a name is not evidence about a field. The
call could as easily clear a generic dirty or cache value as a side effect. This project has been
caught inferring semantics from plausible names before, so the recorded facts are the narrow ones:
something `clearmap` does sets the value, `resetvisibility` zeroes it, and the meaning stays
**Unknown**. Separating "visibility" from "scratch state that the visibility pass happens to reset"
needs an experiment on visibility itself.

The disassembly has since added real weight on the *shape* rather than the meaning — a signed
scalar, read sign-extended 11 times, never masked, interpolated against a base of `0x80` — which
kills the flag reading outright without settling what the scalar measures. See
[the `+2` field is a signed scalar](#the-2-field-is-a-signed-scalar-not-a-bitfield).

What is still **not** established is whether a load or a save touches the value independently. Every
echo save in the `mapload` run happened after the renderer block, so rung 4's sixteen cleared
interior cells are equally explained by `resetvisibility` running in that block. The loader itself
reads the cell grid as opaque bytes — one `fread` per 64 cells at `0x004a5364` — so nothing in the
load path singles this field out.

The [refutation of the forced-texture flag](#tag-bit-0x00800000--refuted-as-a-forced-texture-flag)
above still stands, and the disassembly has since replaced its evidence with something stronger.
The corpus argument was that the value sits on exactly the border ring and on no interior cell. The
binary argument is that `forcetexture` writes only lane `+0`, 16 bits wide, at `0x004a5f06`, and so
cannot touch `+2` at all — which settles it without reference to operation order. The
"`forcetexture` never sets it" step that this section previously called "an artefact of operation
order" turns out to have been **correct**, for a reason neither run could see: the two operations
write different fields.

**Writer guidance: preserve the whole `+2` field, never clear it.** The corpus shows it on disk in
one place — 27,448 cells across 146 `.smp` files, exactly their perimeters — and the probe only ever
used `loadscenariomap`/`savescenariomap`, never the `.smp` path. Whether a `.smp` load-and-save
preserves the ring is **unmeasured**, so an editor that dropped it could be destroying real data.
`set_tile` and `fill_terrain` preserve `tag & 0xffff0000`, which is the whole field rather than the
one bit they used to keep.

### `setterrain` transition tiles: one offset table, one anchor per background

**Observed in gameplay, 2026-09-17 (the `terrainrings` probe).** Eleven terrains painted onto eleven
backgrounds, 121 rings. The earlier one-background measurement produced an eight-tile table; all
eleven turn out to be **the same table** plus a per-background anchor.

| direction | offset |  | direction | offset |
| --- | ---: | --- | --- | ---: |
| N | −13 | | NW | +3 |
| S | −14 | | NE | +4 |
| W | −11 | | SW | +2 |
| E | −12 | | SE | +1 |

```
background  anchor   ring (N  S  W  E  NW NE SW SE)
  6 land      15       2  1  4  3  18 19 17 16
  1 water     63      50 49 52 51  66 67 65 64
  2 desert   111      98 97 100 99 114 115 113 112
  3 mountain 159     146 145 148 147 162 163 161 160
  4 happy    207     194 193 196 195 210 211 209 208
  5 ice      255     242 241 244 243 258 259 257 256
  7 swamp    303     290 289 292 291 306 307 305 304
  8 lava     351     338 337 340 339 354 355 353 352
```

Subtract the anchor and every row is identical, for **eight of eight blending backgrounds** — not
eleven; three produce no uniform ring and are excluded below.

**The anchor is defined as `SE − 1`, so read the strength of this carefully.** One free parameter per
background is fixed by its SE tile. That leaves the other **seven** offsets across **eight**
backgrounds — **56 constraints satisfied by the same seven numbers**. That is what makes it a finding
rather than a restatement: eight independent tables could each be a coincidence, one table that
regenerates all eight cannot.

The independent confirmation is that the anchors then land on `15, 63, 111, 159, 207, 255, 303, 351`
— a contiguous arithmetic run of stride **48**, every one congruent to **15 mod 48**. Nothing in
"anchor = SE − 1" imposes an arithmetic grid.

**What that does not establish** is that the whole atlas is partitioned into 48-tile terrain blocks.
Eight transition motifs spaced 48 apart is a statement about those motifs. An atlas parser must not
classify every 48-slot region as a terrain block on this evidence.

**Water's anchor is not its representative tile.** `terrain_type_base_tile(1)` is 392, which is 8
mod 48; its blending anchor is 63. The two are different things, and a painter that used 392 as an
anchor would take water transitions from the wrong block.

#### The three backgrounds that do not produce a uniform ring

| background | behaviour |
| --- | --- |
| **0** `tt_dirt` | **No transitions at all** — every ring cell keeps tile 175 |
| **10** `tt_impassible` | **No transitions at all** — every ring cell keeps tile 469 |
| **9** `tt_road` | Ring depends on the **painted** terrain; only the edges change, corners keep 459 |

Both no-transition backgrounds are also the two whose representative tiles are **not** on the block
grid — 175 and 469 are 31 and 37 mod 48. That is suggestive, not established: two cases is not a
rule.

#### Road is not a direction table in either role

As a **painted** terrain, `tt_road` is **ragged along every edge** on all seven backgrounds where it
blends — the ring is not one tile per direction but varies along the run. As a **background** it is
the `PerPaintedTerrain` row above. A painter must special-case road both ways.

The measured behaviour is committed as `TRANSITION_RING_OFFSETS`, `TERRAIN_TRANSITIONS` and
`transition_ring()` in `spikes/asset-viewer/src/map.rs`, with a test that regenerates all eight
measured rings from the single table.

**The generator is `tools/emit_terrain_tables.py`, and `--check` re-derives the constants from the
saved maps and fails on any difference.** That exists because a reviewer pointed out the original
generator was a throwaway script: "generated rather than transcribed" was then an unverifiable
claim, and the same transcription slip could have been copied into both the constants and the test
fixture meant to catch one. The committed generator re-derives independently — and with a *stricter*
rule, requiring every ring cell to agree rather than the eight sampled midpoints — and reproduces
the constants exactly.

```sh
lom-asset-viewer --map-transition-rings
```

### A painted region's interior is a random draw, not a base tile

**Observed in gameplay, 2026-09-17.** `setterrain` picks a painted region's interior from an
eight-member family, and picks it **randomly**.

The family is `384 + 8k`, where `k` is the terrain's block index `(anchor − 15) / 48` — so terrain 6
draws from `384..391`, water from `392..399`, desert from `400..407`, and so on in anchor order.
Verified for all eight blending terrains on all eleven backgrounds.

**The randomness is the load-bearing part.** The same experiment run twice — terrain 6 on a tile-15
background, the same 3×3 at the same coordinates, two separate attended runs — gave centre tile
**385** once and **390** the other time, while the transition ring was **byte-identical across both
runs**. That double result is worth more than either half: the ring is confirmed by independent
replication, and the interior is confirmed to be unreproducible.

So a writer cannot reproduce an interior, and should not try. `--map-set-terrain` writes one
representative tile, which is a legitimate if less varied choice. `--map-paint-terrain` no longer
does: it draws the interior from the same family the engine does — the `.til` file declares those
eight slots as equally valid, which is [why the draw is random](#the-tileset-declares-the-whole-blend-rule)
— and chooses among them by `TileSelector`, reporting the count it could not determine.

**This corrects two earlier claims of mine, both in the same direction.** An earlier section said
`base_tile` was non-invariant "for terrain 6" and blamed the *background*, citing a tile-392
background as the contrast. `zr1.scn` **is** a tile-392 background, and terrain 6's blob centre there
is 391, not 15. `base_tile` is non-invariant for all nine blending terrains, and the variable is the
painted region's **extent**, not the background: `TERRAIN_BASE_TILES` was measured with **single-cell**
`setterrain`, which has no interior at all. The family was also written as `385..391`, which is wrong
at both ends for the run it came from — 384 occurs and 385 does not in `zr6.scn`.

A reviewer caught the contradiction by reading the committed artifacts. The run-to-run difference,
which explains it, came out of checking that reviewer's numbers against my own.

The reverse direction is unaffected: `getterrain` on a representative tile answers its type, and the
blend background read back as expected on all eleven rows. What the writer does with the table —
forcing one representative tile of a type into a cell — remains right.


### The tileset declares the whole blend rule

**Observed in a local binary, 2026-09-17.** The 56-constraint offset table above is a *lookup into
data the game ships*. `tilesb01.til` declares, for every atlas slot, which terrain a cell holding it
belongs to **and what it requires of all eight neighbours**. `setterrain` is then one operation, not
two: set the region's terrain, and re-select a tile for every cell whose neighbourhood moved. The
"transition ring" is not a special case — it is the same re-selection applied to the cells just
outside the region.

The line format, which this repository has parsed since Stage 1 and was until now reading only two
columns of:

```text
;TILE= tilenum,self, n, ne,  e, se,  s, sw,  w, nw,    index
TILE=     15, 6,  ~6,  *,  ~6,  *,  ~6,  *,  ~6,  *,  15
TILE=     16, 6,  6,  6,  6,  6,  6,  6,  6,  ~6|9,  16
TILE=    384, 6,  6,  6,  6,  6,  6,  6,  6,  6,   0
```

`~` is *not*, `|` is *one of*, `*` is *don't care*. `self` is the cell's own terrain type. So tile 15
is literally "a plains cell none of whose four cardinal neighbours is plains", tile 16 is "plains
with non-plains only to the north-west", and tile 384 is "plains surrounded by plains".

#### Corrected: the declaration was already in reach and was being discarded

This is not a newly located file. `docs/game-architecture.md` has recorded the 26 `.til` members in
GS5R3 `pic.mpq` since Stage 1, and `docs/research-log.md` recorded `tilesb01.til`'s 624-slot atlas
and 11 terrain types. What was wrong is narrower and worse: **`spikes/asset-viewer/src/tile.rs`
parsed each `TILE=` line and kept only `tilenum` and `self`, throwing the eight neighbour columns and
the trailing index column away.** So the blend rule was measured in an attended engine run — 121
painted blobs, a human at the keyboard — while the same statement sat unparsed in a file the
repository had already opened. The lesson is not "find the file"; it is **read every column of a
format you claim to parse, or say in the parser which ones you are dropping and why.**

(A note for anyone working from a hand-off: a claim that no `.til` member could be found in the
archives is not one this repository ever made, and `gs.mpq`/`imp.mpq` are the wrong archives to have
looked in. The members are in `pic.mpq` under a `til\` prefix, which is why a bare `*.til` search
against the other two archives returns nothing.)

#### The direction convention: derived, not chosen

A `.til` column could mean either *"the neighbour in this geometric direction from me"* or *"the
direction from that neighbour back to me"*. The two are mirror images. **A wrong choice here
produces maps that look entirely plausible and are systematically flipped**, which is precisely the
failure mode that let the X-major cell order survive for months.

It is settled by the engine's own saved maps, not by which reading makes the arithmetic tidier.
Against `artifacts/engine-probe-captures/terrainrings-20260917`, over the eight blending backgrounds
× eight painted terrains × eight ring cells:

| reading | ring tiles satisfying their own constraints |
| --- | --- |
| column = geometric direction from the cell | **576 / 576** |
| column = direction from the neighbour back to the cell (mirrored) | **0 / 576** |

And each of the eight columns discriminates *individually* — every column has a nonzero failure count
under the mirrored reading — so this is eight independent confirmations, not one systemic fit.

The concrete line, in `zr6.scn`: the ring cell one step north of a painted blob holds tile **2**,
whose only non-plains column is `s`. The paint is to its south, which is where the paint actually
is. So `n` means `(0, −1)`, `s` means `(0, +1)`, and `y` increases southwards as everywhere else in
this writer.

**This is where the apparent "inversion" comes from, and it is not one.** Measured `W = −13 + 2 = −11`
gives tile 4 from anchor 15, and tile 4 declares a single *east* edge. That reads like a mirror until
you name the two things separately: the measured label `W` is *the ring cell's position relative to
the blob*, while `e` is *the direction from that cell to its own neighbour*. The cell west of the
paint has the paint to its east. Both are geometric; neither is inverted.

#### What the constraints reproduce, and what they cannot

Simulating "set the region's terrain, then re-select every disturbed cell" over all 121
background-by-painted rows of the run:

| outcome | cells | agree with the engine |
| --- | --- | --- |
| exactly one candidate tile | 1,888 | **1,888** |
| the cell's existing tile is still the only valid one | 196 | **196** |
| several candidates match equally | 253 | 44 — but the engine's tile is **within the candidate set in all 253** |
| no candidate at all | 512 | 0 — the engine writes one anyway |

**2,084 of 2,084 deterministic cells, zero mismatches.** That covers every transition ring cell and
every perimeter cell of every painted region — the whole of what the eight-entry offset table
covered, and the region perimeters besides, which it did not.

The 253 ambiguous cells are the interior draw. The 512 with no candidate are exactly road and
impassable: `tilesb01.til` gives `tt_impassible` slots `464..=471`, every one of which demands that
all eight neighbours also be impassable, so a rectangle of it has no legal boundary; and road's
slots describe a *network*, not an area.

#### The three things that decide a cell

Matching a neighbourhood against the constraints usually leaves exactly one tile. Where it leaves
several, the choice is made in this order, and **the order is the whole correctness of the
operation** -- getting it wrong produces maps that are legal by the tileset and visibly absurd.

**1. The map edge is read closed.** An off-map neighbour satisfies every constraint, including a
negated one, so along an edge the one-sided boundary tiles compete with the interior family on equal
terms. A lowest-slot tie-break then takes the *most* wrong legal option. Measured on a 9x9 map:

| paint | what the open reading wrote |
| --- | --- |
| whole-map water | a complete phantom coastline -- row 0 all tile 49, which asserts **land to the north**, row 8 tile 50, column 0 tile 51, column 8 tile 52, interior correctly 392 |
| whole-map road | **nine different road tiles** (450..457 and 459) for a uniform road field |
| whole-map impassable | a single tile 464 everywhere, rather than the 469 already there |

So candidates are taken with every off-map neighbour read as the cell's **own terrain** first, and
the open reading is used only if closing leaves nothing. Closed candidates are always a subset of
open ones, so this narrows a tie and can never invent a tile the open reading rejected. What the
engine actually reads past a map edge remains **Unknown** -- every `terrainrings` blob is interior,
so no saved artifact says -- but reading it closed is the choice that does not fabricate a coastline.

**2. A cell that already holds a valid candidate keeps it.** This is not a tie-break of convenience;
it is what the engine was observed doing. In `zr0.scn` a water blob painted onto dirt leaves ring
cell `(13, 5)` at tile `175` although `[31, 79, 127, 175, 223, 271]` all match it. Across the
captures this accounts for **196 cells, and reproduces all 196**.

**3. Otherwise the selector picks**, lowest slot by default or `--seed N`, and the cell is reported
as *drawn* rather than determined.

That gives four outcomes, and the difference between the middle two is the difference between
reproducing the engine and merely being legal:

| outcome | cells in the captures | agree with the engine |
| --- | --- | --- |
| one candidate | 1,888 | **1,888** |
| several, and the cell already held one -- *kept* | 196 | **196** |
| several, and the cell held none -- *drawn* | 253 | 44, by coincidence |
| none -- road and impassable | 512 | 0; the engine writes one anyway |

**A correction to an earlier version of this document.** It reported 2,084 deterministic cells as
"cells whose candidate set was a single tile". The total is right and the description was not: 196 of
them have between two and sixteen candidates and are reproducible because the engine leaves such a
cell alone. Worse, the figure was measured with a keep-current rule the shipped code did not have,
so for one revision the document was describing a procedure the tool did not implement. It does now.

The 253 drawn cells are **not** convertible by keeping — by construction they hold no valid candidate,
so there is nothing to keep. They are the genuine random draw, and the tool reports their count as
`drawn:` on every paint.

#### The ring is only the cells beside terrain that moved

A ring cell is re-selected because a neighbour's terrain moved. One whose eight neighbours all
stayed put has no reason to be touched, and the engine does not touch it.

A single global "did anything change" flag gets this right for an all-changed region and for an
unchanged one, and wrong in between. Painting plains over a rectangle that was already half plains
re-selected the whole rectangular ring, and a ring cell holding a non-lowest interior member moved
for no terrain reason: **tile 387 became 384 three columns away from the only cell whose terrain
changed**. The affected set is now the rectangle plus any cell outside it with a neighbour whose
terrain actually moved, which also means fewer cells are consulted and so fewer paints refuse.

#### What this explains that was previously unexplained

- **The random interior is no longer just an observation.** `TILE=384..391` are eight *identical*
  all-neighbours-plains lines. Nothing distinguishes them, so nothing can choose between them but a
  draw — which is why two runs of the same experiment gave 385 and 390 with a byte-identical ring.
  The observation and the mechanism now agree, and `384 + 8k` is a block of eight per terrain in the
  file rather than an inference from anchors.
- **The three backgrounds that "produce no uniform ring" are three different things, all declared.**
  `tt_dirt`'s slots 31, 79, 127, 175, 223 and 271 are `self = 0` with `*` in all eight columns, so
  every dirt cell has six equally valid candidates and the tileset forces none of them.
  `tt_impassible`'s slots demand all-impassable neighbours and so have no boundary member at all.
  Road's slots are a network. None of the three needed a special case in the measurement's sense;
  they fall out.

  **Falling out is not automatic, and this sentence used to be false.** It claimed a dirt cell
  "matches whatever it already holds and is never re-selected", which was the stated reason for
  deleting the `NoTransition` special case -- and nothing in the paint read the current tile, so a
  lowest-slot tie-break moved all sixteen ring cells of a 3x3 plains paint from 175 to 31. What
  makes it true is the rule in [the three things that decide a
  cell](#the-three-things-that-decide-a-cell): a cell that already holds a valid candidate keeps
  it. Six wildcard candidates then resolve to the one already there, and the ring is untouched
  exactly as `TERRAIN_TRANSITIONS` says it is.
- **Terrain 9 is `"free move"`** — roads. That is why `6|9` appears throughout the plains
  constraints and `~6|9` in its negations: a road counts as plains for blending purposes, which is
  what lets a road run through a field without cutting a shoreline into it.
- **Water's anchor 63 was never an exception.** 63 is water's block-index-15 slot, exactly like
  plains' 15 — `48 + 15`. What differs is `TERRAIN_BASE_TILES`, which records water's *interior*
  slot 392 where it records plains' *index-15* slot 15. Those were never the same kind of thing, and
  the engine's own table is simply inconsistent between the two rows.

#### The empirical measurement is retained, as corroboration

**The 56-constraint offset table does not become wrong. It becomes derived.** Fifty-six measurements
collapsing onto seven numbers is real evidence, independently obtained, and it agrees with the file.
`TRANSITION_RING_OFFSETS`, `TERRAIN_TRANSITIONS`, `transition_anchor`, `transition_ring`,
`road_background_ring` and `interior_tile_family` are all **kept**, and
`the_measured_offset_table_is_what_the_declared_constraints_produce` holds the two against each
other: it builds a tileset laid out the way a real terrain block is laid out, paints through the
constraint matcher, and asserts the result equals `anchor + offset` for all eight directions. If
either side drifts, that test fails.

What was *retired* is the offset table's role as the **paint's decision procedure**. The paint no
longer consults it; `--map-transition-rings` still prints it as the measurement it is.


### Painting a shipped world map is refused about 30% of the time

**Observed in a local binary, 2026-09-17.** The constraint matcher's `NoMatchingTile` refusal was
derived from the `terrainrings` captures and described there as "exactly the road and impassable
cases". Driving the planner from a UI, where a person drags a rectangle anywhere they like rather
than at coordinates chosen to work, made the **rate** visible for the first time. It is much higher
than "road and impassable" suggests, and it is not a defect in the tool.

`examples/paint_refusal_survey.rs` plans — never applies, never writes — one 3x3 paint of every
terrain the tileset draws at **every position the rectangle legally fits**: 126 x 126 = 15,876
origins on a 128-wide map, per terrain. Through `tilesb01.til`:

| map | terrains 0–8 refused | road, terrain 9 | impassable, terrain 10 |
| --- | ---: | ---: | ---: |
| `URAK.scn` | 43,714 of 142,884 — **30.6%** | 98 of 15,876 accepted | **0 of 15,876** |
| `Corlis.scn` | 38,466 of 142,884 — **26.9%** | 1 of 15,876 | **0 of 15,876** |
| `Avaeron128.scn` | 18,554 of 142,884 — **13.0%** | 1 of 15,876 | **0 of 15,876** |
| `fire.lgd` | 1,490 of 142,884 — **1.0%** | 15,540 of 15,876 accepted | **0 of 15,876** |

Every refusal in all four maps is `no-matching-tile`; no other kind occurred.

**Corrected, 2026-09-17: this was first published from a disjoint sample and called a population.**
The survey stepped by the rectangle's own side, testing 42 x 42 = 1,764 origins out of the 15,876 a
3x3 actually fits on, and the figure was quoted as the rate over *all* 3x3 paints. Re-run at stride
1 the rate is **30.59%** against the sample's 31.07% on `URAK.scn`, 26.92% against 27.73% on
`Corlis.scn`, 12.99% against 13.41% on `Avaeron128.scn`, and 1.04% against 0.93% on `fire.lgd`. So
the published number was not wrong — but it was not measured, and the stride is now 1 by default.
The old stride also skipped the last `width % side` columns and rows entirely, which is the map
edge, and the map edge is exactly where the untested off-map-neighbour assumption applies.

Two readings, and one that is not settled:

- **Impassable is unpaintable as an area, on real maps and not only in principle.** 0 of 63,504
  attempts across four maps. `tilesb01.til` gives `tt_impassible` eight slots, each demanding that
  all eight neighbours also be impassable, so a rectangle of it has no legal boundary anywhere. The
  engine fills those cells from the block regardless. This is now measured on shipped data rather
  than derived from the tileset alone.
- **The rate tracks how mixed the map is.** Within one map the nine ordinary terrains refuse within
  a few percent of each other; between maps the spread is 30:1. `fire.lgd` is 16,228 of its 16,384
  cells a single terrain — it is a nearly blank map — and `URAK.scn` has six terrains above 1,700
  cells each. What a refusal reports is a *neighbourhood* the tileset declares no tile for, and a
  mixed map has many more distinct neighbourhoods.
- **`fire.lgd` accepting 15,540 road paints is a lead, not a conclusion.** Road is refused almost
  everywhere on the other three. The uniform-field explanation is consistent with it and is not
  evidence for it; nothing here establishes the mechanism.

#### What decides how much of a painted map is a draw: the terrain already there

**Corrected, 2026-09-17.** An earlier version of this section claimed "a blank field is where
several interior tiles tie most often, so the emptier the map, the more of the result is a legal
draw", and quoted "roughly 17 unreproducible cells per paint on `fire.lgd` against 1.0 on
`URAK.scn`". The second figure matched no reading of the instrument, and **the mechanism is refuted
by the instrument's own output.** Drawn cells per accepted paint, at stride 1:

| | `fire.lgd` (blank) | `URAK.scn` (mixed) |
| --- | ---: | ---: |
| terrain **0**, which `fire.lgd` already is | **0.05** | **7.63** |
| terrain **2**, which it is not | **16.73** | **1.40** |
| all terrains together | 15.07 | 2.05 |

Emptiness cannot be the variable: on the blank map terrain 0 draws 0.05 cells per paint against
7.63 on the mixed one, a 150x inversion, while terrain 2 runs the other way. The driver is the one
[`TerrainPaintPlan::drawn_cells`](../spikes/asset-viewer/src/map.rs) already documents —
`TileChoice::Kept`. A cell that **already holds a tile the constraints accept keeps it**, and that
is reproducible, because it is what the engine was observed doing. So:

> **What matters is whether the terrain being painted is already there, not how empty the map is.**
> `fire.lgd` is 99% terrain 0, so painting terrain 0 onto it is almost entirely Kept and almost
> entirely reproducible, and painting anything else onto it is almost entirely new and almost
> entirely drawn.

The practical consequence for anyone editing: **widening an existing region of a terrain is close to
reproducible; stamping a new terrain into a large uniform field is mostly this writer's invention.**

```sh
cargo run --release --example paint_refusal_survey -- MAP.scn tilesb01.til 3
cargo run --release --example paint_refusal_survey -- MAP.scn tilesb01.til 3 8   # coarser stride
```

## What the loader does, read out of `lomse.exe`

**Observed in a local binary, 2026-09-17.** Everything above this section was inferred from
statistics over 365 files. This section is read from the engine's instructions instead. Reproduce
all of it with:

```sh
cargo run --release --example map_loader_survey -- "/path/to/English/lomse.exe"
```

The program refuses to report anything until it has re-derived its anchors from the executable it is
given, so it cannot quietly survey the wrong bytes of a different build.

### The call chain

| Operator | Thunk | Object | Worker |
| --- | --- | --- | --- |
| `loadmap` | `0x004dfad0` | `0x005ae958` | `0x004a5270` → terrain reader `0x004a52e0` |
| `loadscenariomap` | `0x00485b80` | `0x005aa12c` | `0x004855c0` |
| `savescenariomap` | `0x00485a80` | `0x005aa12c` | `0x00485550` |
| `savespecialmap` | `0x004eec00` | `0x005aa12c` | **`0x00485550`** — the same function |
| `resetvisibility` | `0x004ab5f0` | `0x005ae958` | `0x004a90b0` |

**`savescenariomap` and `savespecialmap` are the same code.** Both thunks load `ecx` with
`0x005aa12c` and call `0x00485550`. The corpus observation that the two write byte-identical files
was correct, and this is why.

**There is one map object, not two.** The scenario object's terrain member is at
`0x005aa12c + 0x482c`, which is `0x005ae958` — the same global `loadmap` and `resetvisibility` use.
So `resetvisibility` and the file loader are demonstrably talking about the same cell array, which is
what makes `resetvisibility` usable as the anchor for the whole survey.

### The scenario file is a sequence of sections

`0x004855c0` reads, in order:

1. `fread` at `0x004855e5` — four bytes into the scenario object's first dword: **the header word**.
2. `0x004a52e0` — the terrain block: width, height, bytes-per-cell, then the grid.
3. `0x004f7120` — the trailing record section.
4. `cmp dword [esi],64h` at `0x00485617` — **if the header word is at least 100**, `0x004c8fa0`
   reads one more dword; otherwise `0x004c8f90` reads nothing and zeroes the field.

Step 4 is what the corpus saw as "some layouts have a four-byte footer and some do not". **It is not
a footer of the record section.** It belongs to a different member of the scenario object and is read
after it. The corpus split at 98-no-footer / 101-footer brackets the engine's threshold of 100
exactly.

**The grid's cell size comes out of the file.** The reader at `0x004a5341` reads the dword at `0x0c`
and multiplies it by 64 at `0x004a535c` to size each block read; the writer hardcodes `8` at
`0x004a544b`. The field this project called `bits_per_pixel` is the engine's bytes-per-cell.

### The record size is one struct with five version gates

The section reader `0x004f7120` reads a `u32` count, then per record a `u32` **kind**, which it
dispatches through a ten-entry jump table at `0x004f73b8` guarded by `cmp eax,9; ja`. Slots 5 and 6
point at the error path, so eight kinds exist. Every record in the corpus is kind 1, whose class is
constructed at `0x0050c0b0` with a vtable at `0x0054e910`; its read virtual is slot `+0x24`
(`0x0050dd70`) and its write virtual slot `+0x28` (`0x0050dda0`), and **both call the same serialiser
`0x0050da70`**.

That serialiser reads a fixed head and then gates five field groups on the header word, which it
reads from `[0x005aa12c]`:

| Head | Bytes | Where |
| --- | ---: | --- |
| record kind | 4 | `0x004f7194`, consumed by the section reader before dispatch |
| base-class prefix | 24 | `0x004f6b00`: six dwords into object `+4`, `+0x1c`, `+0x20`, `+0x24`, `+0x28`, `+0x30` |
| one more dword | 4 | `0x0050dab5`, into object `+0x48` |

| Gate | Threshold | At or above | Below |
| --- | ---: | ---: | ---: |
| `0x0050dac2` | 98 | 2 | 8 |
| `0x0050db14` | 54 | 8 | 0 |
| `0x0050db87` | 74 | 4 | 0 |
| `0x0050dba1` | 96 | 1 | 0 |
| `0x0050dbbb` | 102 | 2 | 0 |

Evaluating that rule reproduces **every one of the six corpus record sizes and all nineteen header
words**, with nothing fitted:

| Header word | Rule | Corpus |
| ---: | ---: | ---: |
| 63, 73 | 48 | 48 |
| 76, 79, 81, 87, 89 | 52 | 52 |
| 96, 97 | 53 | 53 |
| 98, 101 | 47 | 47 |
| 102–111 | 49 | 49 |

So the answer to "one struct with conditional fields, or a switch?" is **one struct with conditional
fields**. `engine_tail_layout` in `map.rs` evaluates the rule, and
`the_engines_version_gates_reproduce_every_observed_tail_layout` checks it against the corpus
observations, which were measured independently of it.

**What the corpus cannot pin.** Each threshold is pinned only to an interval: the corpus contains 73
and 76, so it cannot tell 74 from 75. The exact values come from the binary alone. The gate at 54 is
not exercised by the corpus at all — every shipped header word is at least 63 — and
`the_corpus_exercises_every_gate_it_is_able_to` says so rather than leaving it a silent hole.

**Also still assumed.** The serialiser can attach variable-length sub-objects on three paths
(`0x0050db48`, `0x0050dbc8`, `0x0050dcd5`), and the rule above walks past all three. That is true of
all 21,117 corpus records and is the first thing to doubt if a file ever disagrees.

### The cell is three fields, and no bit of it is ever masked

#### Four routes reach the cells, and each one had to be found separately

This is the part that took four attempts, and the count of sites roughly doubled at the end of it.
The engine reaches the map object and its cell grid in four distinct ways, and a discovery pattern
for any one of them is structurally blind to the other three:

| Route | Shape | Sites | Cost of missing it |
| ---: | --- | ---: | --- |
| 1 | `mov ecx, 0x005ae958` | 499 | — (the original scan) |
| 2 | `lea ecx, [reg + 0x482c]` | 28 | 9 methods, one with a `+2` write the lane filter would have caught |
| 3 | `call 0x004c6fc0` → object in `eax` | 208 | 10 methods and 7 cell sites, including 5 `+2` readers |
| 4 | `mov reg, [0x005ae9ac]` — the cell array straight from the global | 11 | 10 cell sites, including 3 more `+2` readers |

Route 2 exists because the map object *is* the scenario object's terrain member, so code holding the
scenario pointer reaches it without ever naming `0x005ae958`. Route 3 is a two-instruction accessor,
`mov eax, 0x005ae958; ret`, which is the most-used route by call count. Route 4 skips the object
entirely: code that wants only the grid loads the pointer out of the global.

**Four is the number of routes *found*, not the number that exist.** Each of the four was discovered
only after a negative result looked wrong, and there is no argument here that the list is closed. A
pointer copied into another object's field, spilled to a stack slot and reloaded, passed as a
function argument, or formed by arithmetic the taint does not model would be a fifth route, and
nothing in this survey would see it. That is why the taint-independent mask scan below matters more
than the taint-dependent one.

**Each failed probe failed the same way: it could not have found what it was looking for.** A search
for `mov ecx, [reg + 0x482c]` returns nothing — the member is a subobject, so its address is taken
with `lea`, never loaded — and that nothing was briefly read as evidence the coverage was fine. A
count of "499 of 503 references to `0x005ae958`" likewise cannot see routes 2, 3 or 4, because none
of them contains that constant in the form being counted. The survey now takes all four and reports
the site count of each, so the next such gap shows up as a number rather than as silence.

With all four, the survey walks **169** map-object methods plus **219** mid-function entry points
(208 accessor calls, 11 absolute cell-array loads), running a forward must-analysis in each — a
register counts as holding the cell array only where that is true on every path reaching it. Entries
that begin mid-function start with *nothing* tainted, because `ecx` at an arbitrary point is not the
object and seeding it would invent accesses.

It found **70** instructions naming a cell lane:

| Byte | Reads | Writes | Access widths | stride-8 operands in surveyed methods |
| ---: | ---: | ---: | --- | ---: |
| `+0` | 16 | 5 | 2 (20 sites), 4 (**1** site) | 55 |
| `+1` | 0 | 0 | — | 0 |
| `+2` | 11 | 12 | 2 only | 7 |
| `+3` | 0 | 0 | — | 0 |
| `+4` | 19 | 7 | 4 only | 49 |
| `+5`–`+7` | 0 | 0 | — | 0 |

How those sites were reached, since the forms do not carry equal weight:

| Addressing | Base established as | Sites |
| --- | --- | ---: |
| `[array + index*8 + disp]` | proven object pointer | 46 |
| `[array + index*8 + disp]` | a `+0x54` load in a known map-object method | 3 |
| `[cell_pointer + disp]` | a `+0x54` load in a known map-object method | 10 |
| `[array + byte_offset + disp]` (scale 1) | proven object pointer | 9 |
| `[array + byte_offset + disp]` (scale 1) | a `+0x54` load in a known map-object method | 2 |

46 of the 70 are reads and 24 are writes. **No site is a one-hop mask**, because there are no masks:
the survey's one-hop rule — an immediate mask applied to a register loaded from a lane in the
immediately preceding instruction — fired zero times.

The scale-1 form carries an assumption the scale-8 form does not — that the register holds a
multiple of the eight-byte stride — and the weaker basis carries another, that a `+0x54` load inside
a map-object method is this class's cell array even where the must-analysis has lost the proof. Both
are printed rather than collapsed, because a `[array + index*4]` would look identical to the first
and would be misreported.

Three findings follow, in descending order of confidence.

**1. The first word is two 16-bit fields.** Not one `u32` "tag". Every *interpreting* access to
byte `+0` is 16 bits wide and every access to byte `+2` is 16 bits wide. `getterrain`'s worker at
`0x004a5240` is the clearest single site:

```
004a5254  mov esi,[esi+54h]                 ; the cell array
004a5261  movsx eax,word [esi+eax*8]        ; the tile slot, sign-extended
```

and `forcetexture`'s worker at `0x004a5ed0` is its mirror:

```
004a5efe  movsx edx,word [edx+esi*8]        ; read the current slot
004a5f06  mov [eax],cx                      ; write the new one, 16 bits
```

The slot then goes into `0x00508f10`, which bounds-checks it (`test edx,edx; jl` and
`cmp edx,[ecx+2ch]`) against the tileset's tile count and answers `-1` outside. So it is a **signed
16-bit** index, and **bits 10–15 belong to it** — they are not a separate unused region. This
supersedes this project's `tag & !0x00800000`; `MapCell::tile_index` now masks the low 16 bits. On
the shipped corpus the two rules agree, because `0x00800000` is the only bit above 15 any corpus cell
sets, which is exactly why the corpus could not tell them apart.

**One access is wider, and it decodes nothing.** `0x004a9660 mov ebx,[ecx+eax]` paired with
`0x004a9666 mov ecx,[ecx+eax+4]` copies a whole cell, both words, into a second array. An earlier
version of this section said the engine "never reads the first word as a dword"; that universal
sentence is **false**. The conclusion is not affected — the same function immediately re-reads the
copied cell's low word with `movsx eax, word [edx+eax]` at `0x004a9671` and bounds-checks it — but
the correct statement is about *interpreting* reads, not about all reads.

**2. `0x00800000` is a value of the second 16-bit field, not a bit of the first.**
`resetvisibility`'s body writes that field as a whole word in five places, never as a bit
operation:

```
004a90e0  mov [edx+eax*8+2],di              ; zero every cell's field
004a9118  mov [ecx+eax*8+2],dx              ; or fill it from map object +0x48
004a9170  mov [edi+eax*8-6],dx              ; and 0x80 along the perimeter, when
004a9192  mov [edi+eax*8+2],dx              ; map object +8 is non-zero
```

The perimeter writes are guarded by `test eax,eax; je` on map object `+8` at `0x004a914a`, which
reconciles the two attended runs that looked contradictory: a fresh `clearmap` map has that field
zero, so the perimeter block is skipped and all 4,096 cells come back clear, while the 146 `.smp`
files that carry `0x0080` on exactly their perimeter ring were saved with it set. The
`mov edx,80h` that supplies the value is at `0x004a915f`.

#### The `+2` field is a signed scalar, not a bitfield

**Corrected, 2026-09-17. A first pass of this survey reported that nothing reads the field; that was
false, and false about a method it had already walked.** Three readers sit inside surveyed
functions and a fourth shape appears twice more. The clearest is at `0x004a938b`:

```
004a938b  mov eax,[edi+54h]                 ; the cell array
004a938e  movsx ecx,word [eax+esi+2]        ; read the field, SIGNED
004a9393  lea eax,[eax+esi+2]
004a9397  cmp ecx,ebx
004a9399  jle short 004A93AEh               ; skip the write when stored <= candidate
004a93a6  mov [eax],bx                      ; reached only when stored > candidate
```

The same shape is at `0x004a9048`/`0x004a9055`, and `0x004a9025`/`0x004a9036` is its unconditional
sibling (`je` rather than `jle`: write whenever the value differs).

There are **11 readers in total**, spread right across the binary — `0x004204cb`, `0x0043bb55`,
`0x00451395`, `0x004a9025`, `0x004a9048`, `0x004a938e`, `0x004c5cc2`, `0x004f68de`, `0x00517b67`,
`0x00519ced`, `0x0050c208` — and **every single one is a `movsx`**. Not one is a mask.

Three things follow, and they matter far more than the miss did:

- **It is a scalar, not a bitfield.** `movsx` plus a signed `cmp` is arithmetic on a 16-bit
  quantity. Nothing anywhere masks it. This *strengthens* the two-field split: a bitfield packed
  alongside a tile index would be tested, and this is compared.
- **At the two `jle` sites it only ever decreases.** `cmp ecx,ebx; jle` skips the store when the
  stored value is less than or equal to the candidate, so the store is reached only when the stored
  value is strictly greater — it is replaced by a *smaller* one. Monotonically non-increasing there.
- **`0x80` is an arithmetic scale base, not a flag.** This is the find that reframes the whole
  `0x00800000` story — and the reframing needs its evidence classes kept apart, because the first
  write-up of it here did not. At `0x00519ce7`:

```
00519ce7  mov ecx,[5AE9ACh]                 ; the cell array, straight from the global
00519ced  movsx edx,word [ecx+eax*8+2]      ; the field
00519cfb  mov eax,80h
00519d00  sub eax,edx                       ; 128 - field
00519d05  imul eax,esi
00519d08  sar eax,7                         ; ... * esi / 128
00519d0b  mov [edx+ebp*8+4],eax
```

  `(0x80 - field) * k >> 7` is a linear interpolation whose denominator *is* `0x80`. The same shape
  is at `0x00517b6f`. It is also compared against a *map-object field* rather than a constant: at
  `0x004c5cc2`, `cmp edx,[5ae9a4h]`, which is map object `+0x4c`. The object's `+0x48` is what the
  grid initialiser fills the field with. So there is a global value and a per-cell value, and the
  per-cell one is tested against the global.

  **Corrected, 2026-09-17 — what this does and does not establish.** A first version of this
  section wrote "the field is a level on a 0..128 scale and `0x0080` is that scale saturated" as
  **Observed**. It is not. Splitting it:

  | Claim | Class |
  | --- | --- |
  | Read sign-extended at all 11 readers; never masked | **Observed in a local binary** |
  | Used as `(0x80 - field) * k >> 7`, so `0x80` is a scale base | **Observed in a local binary** |
  | Compared against map object `+0x4c` | **Observed in a local binary** |
  | The corpus holds only `0x0000` and `0x0080` in this field | **Observed** (census of 353 maps, 1,040,384 cells) |
  | Its range is *closed* at `0..128` | **Inferred** |
  | The corpus's `0x0080` is that range **saturated** | **Inferred** |

  The two Inferred rows are the step that joins the arithmetic to the census, and **nothing
  disassembled so far clamps the field**. A scale base of `0x80` is what you would use for a
  fraction in `0..128`, and it is also what you would use for a fixed-point quantity that can
  exceed it; the instructions do not choose. What *is* settled either way is that **the bitfield
  reading is dead** — 11 sign-extending reads and no mask anywhere — and that is the finding worth
  keeping.

**The meaning is still Unknown, and the label is now doing different work.** It is no longer
"nothing reads it" — eleven things read it. A signed scalar that decreases monotonically at two
sites, is compared against a global threshold, and scales a rendering quantity against a base of
`0x80` is *consistent* with fog or light intensity, and `resetvisibility` writing it fits. What is
missing is the step that ties the interpolated result to anything visible: `0x00519d0b` writes it
into another array whose consumer this survey has not followed. **Inferred**, from the arithmetic
and the operator's name — a stronger position than the name alone, and not Observed.

Why the first pass missed it: `cell_lane` required an eight-byte index scale, and all three readers
use the scale-1 form `[array + byte_offset_register + 2]`. The same gap dropped a write inside the
survey's own anchor — `resetvisibility`'s second perimeter write at `0x004a9178`. The program now
accepts both forms and **counts every operand it reaches through the cell array but cannot decode**,
printing the total as `cell-unclassified`, so the next blind spot of this kind is loud rather than
silent. It currently reports zero.

**3. The second word is an `f32`.** Confirmed, and no longer an inference from value ranges. Fifteen
of the eighteen sites touching byte `+4` are x87 instructions. `setelevation`'s worker is the whole
story in five instructions:

```
004a59e8  mov ecx,[ebx+54h]
004a59f3  fld dword [ecx+eax*8+4]           ; load the old elevation
004a59f7  fcomp dword [esp+34h]             ; compare with the new one
004a59fb  lea ecx,[ecx+eax*8+4]
004a5a13  mov [ecx],edx                     ; store the new bits
```

**And no mask anywhere.** Across all **70** sites the survey found **zero** `and`, `test`, `or`,
`xor`, `shr` or `sar` with an immediate against any cell lane, either on the cell operand directly or
on a register loaded from one in the next instruction.

**The reason that is worth stating is that it survived the survey doubling, four times over.** The
site count went 35 → 53 → 60 → 70 as the scale-1 addressing form and then access routes 2, 3 and 4
were added. Every one of the 35 instructions those passes recovered is a `movsx`, `mov` or `cmp`
with no immediate. Widening the analysis added sites and did not add a single mask — and that was
measured at each step rather than predicted once. Every bit-by-bit question this project has asked
about the cell has the same answer: the engine does not work on the cell in bits. It reads a 16-bit
tile slot, reads and writes a 16-bit level, and loads a 32-bit float.

**A second form of the result that does not depend on the taint at all.** The objection to the
above is that a negative produced by a taint analysis is only as good as the taint, and the counters
show the taint does lose things. So the survey also scans **every instruction it decodes, ignoring
the taint entirely**, for a masking instruction whose operand is eight-byte strided at a cell-lane
displacement — `and`, `test`, `or`, `xor`, `shr`, `sar`, `bt`, `bts`, `btr` with an immediate. That
over-counts freely: any eight-byte array in any surveyed function would qualify, and there are
several. It finds **zero**.

That is the stronger statement, because no amount of missed taint can weaken it: within the surveyed
code there is no masking instruction against an eight-byte-strided operand at a cell-lane offset,
whether or not the analysis could prove the base was the cell array. It also covers the 20 unreached
blocks, which this scan reads regardless of reachability. What it does *not* cover is the scale-1
addressing form — lanes there cannot be identified without the taint — or any function the four
discovery routes never found.

**The ceiling, stated plainly.** Every discovery route requires a direct `call rel32`, so **virtual
dispatch is entirely uncovered** — the surveyed methods contain 36 indirect call sites, and a method
reached only through a vtable slot is not in the set. The honest form of the finding is: *no
interpreting bitwise-immediate access to a cell lane exists among map-object methods reachable
without virtual dispatch, and no masked eight-byte-strided operand at a cell-lane offset exists
anywhere in the code those routes reach.* Vtable recovery is the next step, not a caveat to wave
at.

**Scope, stated so a quiet result is not read as a clean one.** The eight-byte-strided operand
counts in the table above come from the *same per-function decodes the survey uses*, not from a
linear disassembly of the whole `.text`. An earlier version of this section quoted 4,892 and 4,284
from such a whole-section decode — which drifts out of phase on embedded jump tables, so those
figures described a deliberately misaligned reading and were quoted as the denominator of a negative
result. They are withdrawn. The remaining gap between the census and the survey's own counts is
other eight-byte arrays in the same functions, the map object's second array at `+0x64` among them —
the one `anythingat?` and `terrainspriteat` use, which the survey deliberately excludes.

**`cell-unclassified` is a real instrument but a narrow one, and the first write-up of it here
overstated what its zero means.** It counts operands the taint *reached* and could not decode. It
says nothing about operands the analysis never reached, which is a larger hole. Adding counters for
the reachable parts of that hole made the point immediately — the run reports:

| Counter | Value | What it means |
| --- | ---: | --- |
| `cell-unclassified` | 0 | operands reached through the cell array and not decoded |
| `survey-entries` | 388 | 169 methods plus 219 mid-function entries |
| `survey-entries-undecodable` | 0 | entry addresses that would not decode |
| `survey-functions-truncated-at-2400-instructions` | 1 | decode hit its per-entry limit |
| `survey-unreached-blocks-with-cell-shaped-operands` | 20 | blocks the dataflow never gave an entry state that nonetheless contain an eight-byte-strided operand |
| `survey-masked-stride8-operands-ignoring-taint` | **0** | see below |

The first version of the discovery loop also **did not converge** within its round cap, and said so
once the counter existed; raising the cap showed the method set was already complete at 169, so the
cap was failing to *prove* convergence rather than losing methods. It is now set where it converges.

A raw count of unreached blocks turned out to be a bad instrument — it is dominated by junk past the
end of a function and grows when the decode limit is *raised*. It is therefore filtered to blocks
containing a cell-shaped operand, where 20 is a real number worth reporting rather than an artefact.

**Five loss paths remain uncounted, and are printed by name rather than described in prose**: an
object pointer spilled to a stack slot and reloaded, an object or cell pointer passed as an
argument, a cell pointer stored into another object's field, pointer arithmetic the taint does not
model, and access routes beyond the four found. An operand lost to any of those is absent from the
results *and* from every counter above.

### An observed anomaly: byte stores to the map object's first byte

**Observed in a local binary, 2026-09-17. Unexplained, recorded rather than guessed at.** A
gamescript operator body writes a single **byte** to the map object's offset `0`, twice on different
paths, from computed values:

```
004a82a1  and al,0Ch
...
004a82b2  call 005394A0h                    ; float-to-integer
004a82b7  mov [5AE958h],al
...
004a82c2  sar eax,8
004a82c5  mov [5AE958h],al
```

A third such store sits at `0x004a82f5`. The same function raises gamescript error `0x0e` through
`0x004d4550`, which is how the operator rejects bad arguments.

What this rules out is small but real: **offset `0` of the map object is not a C++ vtable pointer**,
because nothing writes a computed byte into one, and it is not part of the cell array, which starts
at `+0x54`. It is a write path to the object that is neither a method call nor a cell access, and
this survey has no reading of it beyond that. Two neighbouring fields are better understood:
`+0x48` is what the grid clear fills the `+2` cell field with, and `+0x4c` is the threshold the
`+2` field is compared against at `0x004c5cc7`.

### What this contradicts

| Previously held | Now | Evidence |
| --- | --- | --- |
| `0x0c` is `bits_per_pixel` | **Corrected**: bytes per cell, read from the file and used as the stride | `0x004a5341`, `0x004a535c`, `0x004a544b` |
| Cell word 0 is a 32-bit tag; bits `10..22` unused | **Corrected**: two 16-bit fields; bits 10–15 belong to the tile slot, which is signed | `0x004a5261`, `0x004a5f06`, `0x00508f10` |
| `tile_index = tag & !0x00800000` | **Corrected**: `tag & 0xffff` | same |
| A writer preserves bit `0x00800000` when setting a tile | **Corrected**: it must preserve the whole upper 16-bit field, `tag & 0xffff0000`. Latent, not live — across all 353 parseable maps and 1,040,384 cells the field holds only `0x0000` (1,012,936) and `0x0080` (27,448) | `0x004a5f06` |
| The `1024` tile-index cap reflects the tag layout ("bits 10..22 unused") | **Corrected**: the cap is a conservative corpus guard; the field is a signed `u16` and the tileset decides the real limit | `0x00508f10` |
| Some layouts have a 4-byte record-section footer | **Corrected**: those four bytes are a separate section, gated at header word 100 | `0x00485617`, `0x004c8fa0`, `0x004c8f90` |
| Six record layouts | **Corrected**: one record struct, five version gates, one record *kind* out of eight | `0x0050da70`, `0x004f73b8` |
| The loader consulting the header word is Inferred | **Confirmed**: it switches on it in six places | `0x0050dac2`, `0x0050db14`, `0x0050db87`, `0x0050dba1`, `0x0050dbbb`, `0x00485617` |
| Cell word 1 is a float, inferred from value ranges | **Confirmed** from the instruction | `0x004a59f3` and fourteen more |
| `0x00800000` is a bit whose meaning is Unknown | **Refined**: it is the value `0x0080` of a *signed 16-bit scalar* at cell `+2`, read sign-extended at all 11 readers and never masked, used arithmetically against a scale base of `0x80`. Whether its range is closed at `0..128`, and so whether the corpus value is saturation, is **Inferred** | `0x00519d00`, `0x004c5cc2`, `0x004a938e` |

### What this did not settle

- **What the `+2` level measures.** Eleven readers were found — an earlier draft of this section
  wrongly said none were. They establish the field's *shape* decisively (a signed `0..128` level,
  interpolated and compared, never masked) and its *meaning* only by suggestion. The missing link is
  the consumer of the array `0x00519d0b` writes the interpolated result into.
- **Virtual dispatch.** Every method-discovery route follows only direct `call rel32`; the surveyed
  methods contain 36 indirect calls. Vtable recovery is the highest-value next step and would raise
  the ceiling on every negative above.
- **What map object offset `0` holds**, given that three byte stores write computed values into it.
- **What the other seven record kinds are.** The jump table at `0x004f73b8` has eight live slots;
  the corpus only ever uses kind 1. The constructors are at `0x004f71b0`, `0x004f71e1`, `0x004f7213`,
  `0x004f7248`, `0x004f727d`, `0x004f72a8`, `0x004f72af`, `0x004f72da`.
- **The exact thresholds 74 and 75.** The binary says 74; the corpus cannot tell.
- **What map object `+8` and `+0x48` are** — the two fields that decide whether `resetvisibility`
  writes the perimeter and what value it fills with.
- **What the dword read at `0x004c8fa0` is for.**
- **Whether a `.map` loaded by `loadmap` has a header word at all.** `0x004a5270` calls the terrain
  reader without reading one first, so a bare terrain file would begin with `width`. Every file in
  `English/map/` parses as a scenario file, so nothing in the corpus exercises that path.

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
target/release/lom-asset-viewer --map-roundtrip '/path/to/Lords of Magic Special Edition/English/map'
target/release/lom-asset-viewer --map-set-tile IN.scn 10 20 392 OUT.scn
target/release/lom-asset-viewer --map-set-terrain IN.scn 10 20 water OUT.scn
target/release/lom-asset-viewer --map-set-elevation IN.scn 10 20 2.5 OUT.scn
target/release/lom-asset-viewer --map-fill-terrain IN.scn water OUT.scn
target/release/lom-asset-viewer --map-paint-terrain IN.scn 10 20 14 22 water OUT.scn tilesb01.til
target/release/lom-asset-viewer --map-paint-terrain IN.scn 10 20 14 22 water OUT.scn tilesb01.til --seed 7
target/release/lom-asset-viewer --map-tileset-for aicave.smp
target/release/lom-asset-viewer --map-paint-terrain aicave.smp 10 20 14 22 12 OUT.smp aibldg01.til
target/release/lom-asset-viewer --map-place-sprite IN.scn 10 20 470 OUT.scn
target/release/lom-asset-viewer --map-remove-sprite IN.scn 200 OUT.scn
target/release/lom-asset-viewer --view-map '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
target/release/lom-asset-viewer --view-map MAP.scn tilesb01.til tilesb01.lbm
target/release/lom-asset-viewer --export-map-preview MAP.scn tilesb01.til tilesb01.lbm /tmp/map-preview.png
target/release/lom-asset-viewer --serve --pic '/path/to/Lords of Magic Special Edition/English/pic.mpq'
target/release/lom-asset-viewer --serve tilesb01.til tilesb01.lbm --port 9000

# Read the format out of the engine rather than out of the corpus.
cargo run --release --example map_loader_survey -- '/path/to/Lords of Magic Special Edition/English/lomse.exe'
```

`map_loader_survey` prints the loader's field-by-field stream trace, the record size the engine's own
version gates imply for every header word, and every instruction that names a cell lane — reached
through all four routes to the map object. It verifies its anchors against the executable before
reporting, recovers the record-kind dispatch bound from the binary rather than trusting a constant,
and exits non-zero if either fails. It also counts every operand it reaches through the cell array
but cannot decode, as `cell-unclassified`; that number should be zero and is the alarm if a new
addressing form appears. Set `MAP_SURVEY_DUMP_METHODS=1` to list the methods surveyed. See
[What the loader does](#what-the-loader-does-read-out-of-lomseexe).

`--dump-map-cells` prints one line per cell — `x`, `y`, packed index, raw tag, masked tile index,
whether the `0x00800000` flag is set, and the elevation word — for the whole grid or for an
inclusive `X0 Y0 X1 Y1` rectangle. `--diff-maps` prints the two headers, every differing cell, and
the first differing byte of the trailing section with a hex window either side; it compares the
tails as raw bytes from their own starts, because assuming a record size would beg the question the
diff is being used to answer. Together they are the readback half of an engine probe: writing
chosen values from the running game is only useful if they can be read out again.

With a tile definition and atlas, the viewer starts in terrain-art mode. Press `C` to cycle through diagnostic cell tags and candidate elevation. The terrain view proves tile selection — the masked tags resolve to real terrain art — but not orientation, and its 8×8-per-cell overview is not yet a faithful recreation of the original renderer's full-size terrain composition.

## Confidence

- **Observed in a local binary:** all 365 files have a 16-byte prefix, declared 8-bit depth, a complete `width × height × 8` cell grid, and a bounded trailing section; every one of the 365 trailing sections resolves to exactly one of six record layouts, and all 21,117 records satisfy the decoded bounds and invariants.
- **Inferred (2026-09-17):** which operand is *x*. The capture shows the two paint operators agree with `map2screen` on axis order; it cannot fix `map2screen`'s own labelling. See the storage-order section.
- **Observed in gameplay (2026-09-17):** the low tag bits are the tile-atlas slot exactly, for seven forced slots spanning `0..623`; cells are packed `second_operand × width + first_operand`; the terrain-type-to-tile table above; terrain type is derived from the tile through the tileset, not stored in the cell; `forcetexture` writes one cell while `setterrain` also blends its 8-neighbourhood; the trailing section is `count`, records, `footer`; `sprite_type` at `+28` is the terrain sprite type id; `instance_id` starts at 200 and increments; `savescenariomap` and `savespecialmap` write identical bytes; sprite placement and removal round-trip byte-exactly.
- **Observed in gameplay (2026-09-17), earlier run:** a 512x512 map generated by the shipped engine carries the same 16-byte prefix as a 128, so the header does not disappear on oversized maps.
- **Corrected:** cell and record coordinates, previously documented and implemented as X-major (`x × height + y`). Every shipped map is square, so the corpus could not falsify it.
- **Refuted:** tag bit `0x00800000` as a forced-texture flag. Forcing textures into 4,096 cells set it in none of them.
- **Inferred:** the second word is elevation; record `+34` is a procedure identifier.
- **Observed in a local binary (2026-09-17):** every one of the 365 installed maps re-encodes to its input bytes, and all 21,117 placed-sprite records rebuild from their typed fields alone. Place-then-remove returns every one of the 365 installed maps byte for byte, exercised through the CLI a file at a time.
- **Observed in gameplay (2026-09-17, mapload probe):** the engine loads maps this project wrote -- edited, sprite-placed, created from nothing, and non-square -- and the two created-from-nothing maps re-save byte-identically; the header word at `0x00` is rewritten from engine state on every save rather than carried from the file; tag bit `0x00800000` does not survive a load-and-save; `setterrain`'s transition ring is a direction table on the background, identical across nine of the eleven terrains.
- **Observed in gameplay (2026-09-17, terrainrings):** `setterrain`'s transition ring, for all eleven backgrounds. It is one offset table plus a per-background anchor for the eight that blend; `tt_dirt` and `tt_impassible` blend nothing; `tt_road` as a background has its own measured edge table. A painted region's **interior** is a random draw from its terrain's `384 + 8k` family and cannot be reproduced by a writer.
- **Not reproduced, permanently:** a painted region's interior. The same experiment run twice gave centre tiles 385 and 390 while the ring was byte-identical, so this is a property of the engine rather than a gap in the measurement.
- **Observed in a local binary (2026-09-17):** the whole blend rule is declared in the shipped `.til` tilesets, whose eight neighbour-constraint columns this project had been parsing and discarding. Re-selecting tiles from those constraints reproduces the engine on 2,084 of 2,084 deterministic cells across the `terrainrings` captures, with zero mismatches, and every cell of all 365 installed maps holds a tile `tilesb01.til` declares. See [the tileset declares the whole blend rule](#the-tileset-declares-the-whole-blend-rule).
- **Derived (2026-09-17):** the `.til` neighbour columns are geometric directions from the cell's own position, `n` being `(0, -1)`. The mirrored reading satisfies 0 of 576 engine-written ring tiles against 576 of 576 for this one.
- **Corrected:** the tileset parser, which read a `TILE=` line's slot and `self` column and threw its eight neighbour constraints away. The rule it was discarding was then measured in an attended engine run instead.
- **Observed in a local binary (2026-09-17):** shipped world maps are **not** fully consistent with their own tileset -- 5.9% of `.scn` cells hold a tile that does not satisfy its declared constraints -- and the 337 `.smp` battle maps do not use `tilesb01.til` at all.
- **Observed in a local binary (2026-09-17):** the engine reads `.scn`/`.lgd`/`.map` through `tilesb01.til`, set by `maptileset` in `START.GS`. `maptileset` occurs **10 times across the 1,700 `gs.mpq` members**: once there, six times as the same `"til/tilesb01.til"` restore on the way out to the menu, and three times as the terrain editor's `currenttileset exch maptileset`, which restores nothing.
- **Observed in a local binary (2026-09-17):** a **combat** map's tileset is declared **per encounter**, as adjacent `/mapfile` and `/tileset` keys of the encounter's dictionary. 169 of the 337 installed `.smp` are bound this way (147 to one tileset, 22 to several); 168 have no binding and are **unresolved**. 15 distinct tilesets appear across the bindings. See [which tileset a map is read through](#which-tileset-a-map-is-read-through).
- **Observed in a local binary (2026-09-17):** scored against the gamescript's own tileset the 169 bound combat maps satisfy **370,254 / 388,910 = 95.20%** of their cells' declared constraints, against **8.43%** through `tilesa01.til`, with the script tileset winning on 166 of 169. The same instrument prints `.scn` at 94.11% open-edge, reproducing this document's independently measured 5.9% violation rate.
- **Observed in a local binary (2026-09-17):** a **second** binding form, the runtime selector: a `/tilesets` procedure over a separately named `/tiles[...]` array, in 41 members, letting **one** encounter pick among up to four tilesets from a sprite's map location. Recorded coarsely and **separately** in `COMBAT_TILESET_ARRAY_CANDIDATES`: it resolves **zero** additional maps -- all 43 maps it names are already declared -- while widening 42 of them across more than one rule class.
- **Corrected (2026-09-17):** *"there is no `/tilesets[...]` randomised array anywhere in the corpus."* This project's own claim, and wrong: the search looked for `/tilesets[` where the form is `/tilesets{...}` plus a separately named `/tiles[...]`. What is genuinely absent is *randomised* selection, which a 2025-06-12 `DUNGEONS5.gs` changelog comment records as **removed**.
- **Corrected (2026-09-17):** the binding extractor did not strip `;` comments, and gamescript members use bare `CR` line endings, so it read bindings out of **disabled code** in seven members -- including the `wilderness_*.gs` pair whose binding is commented out because those are the outside-combat files. Honouring comments takes multi-valued maps from 26 to 22 and undeclared cells from 880 to 466.
- **Observed in a local binary (2026-09-17):** `combattileset` occurs **exactly once in all 1,700 `gs.mpq` members** and is `tilesa01.til`. Per a corpus comment in `wilderness_land.gs`, it is the default for an encounter that defines **neither** `mapfile` nor `tileset` -- engine-generated outside combat -- **not** the tileset of the shipped `.smp` files.
- **Refuted (2026-09-17):** *"all 337 `.smp` are read through `tilesa01.til`."* This project's own claim, held for a few hours. The `combattileset`-occurs-once measurement was correct; the inference from it was not. Two reviewers reproduced every supporting percentage and endorsed it anyway.
- **Refuted (2026-09-17):** *"no sampled tileset fits `.smp`; the best had 36% of cells undeclared."* The sample excluded both 624-slot tilesets, and the figure was inverted -- 35.41% is what the two **worst** tilesets *declare*.
- **Corrected (2026-09-17):** a per-faith/per-location tileset read from the `.smp` filename was filed as **Refuted** with the wrong explanation. Combat maps really do each have their own tileset, so the mechanism is **Observed**; what is false is that the *filename* selects it. `{faith}bldg01.til` matches the gamescript pairing for only **41 of 263** faith-prefixed maps, where it merely *declares* every slot for 236 -- and conflating those two is what made the rule look causal.
- **Corrected (2026-09-17):** *"the other 24 `.til` members are the terrain editor's palette, not map tilesets."* False: 15 of the 26 shipped tilesets appear in combat-map bindings, and `cavelava.til` alone is named by about 60 `gs\dungeons\...` members.
- **Refuted (2026-09-17):** a constant slot offset or bank. The 624-offset sweep is a broad hump peaking at 47.88% for offset +336 with offset 0 ranked 70th of 624, median 2.77%, and the hump is wildcard density in the `free move` blocks (**2.33** `*` columns per row for slots `480..=623` against **1.67** for `0..=479`), not alignment.
- **Withdrawn (2026-09-17):** `loadsubmappages` as the explanation for a low combat-map satisfaction rate. There is no such residue: the 8.11% was scoring against the wrong tileset. The two 320x624 pages remain a recorded, unexplained observation.
- **Corrected (2026-09-17, review):** the paint's tie-break. Reading an off-map neighbour as satisfying every constraint let the one-sided boundary tiles compete with the interior family along every edge, and a lowest-slot tie-break then wrote a phantom coastline on a whole-map water paint and nine different road tiles on a uniform road field. The edge is now read **closed**, and a cell that already holds a valid candidate **keeps** it -- which also makes the `tt_dirt` ring a no-op, as `TERRAIN_TRANSITIONS` always said it was.
- **Corrected (2026-09-17, review):** "2,084 cells whose candidate set was a single tile". The total is right; 196 of them have two to sixteen candidates and are reproducible because the engine leaves such a cell alone. The figure had also been measured with a keep-current rule the code did not yet implement.
- **Observed in a local binary (2026-09-17):** all 4,043 `TILE=` rows and all 402 `TERRAINTYPE=` rows in all 26 shipped tilesets have exactly 11 fields, and none is incomplete. A parser comment justifying lenient short rows on the grounds that `tilesa01.til` "is already a different shape" was **wrong**: the two differ in row count, 609 against 617, not field shape.
- **Observed in a local binary (2026-09-17):** terrain ids are **tileset-local**. `tilesb01.til` declares 0..10; `cavecry2.til` reaches 42. Atlases run 64, 128, 256 and 624 slots.
- **Refuted (2026-09-17):** the header word at `0x00` as a value the engine *carries through a save*. `URAK.scn`'s `0x6c` came back as `0x6f`. Whether the **loader** reads it is untested -- that would take loading two maps differing only in that word -- and "from its own state" is equally consistent with "set by the last `newmap`".
- **Unknown:** elevation units, what sets tag bit `0x00800000` in memory, the trailing footer's `1` vs `3` and why four of the six layouts have no footer at all, the attribute field at `+24`, and what the six record layouts' constant tail words mean. **Inferred:** that the header word at `0x00` is a format version and is what selects a layout; the corpus partition is exact and ordered, but no engine measurement confirms the loader reads it. The writer copies all of them rather than minting them -- except a newly placed sprite, which mints `+24`, and that record has now been shown to survive the engine byte-exactly.
