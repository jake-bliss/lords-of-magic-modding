# Map and Scenario Format

## Status

**Header, cell grid, terrain-atlas lookup, and dominant 49-byte record milestone complete.** The native Rust parser bounds-checks every installed `.scn`, `.smp`, and `.lgd` file without modifying it. It renders the standard world map from original terrain art and structurally decodes all files in the dominant trailing-record family. The 52-/53-byte families and exact meanings of several object fields remain under investigation.

**An attended engine run on 2026-09-17 wrote maps with values chosen in advance and read them back.** It confirmed the tile-index reading by construction, produced the engine's terrain-type-to-tile table, and **refuted two claims this document previously asserted**: the cell storage order (it is packed y-major, not X-major) and the meaning of tag bit `0x00800000` (it does not mark a forced texture; its meaning is Unknown). Both refutations, and the reasoning that produced the wrong claims, are kept below.

**A map writer landed on 2026-09-17, and the engine accepts what it writes.** Every installed map
re-encodes to the exact bytes it was read from -- 365 of 365, with all 16,628 placed-sprite records
rebuilt from their typed fields. An attended run then handed the running game seven maps and it
loaded **all seven**: a shipped map re-encoded by this project, an edited one, one with a sprite
placed, one created from nothing, and one that is **non-square**. The two created-from-nothing maps
re-saved **byte-identically**. See [Writing maps](#writing-maps) and [Engine
acceptance](#engine-acceptance-measured).

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
- rendering `URAK.scn` through those indices produces a coherent world containing connected oceans, snow, forest/grass, and desert regions. This establishes that the masked values index real terrain art rather than noise. It establishes **nothing about orientation**: a transposed world map is also coherent, which is why this check never caught the X-major error.

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
| `+24` | 4 | `attribute_bits` | Meaning unknown. **Observed in a local binary:** across 16,628 corpus records only the upper nibble varies, taking codes `0..11` and `15`. **Observed in gameplay, 2026-09-17:** all three probe records carry `0x00000001`, which has low bits set and so *violates* that corpus invariant. The contradiction is the finding. The likelier reading is that a freshly minted, procedure-less sprite writes a record shape the corpus does not contain — not that 16,628 records were misread — so the corpus measurement stands and `attribute_code_candidate()` is retained, flagged, and asserted of nothing |
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

- **what tag bit `0x00800000` means** — `forcetexture` sets it and **`resetvisibility`** clears it, both measured; reading it as *visibility state* is an inference from the operator's name and is **not** established. What computes the perimeter ring the corpus carries, and whether a `.smp` load-and-save preserves it, are open;
- **the 52-/53-byte record families** — now known to be a **content** difference, not a format one,
  since both save operators write identical bytes;
- **the 18 unmatched tails** and the one ambiguous file;
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

`--map-roundtrip` asserts that over the installed corpus: **365 checked, 365 byte-identical, 16,628
placed-sprite records rebuilt from their typed fields, 0 failures.** Run it before trusting an edit.

Two rules make that possible while most of this format is still Unknown:

1. **When editing existing data, fields whose meaning is unknown are copied, never minted.** The
   header word at `0x00`, the trailing footer, the record attribute field at `+24`, tag bit
   `0x00800000` and the whole trailing section of every family this project has *not* decoded all
   survive a round trip untouched. A writer that guessed at them would corrupt maps in ways no test
   here could see.

   **Placing a new sprite is the exception, and it mints nine fields.** A record that did not exist
   has to get its bytes from somewhere. Eight of the nine are invariant across all 16,628 corpus
   records. The ninth is the `+24` attribute field, written as `0x00000001` because that is what the
   2026-09-17 probe watched the engine write for a fresh sprite — and it **contradicts** the corpus
   reading of that field, in which only the upper nibble varies. Whether the engine accepts a record
   of this shape is unmeasured. Editing an existing sprite mints nothing; placing a new one does.
2. **Records rebuild from typed fields, not from carried-over bytes.** The decoded fields cover all
   49 bytes of a placed-sprite record with no gap, so `PlacedSpriteRecord49::to_bytes` reproduces
   the original exactly *and* an edited field actually lands. `--map-roundtrip` checks that record
   by record, which is stricter than comparing whole files: a file can round-trip through its raw
   tail while a field is being written back wrong.

### What the editor can and cannot do

| Operation | Status |
| --- | --- |
| Force a cell's tile-atlas slot | Reproduces `forcetexture` exactly |
| Force a cell to a terrain type's base tile | Reproduces `forcetexture` with the measured terrain table |
| Fill every cell with a terrain type | Reproduces `clearmap` |
| Set a cell's elevation word | Writes the word; its runtime units stay **Inferred** |
| Place or remove a terrain sprite | Round-trips byte-exactly, as the engine's own does |
| **Paint a region and blend its transition ring** | Reproduces `setterrain`'s **ring** on a uniform, recognised background; refuses everything else. See below |
| Create a map from nothing | **Not offered.** See below |

**`setterrain`'s ring is now reproduced, and only its ring.** `--map-paint-terrain IN X0 Y0 X1 Y1
TERRAIN OUT` paints a rectangle and writes the measured transition ring one cell outside it, from
[the offset table](#setterrain-transition-tiles-one-offset-table-one-anchor-per-background): one
tile per direction, `anchor(background) + offset(direction)`. What it does **not** reproduce is the
*core*: the engine picks its core tile from a family — terrain 6 onto a tile-15 background wrote
tiles in `385..391` — and the region here is filled with the type's representative tile instead,
which is `forcetexture` semantics. The blend is measured; the core is not, and the command says so
on stderr every time.

Everything the run did not establish is **refused**, not approximated, because plausible-looking
wrong tiles in a map are exactly what no test here could catch:

| Case | Why |
| --- | --- |
| `tt_road` as the painted terrain | Ragged along every edge on all seven backgrounds where it blends — not one tile per direction |
| `tt_road` as the background | Its ring depends on the painted terrain and only its edges change |
| A background cell whose tile is not a type's representative tile | The background terrain is then unknown, and the rule was measured against a known background |
| Two terrains in the 8-neighbourhood | Painting next to an existing boundary is unmeasured |
| A region covering the whole map | No ring, so no background to read; that is `--map-fill-terrain` |

`tt_dirt` (0) and `tt_impassible` (10) are not refused: the region is painted and **no** ring is
written, which is what the engine does. Painting a terrain onto itself writes nothing at all — the
run's own control row produced a ring of pure background tile.

**On shipped maps this refuses almost everywhere, and that is the honest answer.** Real maps are
painted from tile families, and only the eleven representative slots are in the terrain table, so
`--map-paint-terrain` on `URAK.scn` reports `tile 31 at (39, 39) is not any terrain type's
representative tile`. The command is usable on uniform ground — a `--map-create` map, or a
`--map-fill-terrain` one — and says why it will not guess anywhere else.

`--map-set-terrain` is unchanged and still offered: it writes one cell, `forcetexture`-style, and it
is the only one of the two that works where the neighbourhood cannot be read. What remains open on
[issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4) is the core-tile family,
road in either role, and painting across an existing boundary.

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
rather than the format's. It *is* range-checked against the **tag word**: corpus tag bits `10..22`
are zero across all 1,258,496 cells, so an index of 1024 or more is refused — a fat-fingered `3920`
for `392` is the realistic input, and it used to be written straight into the tag.

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

**Observed in gameplay, 2026-09-17.** The two runs bracket the behaviour:

| sequence | the bit, across all 4,096 cells of a 64x64 map |
| --- | --- |
| `clearmap` then save, **no renderer calls** | **set** |
| `clearmap`, paints, then `rebuild3dmap resetvisibility rendermap refreshdirty`, then save | **clear** |

So `forcetexture` — which `clearmap` calls on every cell — *does* set the bit, and something on the
render path clears it. **There was never a contradiction** with the earlier "0 of 4,096" reading:
that run was measuring the state after a rebuild.

**Isolated 2026-09-17 to a single call.** Five fresh maps, one renderer call each — fresh because
once the bit is cleared it stays cleared:

| map | sequence | bit set |
| --- | --- | ---: |
| `zf0.scn` | `clearmap`, save | 4096 / 4096 |
| `zf1.scn` | `clearmap`, `rebuild3dmap`, save | 4096 / 4096 |
| **`zf2.scn`** | `clearmap`, **`resetvisibility`**, save | **0 / 4096** |
| `zf3.scn` | `clearmap`, `rendermap`, save | 4096 / 4096 |
| `zf4.scn` | `clearmap`, `refreshdirty`, save | 4096 / 4096 |

**`resetvisibility` is the one.** Not `rebuild3dmap`, which an earlier draft of this section
proposed — that hypothesis was wrong and the isolation says so.

**What the operator's name suggests, and what it does not establish.** `resetvisibility` clearing a
per-cell bit invites reading the bit as visibility state, and that would reframe the corpus pattern
neatly — the bit sits on exactly the perimeter ring of 146 `.smp` files, which reads very differently
as a visibility flag than as a texture flag.

But that is an inference **from the operator's name**, and a name is not evidence about a field. The
call could as easily clear a generic dirty or cache flag as a side effect. This project has been
caught inferring semantics from plausible names before, so the recorded fact is the narrow one:
`forcetexture` sets the bit, `resetvisibility` clears it, and the meaning stays **Unknown**.
Separating "visibility" from "scratch state that the visibility pass happens to reset" needs an
experiment on visibility itself.

What is still **not** established is whether a load or a save touches the bit independently. Every
echo save in the `mapload` run happened after the renderer block, so rung 4's sixteen cleared
interior cells are equally explained by `resetvisibility` running in that block.

The [refutation of the forced-texture flag](#tag-bit-0x00800000--refuted-as-a-forced-texture-flag)
above still stands, but on different evidence than it was written with: the bit does not mean "this
cell's texture was forced", because the shipped corpus carries it on exactly the border ring and on
no interior cell. The "`forcetexture` never sets it" step in that argument was an artefact of
operation order and should not be relied on.

**Writer guidance: preserve the bit, never clear it.** The corpus shows it living on disk in one
place — 27,448 cells across 146 `.smp` files, exactly their perimeters — and the probe only ever
used `loadscenariomap`/`savescenariomap`, never the `.smp` path. Whether a `.smp` load-and-save
preserves the ring is **unmeasured**, so an editor that dropped it could be destroying real data.

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

So a writer cannot reproduce an interior, and should not try. Writing one representative tile is a
legitimate, if less varied, choice — and it is what `--map-set-terrain` and `--map-paint-terrain` do.

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
target/release/lom-asset-viewer --map-paint-terrain IN.scn 10 20 14 22 water OUT.scn
target/release/lom-asset-viewer --map-place-sprite IN.scn 10 20 470 OUT.scn
target/release/lom-asset-viewer --map-remove-sprite IN.scn 200 OUT.scn
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

With a tile definition and atlas, the viewer starts in terrain-art mode. Press `C` to cycle through diagnostic cell tags and candidate elevation. The terrain view proves tile selection — the masked tags resolve to real terrain art — but not orientation, and its 8×8-per-cell overview is not yet a faithful recreation of the original renderer's full-size terrain composition.

## Confidence

- **Observed in a local binary:** all 365 files have a 16-byte prefix, declared 8-bit depth, a complete `width × height × 8` cell grid, and a bounded trailing section; all 196 exact 49-byte-family files and 16,628 records satisfy the decoded bounds and invariants.
- **Inferred (2026-09-17):** which operand is *x*. The capture shows the two paint operators agree with `map2screen` on axis order; it cannot fix `map2screen`'s own labelling. See the storage-order section.
- **Observed in gameplay (2026-09-17):** the low tag bits are the tile-atlas slot exactly, for seven forced slots spanning `0..623`; cells are packed `second_operand × width + first_operand`; the terrain-type-to-tile table above; terrain type is derived from the tile through the tileset, not stored in the cell; `forcetexture` writes one cell while `setterrain` also blends its 8-neighbourhood; the trailing section is `count`, records, `footer`; `sprite_type` at `+28` is the terrain sprite type id; `instance_id` starts at 200 and increments; `savescenariomap` and `savespecialmap` write identical bytes; sprite placement and removal round-trip byte-exactly.
- **Observed in gameplay (2026-09-17), earlier run:** a 512x512 map generated by the shipped engine carries the same 16-byte prefix as a 128, so the header does not disappear on oversized maps.
- **Corrected:** cell and record coordinates, previously documented and implemented as X-major (`x × height + y`). Every shipped map is square, so the corpus could not falsify it.
- **Refuted:** tag bit `0x00800000` as a forced-texture flag. Forcing textures into 4,096 cells set it in none of them.
- **Inferred:** the second word is elevation; record `+34` is a procedure identifier.
- **Observed in a local binary (2026-09-17):** every one of the 365 installed maps re-encodes to its input bytes, and all 16,628 placed-sprite records rebuild from their typed fields alone. Place-then-remove returns a shipped 128x128 map byte for byte.
- **Observed in gameplay (2026-09-17, mapload probe):** the engine loads maps this project wrote -- edited, sprite-placed, created from nothing, and non-square -- and the two created-from-nothing maps re-save byte-identically; the header word at `0x00` is rewritten from engine state on every save rather than carried from the file; tag bit `0x00800000` does not survive a load-and-save; `setterrain`'s transition ring is a direction table on the background, identical across nine of the eleven terrains.
- **Observed in gameplay (2026-09-17, terrainrings):** `setterrain`'s transition ring, for all eleven backgrounds. It is one offset table plus a per-background anchor for the eight that blend; `tt_dirt` and `tt_impassible` blend nothing; `tt_road` as a background has its own measured edge table. A painted region's **interior** is a random draw from its terrain's `384 + 8k` family and cannot be reproduced by a writer.
- **Not reproduced, permanently:** a painted region's interior. The same experiment run twice gave centre tiles 385 and 390 while the ring was byte-identical, so this is a property of the engine rather than a gap in the measurement.
- **Refuted (2026-09-17):** the header word at `0x00` as a value the engine *carries through a save*. `URAK.scn`'s `0x6c` came back as `0x6f`. Whether the **loader** reads it is untested -- that would take loading two maps differing only in that word -- and "from its own state" is equally consistent with "set by the last `newmap`".
- **Unknown:** elevation units, what sets tag bit `0x00800000` in memory, the trailing footer's `1` vs `3`, the attribute field at `+24`, and what distinguishes the 52-/53-byte record variants. The writer copies all of them rather than minting them -- except a newly placed sprite, which mints `+24`, and that record has now been shown to survive the engine byte-exactly.
