use std::collections::BTreeSet;
use std::fmt;

use crate::tile::{
    Direction, Neighbourhood, TileChoice, TileSelector, TileSetDefinition,
};

const HEADER_SIZE: usize = 16;
const CELL_SIZE: usize = 8;
const PLACED_SPRITE_RECORD_49_SIZE: usize = 49;
const PLACED_SPRITE_SECTION_49_FIXED_BYTES: usize = 8;

/// Bit `0x00800000` of a cell tag. Its meaning is **Unknown**.
///
/// **Refuted in gameplay, 2026-09-17.** This constant used to be called
/// `CELL_TAG_FORCED_TEXTURE`, on the corpus reasoning that the bit appears only in `.smp` files
/// and often on exactly a map's perimeter, which looked like the editor's `forcetexture`
/// operation. An attended engine run then forced a texture into all 4,096 cells of a fresh 64x64
/// map with `clearmap`, forced seven more cells individually, and saved: **no saved cell had this
/// bit set.** Forcing a texture does not set it, so it does not mean "forced texture". The
/// reasoning is kept here so nobody re-derives it from the same corpus shape.
///
/// **Observed in gameplay, 2026-09-17: `forcetexture` sets this bit and `resetvisibility` clears
/// it.** Isolated with five fresh maps, one renderer call each -- fresh because once the bit is
/// cleared it stays cleared:
///
/// | sequence | bit, across all 4,096 cells of a 64x64 map |
/// | --- | --- |
/// | `clearmap`, save | **set** |
/// | `clearmap`, `rebuild3dmap`, save | **set** |
/// | `clearmap`, **`resetvisibility`**, save | **clear** |
/// | `clearmap`, `rendermap`, save | **set** |
/// | `clearmap`, `refreshdirty`, save | **set** |
///
/// Not `rebuild3dmap`, which two earlier drafts of this comment proposed. There was never a
/// contradiction between the runs that produced "4,096 of 4,096" and "0 of 4,096"; they are a
/// matched pair separated by the renderer block, and `resetvisibility` inside it is the cause.
///
/// **The meaning is still Unknown.** The operator's name invites reading the bit as visibility
/// state, which would fit the corpus neatly -- it sits on exactly the perimeter ring of 146 `.smp`
/// files. But that is an inference from a name, and the call could as easily reset a generic dirty
/// or cache flag as a side effect. Also unestablished: whether a load or a save touches the bit
/// independently, since every echo save in the `mapload` run followed the renderer block.
///
/// **A writer must preserve the bit and never clear it.** The corpus shows it living on disk in one
/// place -- those 146 `.smp` perimeters -- and no probe has exercised the `.smp` load-and-save path
/// at all, so an editor that dropped it could be destroying its only real occurrence.
///
/// A writer must therefore treat this bit as **cosmetic**: preserving it costs nothing and loses
/// nothing, and setting it achieves nothing the engine will keep.
///
/// It is still masked out of [`MapCell::tile_index`], which is independent of what it means: with
/// the mask every corpus cell indexes a tile in `0..623`, and without it the flagged cells do not.
pub const CELL_TAG_HIGH_FLAG: u32 = 0x0080_0000;

/// One engine terrain type: the tile slot `setterrain` paints with, and its `gs\maplib.gs` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainTypeInfo {
    pub terrain_type: u32,
    pub base_tile: u32,
    pub script_names: &'static [&'static str],
}

/// The engine's terrain-type-to-tile table, read back out of the running game.
///
/// **Observed in gameplay, 2026-09-17.** A probe ran `setterrain` with types `0..=10` along one
/// row of a fresh map and saved it; each painted cell's tag is the `base_tile` below. Names come
/// from `gs\maplib.gs` (**Documented**).
///
/// Together with [`OBSERVED_TILE_TERRAIN_TYPES`] this establishes that a cell's terrain *type* is
/// derived from its tile index through the tileset rather than stored in the cell, which is why
/// tag bits `10..22` are unused across the whole corpus.
///
/// **Qualified 2026-09-17 by the mapload probe: `base_tile` is a *representative* tile of each
/// type, not the tile `setterrain` would write in an arbitrary neighbourhood.** The original
/// measurement painted onto a background forced to tile 392; painting onto a tile-15 background
/// instead makes `setterrain 6` write tiles in `385..391` rather than 15. `setterrain` picks from a
/// family according to the local neighbourhood -- see [`LAND_TRANSITION_TILES`]. The reverse
/// direction is unaffected: `getterrain` on any of these tiles answers the type, and
/// `base_tile_terrain_type` round-trips. What this project's writer does with the table --
/// forcing one representative tile of a type into one cell -- remains exactly right.
pub const TERRAIN_TYPES: [TerrainTypeInfo; 11] = [
    TerrainTypeInfo {
        terrain_type: 0,
        base_tile: 175,
        script_names: &["tt_dirt", "tt_rough"],
    },
    TerrainTypeInfo {
        terrain_type: 1,
        base_tile: 392,
        script_names: &["tt_water"],
    },
    TerrainTypeInfo {
        terrain_type: 2,
        base_tile: 111,
        script_names: &["tt_desert", "tt_sand"],
    },
    TerrainTypeInfo {
        terrain_type: 3,
        base_tile: 159,
        script_names: &["tt_mountain"],
    },
    TerrainTypeInfo {
        terrain_type: 4,
        base_tile: 207,
        script_names: &["tt_happy", "tt_meadow"],
    },
    TerrainTypeInfo {
        terrain_type: 5,
        base_tile: 255,
        script_names: &["tt_ice", "tt_snow"],
    },
    TerrainTypeInfo {
        terrain_type: 6,
        base_tile: 15,
        script_names: &["tt_land", "tt_plains"],
    },
    TerrainTypeInfo {
        terrain_type: 7,
        base_tile: 303,
        script_names: &["tt_swamp"],
    },
    TerrainTypeInfo {
        terrain_type: 8,
        base_tile: 351,
        script_names: &["tt_lava"],
    },
    TerrainTypeInfo {
        terrain_type: 9,
        base_tile: 459,
        script_names: &["tt_road"],
    },
    TerrainTypeInfo {
        terrain_type: 10,
        base_tile: 469,
        script_names: &["tt_impassible", "tt_impassable"],
    },
];

/// `(tile_index, terrain_type)` pairs read back with `getterrain` from cells whose *tile* was
/// forced and whose terrain type was never set.
///
/// **Observed in gameplay, 2026-09-17.** This is the inverse direction of [`TERRAIN_TYPES`]: the
/// engine answered a terrain-type question about a cell that only ever had a tile written to it,
/// so the type must come from the tileset. It is a sample of the live `tilesb01` tileset, not a
/// complete table, and slots that are not a type's `base_tile` (48, 96) still answer with a type.
pub const OBSERVED_TILE_TERRAIN_TYPES: [(u32, u32); 7] =
    [(0, 0), (1, 6), (2, 6), (48, 0), (96, 0), (392, 1), (623, 9)];

/// The tile slot `setterrain` paints for a terrain type, or `None` if the type is out of range.
pub fn terrain_type_base_tile(terrain_type: u32) -> Option<u32> {
    TERRAIN_TYPES
        .iter()
        .find(|entry| entry.terrain_type == terrain_type)
        .map(|entry| entry.base_tile)
}

/// The terrain type whose `setterrain` paints this tile slot, if any.
///
/// This is only the reverse of the painted-base-tile table. It is **not** the tileset's
/// tile-to-terrain lookup, which covers every slot; see [`OBSERVED_TILE_TERRAIN_TYPES`].
pub fn base_tile_terrain_type(tile_index: u32) -> Option<u32> {
    TERRAIN_TYPES
        .iter()
        .find(|entry| entry.base_tile == tile_index)
        .map(|entry| entry.terrain_type)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapCell {
    pub tag: u32,
    pub value_bits: u32,
    pub value: f32,
}

impl MapCell {
    /// The tile-atlas slot this cell paints.
    ///
    /// **Observed in gameplay, 2026-09-17.** Seven cells forced to slots 0, 1, 2, 48, 96, 392 and
    /// 623 with the editor's `forcetexture` round-tripped byte-exact through `savescenariomap`,
    /// including both ends of the range, so the low tag bits *are* the slot. This was previously
    /// only inferred from the fact that masked corpus values land inside the atlas.
    /// Whether two cells hold the same eight bytes.
    ///
    /// NOT `==`. `MapCell` derives `PartialEq` over an `f32`, and `NaN != NaN`, so two cells
    /// copied from identical bytes would compare unequal whenever the elevation is non-finite --
    /// in a diff whose entire job is to say which bytes changed. Every corpus elevation is finite
    /// today; nothing guarantees a generated one is.
    pub fn has_same_bytes(&self, other: &Self) -> bool {
        self.tag == other.tag && self.value_bits == other.value_bits
    }

    pub fn tile_index(&self) -> u32 {
        self.tag & !CELL_TAG_HIGH_FLAG
    }

    /// Whether bit `0x00800000` is set. What that means is Unknown; see [`CELL_TAG_HIGH_FLAG`].
    pub fn high_flag_set(&self) -> bool {
        self.tag & CELL_TAG_HIGH_FLAG != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedSpriteRecord49 {
    pub raw: [u8; PLACED_SPRITE_RECORD_49_SIZE],
    pub record_kind: u32,
    pub record_version: u32,
    pub cell_index: u32,
    pub unknown_12: u32,
    pub unknown_16: u32,
    /// **Observed in gameplay, 2026-09-17:** 200, 201, 202 for the three sprites placed on a fresh
    /// map, so it is a sequential per-map instance id starting at 200 -- which matches the corpus
    /// range of `200..1659`.
    pub instance_id: u32,
    pub attribute_bits: u32,
    /// The terrain sprite type id.
    ///
    /// **Observed in gameplay, 2026-09-17**, promoted from `sprite_type_candidate`: three sprites
    /// of a type freshly minted by `addterrainspritetype` in the same keypress all wrote the id
    /// that operator returned, 470.
    pub sprite_type: u32,
    pub marker_32: u16,
    pub procedure_id_candidate: i32,
    pub unknown_38: u32,
    pub unknown_42: u32,
    pub unknown_46: [u8; 3],
}

impl PlacedSpriteRecord49 {
    /// The top nibble of `attribute_bits`, and a **suspect** reading of it.
    ///
    /// Every record the 2026-09-17 probe wrote carries `attribute_bits == 0x00000001`, for which
    /// this returns 0. A plain small integer read as a top nibble is exactly what that looks like,
    /// so the nibble split is probably wrong. No replacement is asserted here, because one placed
    /// sprite type cannot distinguish the field's layout; read `attribute_bits` directly.
    pub fn attribute_code_candidate(&self) -> u8 {
        (self.attribute_bits >> 28) as u8
    }

    /// The `(x, y)` this record sits on, unpacked from `cell_index` the same way cells are.
    ///
    /// **Corrected 2026-09-17.** This used to divide by the map *height*, on the belief that cells
    /// were X-major. See [`MapAsset::cell_index`] for the measurement that refuted it.
    pub fn coordinates(&self, map_width: u32) -> (u32, u32) {
        (self.cell_index % map_width, self.cell_index / map_width)
    }
}

/// The trailing section: `u32 record_count`, `record_count * 49` bytes of records, `u32 footer`.
///
/// **Observed in gameplay, 2026-09-17, by construction.** The corpus only showed that the section
/// is `count * 49 + 8` bytes long; it could not say which four of those eight fixed bytes came
/// first. Saving the same map with no sprites gave an 8-byte tail of `00000000 01000000`, and with
/// three sprites a 155-byte tail of `03000000` + 147 + `01000000`. So the leading word is the
/// count and the trailing word is a footer that did **not** move when three records were added --
/// it is not a sprite-related count. Its meaning stays Unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedSpriteSection49 {
    pub records: Vec<PlacedSpriteRecord49>,
    pub footer: u32,
    /// The highest instance id this section has held **since it was parsed**.
    ///
    /// **This is not persisted, and it cannot be**: the format has no field for it, and inventing
    /// one would violate the rule that unknown bytes are copied rather than minted. It is rebuilt
    /// from the live records on every parse.
    ///
    /// So the no-reuse property holds only *within one parse*. Each CLI invocation is its own
    /// parse, so `--map-remove-sprite` followed by `--map-place-sprite` **does** reissue the freed
    /// id -- verified on the built binary, where place/place/remove-201/place returns 201 again,
    /// now pointing at a different cell. That is a real limitation of editing one file per process
    /// and it is documented as one in `docs/map-format.md`, not papered over here.
    ///
    /// It is still worth keeping for a caller that makes several edits against one parsed map --
    /// a future interactive editor -- because within that scope it does prevent the dead id from
    /// coming back. Other files reference objects by id, so a reused id can silently re-point an
    /// outside reference at a different object.
    pub(crate) instance_id_high_water: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapAsset {
    pub metadata: u32,
    pub width: u32,
    pub height: u32,
    pub bits_per_pixel: u32,
    pub cells: Vec<MapCell>,
    pub placed_sprites_49: Option<PlacedSpriteSection49>,
    /// The trailing section exactly as it was read.
    ///
    /// Held so that a map whose tail this project does **not** understand -- the 52-byte and
    /// 53-byte families, the 18 unmatched tails -- still writes back byte-for-byte. A writer that
    /// can only round-trip the records it decoded is a writer that silently discards the rest.
    pub trailing_raw: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MapTailLayout {
    pub record_size: usize,
    pub total_fixed_bytes: usize,
}

impl fmt::Display for MapTailLayout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}-byte-records+{}-fixed",
            self.record_size, self.total_fixed_bytes
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapError(String);

impl MapError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for MapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MapError {}

impl MapAsset {
    pub fn parse(source: &[u8]) -> Result<Self, MapError> {
        if source.len() < HEADER_SIZE {
            return Err(MapError::new("map header is truncated"));
        }
        let metadata = read_u32(source, 0)?;
        let width = read_u32(source, 4)?;
        let height = read_u32(source, 8)?;
        let bits_per_pixel = read_u32(source, 12)?;
        if width == 0 || height == 0 {
            return Err(MapError::new("map dimensions must be nonzero"));
        }
        if bits_per_pixel != 8 {
            return Err(MapError::new(format!(
                "unsupported map cell depth {bits_per_pixel}; expected 8"
            )));
        }

        let cell_count = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| MapError::new("map cell count overflow"))?;
        let cell_bytes = cell_count
            .checked_mul(CELL_SIZE)
            .ok_or_else(|| MapError::new("map cell byte count overflow"))?;
        let trailing_offset = HEADER_SIZE
            .checked_add(cell_bytes)
            .ok_or_else(|| MapError::new("map cell offset overflow"))?;
        if trailing_offset > source.len() {
            return Err(MapError::new(format!(
                "map cell grid is truncated: expected {cell_count} cells"
            )));
        }

        let mut cells = Vec::with_capacity(cell_count);
        for index in 0..cell_count {
            let offset = HEADER_SIZE + index * CELL_SIZE;
            let tag = read_u32(source, offset)?;
            let value_bits = read_u32(source, offset + 4)?;
            cells.push(MapCell {
                tag,
                value_bits,
                value: f32::from_bits(value_bits),
            });
        }
        let trailing_bytes = source.len() - trailing_offset;
        let placed_sprites_49 =
            parse_placed_sprites_49(source, trailing_offset, trailing_bytes, cell_count)?;
        let trailing_raw = source[trailing_offset..].to_vec();

        Ok(Self {
            metadata,
            width,
            height,
            bits_per_pixel,
            cells,
            placed_sprites_49,
            trailing_raw,
        })
    }

    /// The packed index of the cell at `(x, y)`: `y * width + x`.
    ///
    /// **Corrected 2026-09-17, from an observation in gameplay.** This was `x * height + y`, and
    /// the whole shipped corpus agrees with that too -- because **every shipped map is square**
    /// (128x128 world maps, 48x48 special maps), which makes the two encodings transposes that no
    /// square file can tell apart. A probe placed three terrain sprites at (20,30), (21,30) and
    /// (20,31), chosen so the two candidate encodings share no value, and the saved records carry
    /// 1940, 1941 and 2004 -- exactly `y * 64 + x`. X-major would have written 1310, 1374, 1311.
    ///
    /// Which operand is *x* was settled separately, from the probe's screen capture, because
    /// operand order and storage order are exact transposes and the bytes alone cannot separate
    /// them: `map2screen` puts screen-x on `(x - y)` and screen-down on `(x + y)`, so a painted
    /// run varying the first operand must travel down-**right** and one varying the second must
    /// travel down-left. Both painted bands run down-right, so the first operand is x.
    ///
    /// Tests that exercise this must use a **non-square** map; a square fixture cannot fail.
    /// Where a placed-sprite record sits, taken from this map rather than a caller's guess.
    ///
    /// `PlacedSpriteRecord49::coordinates` needs the map's **width**, and `map.height` compiles
    /// just as well. That is not hypothetical: `--describe-map` was still passing `height` after
    /// the packing correction had landed everywhere else, and no test caught it, because a square
    /// map cannot tell the two apart -- the same reason the X-major reading survived for months.
    /// Going through the map removes the choice.
    pub fn record_coordinates(&self, record: &PlacedSpriteRecord49) -> (u32, u32) {
        record.coordinates(self.width)
    }

    /// Where the trailing section starts: immediately after the cell grid.
    ///
    /// **Derived, not stored.** This and the three below used to be fields set at parse time, and
    /// they went stale the moment `place_sprite` or `remove_sprite` ran -- `trailing_bytes` would
    /// still report the pre-edit tail length while `to_bytes` wrote the new one. No caller was
    /// wrong yet, which is exactly why it was worth removing: the next one would have been.
    pub fn trailing_offset(&self) -> usize {
        HEADER_SIZE + self.cells.len() * CELL_SIZE
    }

    /// The trailing section as it will be written.
    ///
    /// Comes from the decoded records when this map is in the 49-byte family and from the verbatim
    /// bytes otherwise -- the same choice [`to_bytes`](Self::to_bytes) makes, deliberately, so the
    /// two can never disagree.
    pub fn trailing_section(&self) -> Result<Vec<u8>, MapError> {
        match &self.placed_sprites_49 {
            Some(section) => section.to_bytes(),
            None => Ok(self.trailing_raw.clone()),
        }
    }

    pub fn trailing_bytes(&self) -> usize {
        match &self.placed_sprites_49 {
            Some(section) => {
                PLACED_SPRITE_SECTION_49_FIXED_BYTES
                    + section.records.len() * PLACED_SPRITE_RECORD_49_SIZE
            }
            None => self.trailing_raw.len(),
        }
    }

    /// The first word of the trailing section, which in the decoded family is the record count.
    pub fn trailing_head_u32(&self) -> Option<u32> {
        match &self.placed_sprites_49 {
            Some(section) => u32::try_from(section.records.len()).ok(),
            None => (self.trailing_raw.len() >= 4).then(|| {
                u32::from_le_bytes(
                    self.trailing_raw[0..4]
                        .try_into()
                        .expect("length was just checked"),
                )
            }),
        }
    }

    pub fn cell_index(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        usize::try_from(y)
            .ok()?
            .checked_mul(usize::try_from(self.width).ok()?)?
            .checked_add(usize::try_from(x).ok()?)
    }

    pub fn cell(&self, x: u32, y: u32) -> Option<&MapCell> {
        self.cells.get(self.cell_index(x, y)?)
    }

    pub fn candidate_tail_layouts(&self) -> Vec<MapTailLayout> {
        let Some(count) = self
            .trailing_head_u32()
            .and_then(|count| usize::try_from(count).ok())
        else {
            return Vec::new();
        };
        [(49, 8), (52, 4), (53, 4)]
            .into_iter()
            .filter_map(|(record_size, total_fixed_bytes)| {
                count
                    .checked_mul(record_size)
                    .and_then(|bytes| bytes.checked_add(total_fixed_bytes))
                    .filter(|expected| *expected == self.trailing_bytes())
                    .map(|_| MapTailLayout {
                        record_size,
                        total_fixed_bytes,
                    })
            })
            .collect()
    }
}

fn parse_placed_sprites_49(
    source: &[u8],
    trailing_offset: usize,
    trailing_bytes: usize,
    cell_count: usize,
) -> Result<Option<PlacedSpriteSection49>, MapError> {
    if trailing_bytes < PLACED_SPRITE_SECTION_49_FIXED_BYTES {
        return Ok(None);
    }
    let count = usize::try_from(read_u32(source, trailing_offset)?)
        .map_err(|_| MapError::new("placed-sprite record count exceeds platform limits"))?;
    let expected = count
        .checked_mul(PLACED_SPRITE_RECORD_49_SIZE)
        .and_then(|bytes| bytes.checked_add(PLACED_SPRITE_SECTION_49_FIXED_BYTES))
        .ok_or_else(|| MapError::new("placed-sprite section size overflow"))?;
    if expected != trailing_bytes {
        return Ok(None);
    }

    let mut records = Vec::with_capacity(count);
    let records_offset = trailing_offset + 4;
    for index in 0..count {
        let offset = records_offset + index * PLACED_SPRITE_RECORD_49_SIZE;
        let end = offset + PLACED_SPRITE_RECORD_49_SIZE;
        let raw: [u8; PLACED_SPRITE_RECORD_49_SIZE] = source[offset..end]
            .try_into()
            .expect("placed-sprite record bounds were checked by the section size");
        let cell_index = read_u32(&raw, 8)?;
        if usize::try_from(cell_index).map_or(true, |cell_index| cell_index >= cell_count) {
            return Err(MapError::new(format!(
                "placed-sprite record {index} references out-of-range cell {cell_index}"
            )));
        }
        records.push(PlacedSpriteRecord49 {
            raw,
            record_kind: read_u32(source, offset)?,
            record_version: read_u32(source, offset + 4)?,
            cell_index,
            unknown_12: read_u32(source, offset + 12)?,
            unknown_16: read_u32(source, offset + 16)?,
            instance_id: read_u32(source, offset + 20)?,
            attribute_bits: read_u32(source, offset + 24)?,
            sprite_type: read_u32(source, offset + 28)?,
            marker_32: read_u16(source, offset + 32)?,
            procedure_id_candidate: read_i32(source, offset + 34)?,
            unknown_38: read_u32(source, offset + 38)?,
            unknown_42: read_u32(source, offset + 42)?,
            unknown_46: source[offset + 46..offset + 49]
                .try_into()
                .expect("placed-sprite suffix bounds were checked by the section size"),
        });
    }
    let footer = read_u32(
        source,
        records_offset + count * PLACED_SPRITE_RECORD_49_SIZE,
    )?;
    let instance_id_high_water = records.iter().map(|record| record.instance_id).max();
    Ok(Some(PlacedSpriteSection49 {
        records,
        footer,
        instance_id_high_water,
    }))
}

fn read_u16(source: &[u8], offset: usize) -> Result<u16, MapError> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| MapError::new("map u16 offset overflow"))?;
    let bytes: [u8; 2] = source
        .get(offset..end)
        .ok_or_else(|| MapError::new("map u16 is truncated"))?
        .try_into()
        .expect("map u16 slice length was checked");
    Ok(u16::from_le_bytes(bytes))
}

fn read_i32(source: &[u8], offset: usize) -> Result<i32, MapError> {
    Ok(read_u32(source, offset)? as i32)
}

fn read_u32(source: &[u8], offset: usize) -> Result<u32, MapError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| MapError::new("map u32 offset overflow"))?;
    let bytes: [u8; 4] = source
        .get(offset..end)
        .ok_or_else(|| MapError::new("map u32 is truncated"))?
        .try_into()
        .expect("map u32 slice length was checked");
    Ok(u32::from_le_bytes(bytes))
}

// ---------------------------------------------------------------------------
// Writing
//
// Everything below turns a parsed `MapAsset` back into bytes, and edits it in the terms the
// engine itself uses. Two rules hold throughout, and both exist because most of this format is
// still Unknown:
//
//   1. **When editing existing data, fields whose meaning is unknown are copied, never minted.**
//      The header word at `0x00`, the trailing footer and the record attribute field at `+24` all
//      survive a round trip untouched. A writer that guesses at them would corrupt maps in ways no
//      test here could see.
//
//      **Placing a *new* sprite is the exception, and it necessarily mints.** A record that did not
//      exist has to get bytes from somewhere. `PlacedSpriteRecord49::new` takes them from the one
//      run in which this project watched the engine write a fresh record, and one of them -- the
//      `+24` attribute field -- *contradicts* the corpus reading of that field. That is stated at
//      the constant, in `docs/map-format.md` and in the tool README, because a reader who believes
//      rule 1 unconditionally would believe a placed sprite guessed at nothing.
//   2. **An unedited map re-encodes to the exact input bytes.** That is asserted over the whole
//      installed corpus, not just fixtures, by `--map-roundtrip`.
// ---------------------------------------------------------------------------

/// The `attribute_bits` value the engine wrote for a freshly placed terrain sprite.
///
/// **Observed in gameplay, 2026-09-17.** All three probe records carried `0x00000001`. That
/// *contradicts* the corpus reading of this field, in which only the upper nibble varies across
/// 16,628 records -- see [`PlacedSpriteRecord49::attribute_code_candidate`]. The contradiction is
/// unresolved, so a newly minted record reproduces the one value that was actually observed being
/// written rather than anything derived from the corpus.
pub const FRESH_SPRITE_ATTRIBUTE_BITS: u32 = 1;

/// The first `instance_id` the engine assigns on a map with no placed sprites.
///
/// **Observed in gameplay, 2026-09-17:** three sprites placed on a fresh map got 200, 201, 202.
pub const FIRST_SPRITE_INSTANCE_ID: u32 = 200;

impl PlacedSpriteRecord49 {
    /// A record of the shape the engine writes for a newly placed terrain sprite.
    ///
    /// **This mints.** Nine fields get values that were not copied from anything in the file being
    /// edited, so a placed sprite is the one place the writer's copy-never-mint rule does not hold.
    ///
    /// Eight of the nine are invariant across all 16,628 corpus records, which is as close to safe
    /// as this project can get: `record_kind` and `record_version` are `1`, `+12` and `+42` are
    /// `0xffffffff`, `+16`, `+38` and `+46..49` are zero, and `marker_32` is `0x01ff`. The
    /// procedure id is `-1`, the "no procedure" value the probe's sprites carried.
    ///
    /// The ninth is [`FRESH_SPRITE_ATTRIBUTE_BITS`], and it is **not** corpus-invariant -- it
    /// contradicts the corpus reading of `+24`. See that constant. Whether the engine accepts a
    /// record of this shape is unmeasured; only an attended engine run settles it.
    pub fn new(cell_index: u32, instance_id: u32, sprite_type: u32) -> Self {
        let mut record = Self {
            raw: [0; PLACED_SPRITE_RECORD_49_SIZE],
            record_kind: 1,
            record_version: 1,
            cell_index,
            unknown_12: u32::MAX,
            unknown_16: 0,
            instance_id,
            attribute_bits: FRESH_SPRITE_ATTRIBUTE_BITS,
            sprite_type,
            marker_32: 0x01ff,
            procedure_id_candidate: -1,
            unknown_38: 0,
            unknown_42: u32::MAX,
            unknown_46: [0; 3],
        };
        record.raw = record.to_bytes();
        record
    }

    /// The 49 bytes of this record, rebuilt from its typed fields.
    ///
    /// The typed fields cover all 49 bytes with no gap -- `0..32` as eight `u32`s, `32..34` as the
    /// marker, `34..38` as the procedure id, `38..42`, `42..46`, then the three-byte tail -- so
    /// for a record that came from [`parse`](MapAsset::parse) this reproduces `raw` exactly and
    /// nothing has to be carried over blindly. `roundtrips_every_corpus_record` asserts that over
    /// the installed corpus rather than trusting the arithmetic.
    pub fn to_bytes(&self) -> [u8; PLACED_SPRITE_RECORD_49_SIZE] {
        let mut bytes = [0_u8; PLACED_SPRITE_RECORD_49_SIZE];
        bytes[0..4].copy_from_slice(&self.record_kind.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.record_version.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.cell_index.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.unknown_12.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.unknown_16.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.instance_id.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.attribute_bits.to_le_bytes());
        bytes[28..32].copy_from_slice(&self.sprite_type.to_le_bytes());
        bytes[32..34].copy_from_slice(&self.marker_32.to_le_bytes());
        bytes[34..38].copy_from_slice(&self.procedure_id_candidate.to_le_bytes());
        bytes[38..42].copy_from_slice(&self.unknown_38.to_le_bytes());
        bytes[42..46].copy_from_slice(&self.unknown_42.to_le_bytes());
        bytes[46..49].copy_from_slice(&self.unknown_46);
        bytes
    }
}

impl PlacedSpriteSection49 {
    /// `u32 record_count`, the records, then the `u32` footer.
    ///
    /// **Observed in gameplay, 2026-09-17, by construction.** The corpus could only show that this
    /// section is `count * 49 + 8` bytes; it could not say which four of the eight fixed bytes came
    /// first. Two saves of the same map settled it -- see [`PlacedSpriteSection49`].
    pub fn to_bytes(&self) -> Result<Vec<u8>, MapError> {
        // The count is a `u32` in the file. `as u32` would wrap silently and write a header that
        // disagrees with the records behind it -- a map that parses and is wrong, which is the one
        // outcome this whole module is built to avoid. Unreachable through the CLI (it would take
        // hundreds of gigabytes of records) and cheap to make impossible anyway.
        let count = u32::try_from(self.records.len())
            .map_err(|_| MapError::new("placed-sprite record count exceeds the 32-bit field"))?;
        let mut bytes = Vec::with_capacity(
            PLACED_SPRITE_SECTION_49_FIXED_BYTES + self.records.len() * PLACED_SPRITE_RECORD_49_SIZE,
        );
        bytes.extend_from_slice(&count.to_le_bytes());
        for record in &self.records {
            bytes.extend_from_slice(&record.to_bytes());
        }
        bytes.extend_from_slice(&self.footer.to_le_bytes());
        Ok(bytes)
    }

    /// The id a newly placed sprite should take: one past the highest in use, or 200 on an empty
    /// map.
    ///
    /// The maximum is taken over the live records *and* the high-water mark, so a removed id is
    /// not handed back out **within one parse** -- see
    /// [`instance_id_high_water`](Self::instance_id_high_water), which explains why that scope
    /// cannot be widened and why a sequence of CLI invocations does reissue a freed id.
    pub fn next_instance_id(&self) -> Option<u32> {
        self.records
            .iter()
            .map(|record| record.instance_id)
            .chain(self.instance_id_high_water)
            .max()
            .map_or(Some(FIRST_SPRITE_INSTANCE_ID), |highest| highest.checked_add(1))
    }
}

/// One direction of a `setterrain` transition halo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionTile {
    /// `(dx, dy)` of the halo cell relative to the painted region: `-1` is outside the low edge,
    /// `+1` outside the high edge.
    pub direction: (i32, i32),
    pub tile: u32,
}

/// Superseded by [`transition_ring`], and kept only because [`LAND_BACKGROUND_TILE`] reads better
/// beside it.
///
/// This was the first measurement, taken against one background before the 11x11 matrix existed,
/// and its doc block used to say "this is one background; a complete painter needs the same
/// measurement against each of the other ten". That measurement has since been made. A test asserts
/// this constant still agrees with `transition_ring(6, 1)`, because the repository already learned
/// what two copies of one measured table cost.
pub const LAND_TRANSITION_TILES: [TransitionTile; 8] = [
    TransitionTile { direction: (0, -1), tile: 2 },
    TransitionTile { direction: (0, 1), tile: 1 },
    TransitionTile { direction: (-1, 0), tile: 4 },
    TransitionTile { direction: (1, 0), tile: 3 },
    TransitionTile { direction: (-1, -1), tile: 18 },
    TransitionTile { direction: (1, -1), tile: 19 },
    TransitionTile { direction: (-1, 1), tile: 17 },
    TransitionTile { direction: (1, 1), tile: 16 },
];

pub const LAND_BACKGROUND_TILE: u32 = 15;

/// The header word every map the shipped engine generated carries.
///
/// **Observed in gameplay.** Engine-generated maps wrote `0x6f` at 32, 48, 64, 128, 256 **and**
/// 512, so the word is independent of geometry. Shipped world `.scn` files occupy `0x6c..0x6f`.
/// **Observed in gameplay, 2026-09-17 (mapload probe): the engine rewrites this word on every
/// save, and does not preserve what it read.** `URAK.scn` carries `0x6c`; loading it and saving it
/// straight back out produced `0x6f`. So the word is written from engine state, not carried from the
/// map -- which retires the "stored tileset selector" reading in that form, and is also why a map
/// created with `0x6f` re-saves byte-identically.
pub const GENERATED_HEADER_WORD: u32 = 0x6f;


// ---------------------------------------------------------------------------
// `setterrain` transition tiles, and the sprite-type table
//
// Both measured by the 2026-09-17 `terrainrings` probe: eleven terrains painted onto eleven
// backgrounds, 121 rings, plus a dump of the engine's own `terrainsprites` dict. The tables below
// are GENERATED from that run's saved maps rather than transcribed, because a hand-copied
// measurement is a measurement with an extra failure mode.
// ---------------------------------------------------------------------------

/// The offset from a background's **anchor** tile to the transition tile in each direction.
///
/// **Observed in gameplay, 2026-09-17.** This is the whole rule. The earlier `mapload` run measured
/// one background and produced an eight-tile table; this run measured all eleven and the eight
/// tables turn out to be *one* table plus a per-background anchor:
///
/// ```text
/// background anchor   ring (N S W E NW NE SW SE)
///     6        15      2   1   4   3  18  19  17  16
///     2       111     98  97 100  99 114 115 113 112
///     3       159    146 145 148 147 162 163 161 160
///     ...
/// ```
///
/// Subtract the anchor and every row is identical: `N-13 S-14 W-11 E-12 NW+3 NE+4 SW+2 SE+1`, for
/// **eight of eight blending backgrounds** -- not eleven. Three backgrounds produce no uniform ring
/// at all and are excluded; see [`TransitionBehaviour`].
///
/// **The anchor is defined as `SE - 1`, so read the strength of this carefully.** One free parameter
/// per background is fixed by its SE tile; that leaves the other **seven** offsets, times eight
/// backgrounds, as **56 constraints satisfied by the same seven numbers**. That is what makes it a
/// finding rather than a restatement -- eight independent tables could each be a coincidence, one
/// table that regenerates all eight cannot.
///
/// The independent confirmation is that the anchors then land on
/// `15, 63, 111, 159, 207, 255, 303, 351` -- a contiguous arithmetic run of stride
/// [`TRANSITION_BLOCK_STRIDE`], every one congruent to [`TRANSITION_ANCHOR_RESIDUE`] mod 48.
/// Nothing in "anchor = SE - 1" imposes an arithmetic grid.
///
/// **And for seven of the eight, the anchor simply *is* the terrain's representative tile** --
/// `terrain_type_base_tile` gives 15, 111, 159, 207, 255, 303 and 351 for terrains 6, 2, 3, 4, 5, 7
/// and 8. Water is the sole exception (anchor 63, representative tile 392). An earlier draft
/// highlighted only that exception, which made the anchor look purely fitted when it is mostly
/// predictable -- and being predictable is what lets someone compute an anchor for a background
/// nobody measured. Use [`transition_anchor`] rather than either constant directly.
///
/// **What this does NOT establish** is that the whole atlas is partitioned into 48-tile terrain
/// blocks. Eight transition motifs spaced 48 apart is a statement about those motifs. An atlas
/// parser must not classify every 48-slot region as a terrain block on this evidence.
pub const TRANSITION_RING_OFFSETS: [TransitionOffset; 8] = [
    TransitionOffset { direction: (0, -1), offset: -13 },
    TransitionOffset { direction: (0, 1), offset: -14 },
    TransitionOffset { direction: (-1, 0), offset: -11 },
    TransitionOffset { direction: (1, 0), offset: -12 },
    TransitionOffset { direction: (-1, -1), offset: 3 },
    TransitionOffset { direction: (1, -1), offset: 4 },
    TransitionOffset { direction: (-1, 1), offset: 2 },
    TransitionOffset { direction: (1, 1), offset: 1 },
];

/// One direction of a transition ring, as an offset from the background's anchor tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionOffset {
    /// `(dx, dy)` of the ring cell relative to the painted region.
    pub direction: (i32, i32),
    pub offset: i32,
}

/// The stride between terrain blocks in the tile atlas.
pub const TRANSITION_BLOCK_STRIDE: u32 = 48;

/// Every blending anchor is congruent to this modulo [`TRANSITION_BLOCK_STRIDE`].
pub const TRANSITION_ANCHOR_RESIDUE: u32 = 15;

/// What `setterrain` does to the ring around a region painted onto this background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionBehaviour {
    /// A ring of [`TRANSITION_RING_OFFSETS`] relative to `anchor`, for nine of the eleven painted
    /// terrains. The two that differ are the background's own type -- painting a terrain onto
    /// itself produces no ring at all -- and `tt_road`, which is ragged along every edge.
    Blends { anchor: u32 },
    /// No transition tiles at all: every ring cell keeps the background tile.
    ///
    /// True of `tt_dirt` (0) and `tt_impassible` (10).
    ///
    /// An earlier version of this comment hinted that being **off** the 48-tile grid predicts not
    /// blending, because 175 and 469 are 31 and 37 mod 48. **That is refuted, not merely
    /// unestablished:** *four* representative tiles are off the grid -- 175 (31), 392 (8), 459 (27)
    /// and 469 (37) -- and water's 392 is one of them while water blends perfectly normally. The
    /// hedge "two cases is not a rule" did not cover the premise being false, and the
    /// counter-example was two paragraphs away in the same document.
    NoTransition,
    /// The ring depends on the painted terrain, and only the edges change -- corners keep the
    /// background tile. Observed only for `tt_road` (9) as a background.
    PerPaintedTerrain,
}

/// Per-background transition behaviour, generated from the 2026-09-17 run.
pub const TERRAIN_TRANSITIONS: [TerrainTransition; 11] = [
    TerrainTransition { terrain_type: 0, behaviour: TransitionBehaviour::NoTransition },
    TerrainTransition { terrain_type: 1, behaviour: TransitionBehaviour::Blends { anchor: 63 } },
    TerrainTransition { terrain_type: 2, behaviour: TransitionBehaviour::Blends { anchor: 111 } },
    TerrainTransition { terrain_type: 3, behaviour: TransitionBehaviour::Blends { anchor: 159 } },
    TerrainTransition { terrain_type: 4, behaviour: TransitionBehaviour::Blends { anchor: 207 } },
    TerrainTransition { terrain_type: 5, behaviour: TransitionBehaviour::Blends { anchor: 255 } },
    TerrainTransition { terrain_type: 6, behaviour: TransitionBehaviour::Blends { anchor: 15 } },
    TerrainTransition { terrain_type: 7, behaviour: TransitionBehaviour::Blends { anchor: 303 } },
    TerrainTransition { terrain_type: 8, behaviour: TransitionBehaviour::Blends { anchor: 351 } },
    TerrainTransition { terrain_type: 9, behaviour: TransitionBehaviour::PerPaintedTerrain },
    TerrainTransition { terrain_type: 10, behaviour: TransitionBehaviour::NoTransition },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainTransition {
    pub terrain_type: u32,
    pub behaviour: TransitionBehaviour,
}

/// A terrain's blending anchor, when it has one.
///
/// Seven of the eight are just `terrain_type_base_tile`; water is the exception. Going through this
/// rather than either constant is what keeps a caller from anchoring water on 392.
pub fn transition_anchor(terrain_type: u32) -> Option<u32> {
    TERRAIN_TRANSITIONS
        .iter()
        .find(|entry| entry.terrain_type == terrain_type)
        .and_then(|entry| match entry.behaviour {
            TransitionBehaviour::Blends { anchor } => Some(anchor),
            _ => None,
        })
}

/// The ring `setterrain` writes around a region of `painted` laid on a field of `background`.
///
/// **Both terrains, deliberately.** An earlier signature took only the background, which made the
/// function unable to express its own documented exceptions: `transition_ring(6)` handed a caller
/// the land ring for painting **road** onto land, where the engine writes a ragged `384..390` run,
/// and for painting **land onto land**, where the engine writes nothing at all. A measurement that
/// can be misused into writing the wrong tiles is worth less than one that returns `None`.
///
/// `None` means "this project cannot give you a ring", for one of four reasons:
///
/// - `painted == background` -- the engine writes no ring; the region's own type already matches.
/// - `painted == 9` (`tt_road`) -- ragged along every edge, so there is no per-direction tile.
/// - the background blends nothing (`tt_dirt`, `tt_impassible`).
/// - the background is `tt_road`, whose ring depends on the painted terrain -- use
///   [`road_background_ring`], which is measured.
pub fn transition_ring(background: u32, painted: u32) -> Option<[u32; 8]> {
    if painted == background || painted == ROAD_TERRAIN {
        return None;
    }
    let anchor = transition_anchor(background)?;
    let mut ring = [0_u32; 8];
    for (slot, offset) in ring.iter_mut().zip(TRANSITION_RING_OFFSETS) {
        *slot = u32::try_from(i64::from(anchor) + i64::from(offset.offset)).ok()?;
    }
    Some(ring)
}

/// `tt_road`'s terrain type.
pub const ROAD_TERRAIN: u32 = 9;

/// The ring around a region painted onto a **road** background.
///
/// **Observed in gameplay, 2026-09-17**, and measured for all eleven painted terrains: the four
/// **corners keep the background tile 459**, and the edges are their own offset table around a
/// per-painted-terrain base `a`:
///
/// ```text
/// N = a      S = a - 2      W = a - 1      E = a + 1
/// a = 456                 for painted tt_dirt (0)
/// a = 488 + 16 * (T - 2)  for painted T in 2..=8
/// no ring                 for painted tt_water (1), tt_road (9), tt_impassible (10)
/// ```
///
/// Verified 11 of 11 against `zr9.scn`. This was left uncommitted in the first pass, with the
/// documentation telling a painter it "must special-case road" and giving it nothing to use -- a
/// reviewer pointed out the data was already in the artifacts.
///
/// Returns the eight ring tiles in [`TRANSITION_RING_OFFSETS`] order, corners included.
pub fn road_background_ring(painted: u32) -> Option<[u32; 8]> {
    let base = match painted {
        0 => 456,
        2..=8 => 488 + 16 * (painted - 2),
        _ => return None,
    };
    let corner = terrain_type_base_tile(ROAD_TERRAIN)?;
    let mut ring = [corner; 8];
    for (slot, offset) in ring.iter_mut().zip(TRANSITION_RING_OFFSETS) {
        *slot = match offset.direction {
            (0, -1) => base,
            (0, 1) => base - 2,
            (-1, 0) => base - 1,
            (1, 0) => base + 1,
            _ => corner,
        };
    }
    Some(ring)
}

/// The tile family a painted region's **interior** is filled from, and why it cannot be predicted.
///
/// **Observed in gameplay, 2026-09-17.** The centre of a painted 3x3 -- the only cell with no
/// outside neighbour -- takes a tile from an eight-member family `384 + 8k`, where `k` is the
/// background's block index `(anchor - 15) / 48`. Verified for all eight blending terrains on all
/// eleven backgrounds.
///
/// **And it is randomised.** The same experiment run twice -- terrain 6 on a tile-15 background, the
/// same 3x3 at the same coordinates -- produced centre tile **385** in one run and **390** in the
/// other, while the ring was byte-identical across both. So the interior is decorative variation
/// the engine picks per paint, and **no writer can reproduce it**; that is a property of the engine,
/// not a gap in the measurement.
///
/// This corrects two earlier claims. The documentation said `base_tile` was non-invariant "for
/// terrain 6" and blamed the *background*; it is non-invariant for all nine blending terrains and
/// the variable is the painted region's extent -- `TERRAIN_BASE_TILES` was measured with
/// **single-cell** `setterrain`, which has no interior. And the family was written as `385..391`,
/// which is wrong at both ends for the run it came from.
pub fn interior_tile_family(painted: u32) -> Option<(u32, u32)> {
    let anchor = transition_anchor(painted)?;
    let block = (anchor - TRANSITION_ANCHOR_RESIDUE) / TRANSITION_BLOCK_STRIDE;
    let base = 384 + 8 * block;
    Some((base, base + 7))
}

/// The measured ring behaviour of one terrain as a background, or `None` if it is not a terrain.
pub fn transition_behaviour(terrain_type: u32) -> Option<TransitionBehaviour> {
    TERRAIN_TRANSITIONS
        .iter()
        .find(|entry| entry.terrain_type == terrain_type)
        .map(|entry| entry.behaviour)
}

/// The transition tile for one direction, out of a ring in [`TRANSITION_RING_OFFSETS`] order.
pub fn ring_tile(ring: &[u32; 8], direction: (i32, i32)) -> Option<u32> {
    TRANSITION_RING_OFFSETS
        .iter()
        .position(|entry| entry.direction == direction)
        .and_then(|index| ring.get(index).copied())
}

/// Why a `setterrain`-style paint was refused.
///
/// Refusing is still the point of this type, but the set of cases has **shrunk sharply** now that
/// the paint reads a `.til`. What used to be refused for want of a measurement -- an unrecognised
/// background tile, a non-uniform neighbourhood, any background other than the eight that were
/// measured -- the tileset simply decides, because it declares the `self` column for essentially
/// the whole atlas and a constraint for all eight neighbours of every slot.
///
/// What remains are cases the declared constraints genuinely do not cover. The largest is
/// [`NoMatchingTile`](Self::NoMatchingTile): painting `tt_road` or `tt_impassible` as a rectangle,
/// or painting onto an impassable field, leaves cells with **no** candidate at all, and the engine
/// writes something there anyway -- so imitating it would be invention, not reproduction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaintRefusal {
    NotARectangle {
        rect: (u32, u32, u32, u32),
    },
    OutsideMap {
        rect: (u32, u32, u32, u32),
        width: u32,
        height: u32,
    },
    /// A cell holds a tile the active tileset does not define, so its terrain is unknown.
    ///
    /// **This used to be the usual answer on shipped maps and no longer is.** The old reading asked
    /// whether the tile was one of eleven representative slots; this asks the tileset, which
    /// declares 617 of `tilesb01.til`'s 624. It now fires only on a slot that really is undeclared
    /// -- `tilesb01.til` leaves seven commented out -- or when the map was authored against a
    /// different tileset than the one supplied.
    BackgroundTileUnrecognised {
        at: (u32, u32),
        tile_index: u32,
    },
    /// No tile of the required terrain type accepts a cell's neighbourhood.
    ///
    /// **Derived, 2026-09-17.** Over the `terrainrings` captures this is exactly the road and
    /// impassable cases: painting `tt_road` (9) or `tt_impassible` (10) as a filled rectangle, and
    /// painting anything onto a `tt_impassible` field. `tilesb01.til` gives `tt_impassible` eight
    /// slots, `464..=471`, every one of which demands that all eight neighbours also be
    /// impassable, so a rectangle of it has no legal boundary; and road's slots describe a
    /// *network*, not an area. The engine fills those cells from the block regardless -- 512 cells
    /// across the captures where no constraint was satisfied -- so there is nothing here to
    /// reproduce, only something to invent.
    NoMatchingTile {
        at: (u32, u32),
        terrain_type: u32,
        neighbours: [Option<u32>; 8],
    },
    /// No tileset was supplied, so no cell's terrain can be read and no tile can be re-selected.
    ///
    /// The map file does not name its tileset -- the header word at `0x00` was refuted as a stored
    /// selector -- so the caller has to say which `.til` the map was authored against. There is no
    /// default that would be a measurement rather than a guess.
    TileSetUnknown,
    /// The supplied tileset declares no **tiles** for the terrain type being painted.
    ///
    /// `tilesa01.til` stops at terrain 9 and `tilesb01.til` goes to 10, so this is a real
    /// difference between shipped tilesets rather than a defensive branch. It also catches a
    /// terrain the file lists under `TERRAINTYPE=` and never draws, which is equally unpaintable.
    TerrainTypeNotInTileSet {
        terrain_type: u32,
    },
    /// A ring tile the table produced does not fit the cell tag's tile field. Cannot happen for the
    /// eight measured anchors; it exists so a future anchor edit cannot silently truncate.
    RingTileOutsideTagField {
        tile_index: u32,
    },
}

impl fmt::Display for PaintRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotARectangle { rect: (x0, y0, x1, y1) } => write!(
                formatter,
                "({x0}, {y0})..({x1}, {y1}) is not a rectangle"
            ),
            Self::OutsideMap { rect: (x0, y0, x1, y1), width, height } => write!(
                formatter,
                "({x0}, {y0})..({x1}, {y1}) is outside this {width}x{height} map"
            ),
            Self::BackgroundTileUnrecognised { at: (x, y), tile_index } => write!(
                formatter,
                "tile {tile_index} at ({x}, {y}) is not declared by the supplied tileset, so this \
                 cell's terrain type is unknown; the map may have been authored against a \
                 different .til"
            ),
            Self::NoMatchingTile { at: (x, y), terrain_type, neighbours } => {
                let described: Vec<String> = Direction::ALL
                    .iter()
                    .zip(neighbours)
                    .map(|(direction, neighbour)| {
                        let terrain = neighbour
                            .map_or_else(|| "off-map".to_owned(), |value| value.to_string());
                        format!("{}={terrain}", direction.column_name())
                    })
                    .collect();
                write!(
                    formatter,
                    "no tile of terrain {terrain_type} accepts the neighbourhood at ({x}, {y}) \
                     [{}]; the tileset declares no boundary tile for this shape, and the engine \
                     writes one anyway rather than following its own constraints",
                    described.join(" ")
                )
            }
            Self::TileSetUnknown => formatter.write_str(
                "no tileset was supplied, so no cell's terrain can be read: pass the .til the map \
                 was authored against. The map file does not record which one it is",
            ),
            Self::TerrainTypeNotInTileSet { terrain_type } => write!(
                formatter,
                "the supplied tileset declares no tiles for terrain type {terrain_type}"
            ),
            Self::RingTileOutsideTagField { tile_index } => write!(
                formatter,
                "ring tile {tile_index} does not fit the cell tag's tile field"
            ),
        }
    }
}

impl From<PaintRefusal> for MapError {
    fn from(refusal: PaintRefusal) -> Self {
        Self::new(refusal.to_string())
    }
}

/// One cell a paint re-selects a tile for, and how the tileset decided it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintCell {
    pub x: u32,
    pub y: u32,
    /// The terrain type the cell holds *after* the paint.
    pub terrain_type: u32,
    /// The tile to write.
    pub tile_index: u32,
    /// What the tileset's constraints said. [`TileChoice::Unique`] is the reproducible case.
    pub choice: TileChoice,
    /// How many of this cell's eight neighbours lie off the map.
    ///
    /// Nonzero means the decision leaned on the off-map assumption in
    /// [`NeighbourConstraint::accepts`], which no saved artifact tests.
    pub neighbours_off_map: usize,
}

/// A paint that has been decided against the map but not yet applied.
///
/// Planning is separate from applying so the CLI's pre-write verification can check the bytes it is
/// about to emit against the *same* decision the edit made, the way `flag_region_cells` already is.
/// Two copies of "which cells were meant" could disagree, and then the check would be confirming
/// the wrong thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainPaintPlan {
    /// Inclusive `(x0, y0, x1, y1)`.
    pub rect: (u32, u32, u32, u32),
    pub terrain_type: u32,
    /// The region cells, in packed row order. Always re-selected.
    ///
    /// **Re-selected even when the painted terrain already is the region's terrain.** That is what
    /// the engine does: on every `T == background` row of the `terrainrings` run the ring was left
    /// untouched while the region was rewritten from the terrain's interior family.
    pub region: Vec<PaintCell>,
    /// The ring one cell outside the region, clipped to the map.
    ///
    /// **Empty when the paint changes no cell's terrain type.** A ring cell is re-selected only
    /// because a neighbour's terrain moved; if nothing moved the engine does not touch it, and
    /// neither does this.
    pub ring: Vec<PaintCell>,
}

impl TerrainPaintPlan {
    /// Every cell the paint writes, region then ring.
    pub fn cells(&self) -> impl Iterator<Item = &PaintCell> {
        self.region.iter().chain(self.ring.iter())
    }

    /// Cells where several tiles matched equally, however the choice was then made.
    ///
    /// Most of these are reproducible: see [`drawn_cells`](Self::drawn_cells) for the ones that are
    /// not, which is the number that actually matters to a caller.
    pub fn ambiguous_cells(&self) -> usize {
        self.cells().filter(|cell| cell.choice.is_ambiguous()).count()
    }

    /// Cells the engine would have drawn at random, so this writer's answer is only *a* legal one.
    ///
    /// **This, not `ambiguous_cells`, is the honest measure of how far a painted map can differ
    /// from one the engine would have written.** A cell that kept a tile it already held is
    /// ambiguous and reproducible at the same time -- the engine leaves those alone.
    pub fn drawn_cells(&self) -> usize {
        self.cells()
            .filter(|cell| !cell.choice.is_reproducible())
            .count()
    }

    /// Cells whose decision used the off-map neighbour assumption.
    pub fn cells_touching_a_map_edge(&self) -> usize {
        self.cells().filter(|cell| cell.neighbours_off_map > 0).count()
    }
}

/// What a paint actually did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainPaint {
    pub plan: TerrainPaintPlan,
    /// Cells whose eight bytes changed, region and ring together.
    pub cells_changed: usize,
    /// Ring cells written. Zero when the paint moved no terrain boundary.
    pub ring_cells_written: usize,
}

/// The engine's terrain-sprite-type table: the name a script registers, and the id it gets.
///
/// **Observed in gameplay, 2026-09-17.** `terrainsprites` is a dict keyed by name -- shipped script
/// reads `terrainsprites /barrow get` -- and `forall` enumerated **197** of its entries.
///
/// **This table is 178 of them, and the accounting matters:**
///
/// | kind | count | recorded |
/// | --- | ---: | --- |
/// | plain name-to-id | 178 | here |
/// | array-valued | 9 | [`TERRAIN_SPRITE_ARRAYS`] |
/// | logged a name, no usable value | 8 | [`TERRAIN_SPRITE_NAME_ONLY`] |
/// | **counted but never logged at all** | **1** | **unidentified** |
///
/// 195 rows reached the log; the probe's own iteration counter said **196**. So one dict entry was
/// enumerated and produced nothing -- a `cvs` failure on a key, or a key whose name printed empty.
/// Identifying it needs another keypress. `tools/emit_terrain_tables.py --check` pins that gap at 1
/// so a change becomes a failure.
///
/// The eight name-only entries are almost certainly procedures (`cvs` on a procedure prints nothing
/// useful) but that is an inference; what is measured is that they exist and are not ids.
/// **So this is not a complete dump of the dict** and a tool must not treat it as one.
///
/// Two earlier versions of this comment got the arithmetic wrong -- first claiming the remainder was
/// all arrays, then saying "ten, and similar" off an unbounded log slice that counted two trailing
/// lines as entries. Both were caught by reviewers doing the subtraction. The generator now bounds
/// the slice at both ends and cross-checks against the probe's counter.
///
/// **This is why a map's `sprite_type` field was unusable.** The id is assigned in script execution
/// order across 536 `addterrainspritetype` call sites, so nothing in the file format says which id
/// is a keep and which is a tree. This table is the missing half, and it makes
/// `--map-place-sprite` able to take a name.
///
/// It is **profile-specific.** These ids come from the working GS5R3 script set; a different mod
/// registers different types in a different order and every id here shifts. Re-run the probe
/// against any profile whose maps you intend to edit.
///
/// These are identifiers from the game's own scripts, the same class of measurement as the operator
/// and terrain-type names already recorded in this repository -- not shipped content.
pub const TERRAIN_SPRITE_TYPES: [(&str, u32); 178] = [
    ("castle1", 0),
    ("castle2", 1),
    ("castled", 2),
    ("castlel", 3),
    ("catsku", 4),
    ("cave", 5),
    ("crystb", 6),
    ("crystp", 7),
    ("crystr", 8),
    ("crysty", 9),
    ("dirtpil", 10),
    ("dtree", 11),
    ("horns", 12),
    ("arms", 13),
    ("ice1", 14),
    ("ice", 15),
    ("livil", 16),
    ("minec", 17),
    ("mineg", 18),
    ("minei", 19),
    ("minem", 20),
    ("mtreep", 21),
    ("palm1", 22),
    ("palm", 23),
    ("pine1", 24),
    ("pine", 25),
    ("ribs", 26),
    ("rock2", 27),
    ("rock4", 28),
    ("rock", 29),
    ("rocks1", 30),
    ("rocks3", 31),
    ("spine", 32),
    ("statue1", 33),
    ("statue2", 34),
    ("statue3", 35),
    ("statue4", 36),
    ("steersk", 37),
    ("teeth", 38),
    ("tempd", 39),
    ("tempf", 40),
    ("templ", 41),
    ("tempw", 42),
    ("tower1", 43),
    ("tower2", 44),
    ("tower3", 45),
    ("tree1", 46),
    ("tree2", 47),
    ("tree3", 48),
    ("warock", 49),
    ("wavil", 50),
    ("tree4", 54),
    ("tree5", 55),
    ("tree6", 56),
    ("tree7", 57),
    ("rock1", 58),
    ("rock3", 60),
    ("statue8", 61),
    ("statue9", 62),
    ("eemush1", 63),
    ("eemush2", 64),
    ("eemush3", 65),
    ("eebush1", 66),
    ("eemush4", 67),
    ("eemush5", 68),
    ("aarock1", 70),
    ("aarock2", 71),
    ("aarock3", 72),
    ("rockw1", 73),
    ("rockw2", 74),
    ("rockw3", 75),
    ("listat1", 76),
    ("listat2", 77),
    ("listat3", 78),
    ("livase0", 79),
    ("livase1", 80),
    ("libnch0", 81),
    ("statue", 82),
    ("ruwood", 84),
    ("ruvase", 85),
    ("rustat2", 86),
    ("rurug", 87),
    ("rustat", 88),
    ("orchard", 89),
    ("orchrd", 90),
    ("atkinf", 91),
    ("atkmis", 92),
    ("definf", 93),
    ("defmis", 94),
    ("front_wall", 119),
    ("side_wall", 120),
    ("corner_wall", 121),
    ("front_ladder", 122),
    ("side_ladder", 123),
    ("fitrch1", 124),
    ("fipitt1", 125),
    ("fichns1", 126),
    ("choven1", 127),
    ("chtabl1", 128),
    ("chhide1", 129),
    ("chbrls1", 130),
    ("orbust1", 131),
    ("ortabl1", 132),
    ("orbook1", 133),
    ("orarmr1", 134),
    ("detort1", 143),
    ("deskll1", 144),
    ("defntn1", 145),
    ("wavase1", 146),
    ("wafntn1", 147),
    ("waflwr1", 148),
    ("llwtcht", 149),
    ("ddwtcht", 150),
    ("statue5", 151),
    ("fish", 152),
    ("wwvil", 153),
    ("lmill", 154),
    ("mill", 155),
    ("wwmrkt", 156),
    ("cryst", 157),
    ("cryst1", 158),
    ("cryst2", 159),
    ("brew", 160),
    ("esp03", 161),
    ("hammer", 162),
    ("cart", 163),
    ("atkcmp1", 164),
    ("atkcmp2", 165),
    ("atkcmp3", 166),
    ("defcmp1", 167),
    ("defcmp2", 168),
    ("defcmp3", 169),
    ("airspcr1", 178),
    ("waterspcr1", 179),
    ("barrow", 180),
    ("bridge1", 181),
    ("bridge2", 182),
    ("conwich", 183),
    ("cyccave", 184),
    ("drgcave", 185),
    ("fount", 186),
    ("hbarrow", 187),
    ("hlygrail", 188),
    ("lchcase", 189),
    ("sdung1", 190),
    ("sdung2", 191),
    ("spdrweb", 192),
    ("trllcave", 193),
    ("watower", 194),
    ("stup1", 209),
    ("stdn1", 210),
    ("arch1se", 211),
    ("arch1ne", 212),
    ("arch2se", 213),
    ("arch2ne", 214),
    ("arcj3ne", 215),
    ("arch3se", 216),
    ("cave1", 217),
    ("cave2", 218),
    ("orc", 219),
    ("hermit", 220),
    ("ship", 221),
    ("limrkt", 222),
    ("ddmrkt", 223),
    ("ffmrkt", 224),
    ("cave3", 225),
    ("cave4", 226),
    ("fleemark", 227),
    ("mines", 228),
    ("agx06", 229),
    ("mtreeo", 230),
    ("mtreef", 231),
    ("mtreea", 232),
    ("llvil", 233),
    ("ffvil", 234),
    ("wwhut", 235),
    ("devil", 236),
    ("lirock", 237),
];

/// Entries of `terrainsprites` whose value is an array or a procedure rather than a single id.
///
/// The arrays are the per-faith tables -- `keep_array`, `vilg_array`, `great_temple_array` and
/// `leader_ttype_array` are eight entries each, one per faith, which is exactly the set the random
/// map generator was observed placing. Enumerating them is the obvious next probe and would
/// complete the table.
pub const TERRAIN_SPRITE_ARRAYS: [&str; 9] = [
    "combatterrainspritearray",
    "great_temple_array",
    "keep_array",
    "leader_ttype_array",
    "special_array",
    "special_unit",
    "special_unit2",
    "terrainspritearray",
    "vilg_array",
];

/// Dict entries that logged a name with no value the probe could render.
///
/// Almost certainly procedures. Committed so the accounting above is checkable rather than a
/// sentence, and so the next probe has a list to resolve.
pub const TERRAIN_SPRITE_NAME_ONLY: [&str; 8] = [
    "define_terrain_sprite",
    "great_temple",
    "keep_ttype",
    "leader_ttype",
    "special_ttype",
    "special_utype",
    "special_utype2",
    "vilg_ttype",
];

/// The type id a script-registered terrain sprite name carries.
pub fn terrain_sprite_type(name: &str) -> Option<u32> {
    TERRAIN_SPRITE_TYPES
        .iter()
        .find(|(entry, _)| entry.eq_ignore_ascii_case(name))
        .map(|(_, id)| *id)
}

/// The name registered for a type id, or `None` when no name or **more than one** name holds it.
///
/// The uniqueness check is not decoration. In the dumped GS5R3 table every id is held by exactly one
/// name -- asserted by a test -- but the table is regenerated per profile, and a profile that
/// registered an alias would make a plain `find` return whichever row came first. Reporting a
/// confident wrong name is worse than reporting none, because the name is what a caller uses to
/// decide whether the id is the object they meant.
pub fn terrain_sprite_name(type_id: u32) -> Option<&'static str> {
    let mut matches = TERRAIN_SPRITE_TYPES
        .iter()
        .filter(|(_, id)| *id == type_id)
        .map(|(name, _)| *name);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

/// The largest side length [`MapAsset::create`] will produce.
///
/// `gs\edit\mapgen.gs` offers presets up to 1024 and works in units of 32; the engine was observed
/// accepting 512. Nothing larger has evidence behind it, and an unbounded value dies in the
/// allocator instead of being refused.
pub const MAX_CREATED_DIMENSION: u32 = 1024;

/// The footer the engine wrote for a map with no placed sprites.
///
/// **Observed in gameplay:** an empty save's entire trailing section is the eight bytes
/// `00000000 01000000` — count `0`, footer `1`. Its meaning is Unknown; the value is not.
pub const EMPTY_MAP_FOOTER: u32 = 1;

impl MapAsset {
    /// A new map of `width` x `height`, every cell painted with a terrain type's base tile.
    ///
    /// **This mints nothing.** Every byte pattern it writes is one the engine itself was observed
    /// writing: the header word (see [`GENERATED_HEADER_WORD`]), a cell grid whose encoding is
    /// measured by construction, and the exact eight-byte empty trailing section the engine saved
    /// for a sprite-less map. "Create from scratch" here means *compose observed patterns*, not
    /// *invent values* — which is why it is possible while three fields' meanings are still Unknown.
    ///
    /// Two things it cannot promise, both of which the attended `mapload` probe exists to settle:
    ///
    /// - **The engine has never been asked to load a map this project created.** Round-trip
    ///   identity shows this writer matches the engine's *writer*; it says nothing about its
    ///   *reader*.
    /// - **No map, shipped or engine-generated, has ever been non-square.** A non-square map is the
    ///   first artifact here that no observation covers.
    ///
    /// Elevation is `0.0` everywhere, which is inside the corpus range and is flat by any reading
    /// of a word whose units are Inferred.
    pub fn create(width: u32, height: u32, terrain_type: u32) -> Result<Self, MapError> {
        if width == 0 || height == 0 {
            return Err(MapError::new("map dimensions must be nonzero"));
        }
        // Bounded before anything is allocated. Without this, `--map-create 100000 100000 land`
        // asks for ~1.2 TB and dies in the allocator, losing the refusal message every other bad
        // input here gets.
        if width > MAX_CREATED_DIMENSION || height > MAX_CREATED_DIMENSION {
            return Err(MapError::new(format!(
                "{width}x{height} exceeds {MAX_CREATED_DIMENSION} per side; the shipped editor \
                 generates up to {MAX_CREATED_DIMENSION} and nothing larger has been observed"
            )));
        }
        let tile_index = terrain_type_base_tile(terrain_type).ok_or_else(|| {
            MapError::new(format!("{terrain_type} is not one of the 11 terrain types"))
        })?;
        check_tile_index(tile_index)?;
        let cell_count = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| MapError::new("map cell count overflow"))?;

        let cells = vec![
            MapCell {
                tag: tile_index,
                value_bits: 0.0_f32.to_bits(),
                value: 0.0,
            };
            cell_count
        ];
        let placed_sprites_49 = PlacedSpriteSection49 {
            records: Vec::new(),
            footer: EMPTY_MAP_FOOTER,
            instance_id_high_water: None,
        };
        let trailing_raw = placed_sprites_49.to_bytes()?;

        Ok(Self {
            metadata: GENERATED_HEADER_WORD,
            width,
            height,
            bits_per_pixel: 8,
            cells,
            placed_sprites_49: Some(placed_sprites_49),
            trailing_raw,
        })
    }
}

/// Reject a tile index that no corpus cell could hold.
///
/// The tileset's own capacity is deliberately *not* checked -- `tilesb01.til` declares 624 slots,
/// but that is one tileset's answer rather than the format's, and the active `.til` decides. What
/// is checked is the **tag word's** layout: bits `10..22` are zero across all 1,258,496 corpus
/// cells, so an index of 1024 or more is outside every observed shape, and `0x00800000` is a
/// separate flag whose meaning is Unknown. A fat-fingered `3920` for `392` is the realistic input.
fn check_tile_index(tile_index: u32) -> Result<(), MapError> {
    if tile_index >= TILE_INDEX_LIMIT {
        return Err(MapError::new(format!(
            "tile index {tile_index} is outside 0..{TILE_INDEX_LIMIT}; corpus tag bits 10..22 are \
             unused, so no observed cell holds an index this large"
        )));
    }
    Ok(())
}

/// One past the largest tile index the corpus tag layout can express.
///
/// Corpus tag bits `10..22` are zero in every one of the 1,258,496 cells, so the tile field is the
/// low ten bits.
pub const TILE_INDEX_LIMIT: u32 = 1 << 10;

impl MapAsset {
    /// This map as a complete file.
    ///
    /// The trailing section comes from the decoded records when this map is in the 49-byte family
    /// and from [`trailing_raw`](Self::trailing_raw) otherwise, so the families this project has
    /// not decoded still write back unchanged instead of being dropped.
    pub fn to_bytes(&self) -> Result<Vec<u8>, MapError> {
        let mut bytes = Vec::with_capacity(
            HEADER_SIZE + self.cells.len() * CELL_SIZE + self.trailing_raw.len(),
        );
        bytes.extend_from_slice(&self.metadata.to_le_bytes());
        bytes.extend_from_slice(&self.width.to_le_bytes());
        bytes.extend_from_slice(&self.height.to_le_bytes());
        bytes.extend_from_slice(&self.bits_per_pixel.to_le_bytes());
        for cell in &self.cells {
            bytes.extend_from_slice(&cell.tag.to_le_bytes());
            bytes.extend_from_slice(&cell.value_bits.to_le_bytes());
        }
        bytes.extend_from_slice(&self.trailing_section()?);
        Ok(bytes)
    }

    fn cell_index_checked(&self, x: u32, y: u32) -> Result<usize, MapError> {
        self.cell_index(x, y).ok_or_else(|| {
            MapError::new(format!(
                "({x}, {y}) is outside this {}x{} map",
                self.width, self.height
            ))
        })
    }

    /// Force the tile-atlas slot of one cell, the way the editor's `forcetexture` does.
    ///
    /// **This is `forcetexture`, not `setterrain`.** Exactly one cell changes. The engine's
    /// `setterrain` additionally blends transition tiles into the cell's 8-neighbourhood, and this
    /// project has **not** measured which tiles it blends in, so that operation is deliberately
    /// not offered rather than approximated. See `docs/map-format.md`.
    ///
    /// Bit `0x00800000` of the existing tag is **preserved**, and that is now the
    /// measured-correct choice rather than only the conservative one. `forcetexture` *does* set the
    /// bit -- a `clearmap` save carried it on all 4,096 cells -- and the renderer path clears it.
    /// Sixteen cells handed to the engine with the bit already set came back cleared, but through a
    /// save that also rebuilt the mesh, so which step cleared them is not separated. See
    /// [`CELL_TAG_HIGH_FLAG`]. Preserving a bit whose lifetime this project does not control is the
    /// only option that cannot destroy data.
    pub fn set_tile(&mut self, x: u32, y: u32, tile_index: u32) -> Result<(), MapError> {
        check_tile_index(tile_index)?;
        let index = self.cell_index_checked(x, y)?;
        let cell = &mut self.cells[index];
        cell.tag = (cell.tag & CELL_TAG_HIGH_FLAG) | tile_index;
        Ok(())
    }

    /// Force one cell to a terrain type's base tile.
    ///
    /// The tile comes from the measured terrain-type-to-tile table, so the cell's terrain type
    /// follows: a cell's type is derived from its tile through the tileset and is not stored in
    /// the cell. Like [`set_tile`](Self::set_tile) this writes **one** cell and does not blend.
    pub fn set_terrain(&mut self, x: u32, y: u32, terrain_type: u32) -> Result<(), MapError> {
        let tile_index = terrain_type_base_tile(terrain_type).ok_or_else(|| {
            MapError::new(format!("{terrain_type} is not one of the 11 terrain types"))
        })?;
        self.set_tile(x, y, tile_index)
    }

    /// Force every cell to a terrain type's base tile, the way `clearmap` does.
    pub fn fill_terrain(&mut self, terrain_type: u32) -> Result<(), MapError> {
        let tile_index = terrain_type_base_tile(terrain_type).ok_or_else(|| {
            MapError::new(format!("{terrain_type} is not one of the 11 terrain types"))
        })?;
        check_tile_index(tile_index)?;
        for cell in &mut self.cells {
            cell.tag = (cell.tag & CELL_TAG_HIGH_FLAG) | tile_index;
        }
        Ok(())
    }

    /// Decide a `setterrain`-style paint of a rectangular region, without touching the map.
    ///
    /// **This reads the rule out of the tileset instead of out of a measurement.** The `.til` file
    /// declares, for every atlas slot, which terrain a cell holding it belongs to and what it
    /// requires of all eight neighbours; painting is then: set the region's terrain, and re-select
    /// a tile for every cell whose neighbourhood moved.
    ///
    /// **Derived, 2026-09-17, against `artifacts/engine-probe-captures/terrainrings-20260917`.**
    /// Simulating this procedure over all 121 background-by-painted-terrain rows reproduces the
    /// engine exactly wherever the constraints decide a cell: **2,084 of 2,084** cells with a
    /// single candidate match the tile the engine wrote, zero mismatches. That set includes every
    /// transition ring cell and every perimeter cell of every painted region -- the whole of what
    /// the old eight-entry offset table covered, and more besides.
    ///
    /// Two things it does **not** do, both deliberate:
    ///
    /// - **Region interiors are not the engine's tiles.** A cell all of whose neighbours share its
    ///   terrain matches its terrain's whole eight-slot interior family, and the engine draws among
    ///   them at random -- the same paint run twice gave centre tiles 385 and 390. This writer
    ///   picks by [`TileSelector`], reports the count through
    ///   [`TerrainPaintPlan::ambiguous_cells`], and makes no claim to have reproduced the draw. In
    ///   all 253 ambiguous cells across the captures the engine's tile was *within* the candidate
    ///   set, so the set is right even where the choice cannot be.
    /// - **Road and impassable rectangles are refused**, as
    ///   [`PaintRefusal::NoMatchingTile`], because the tileset declares no boundary tile for them.
    ///
    /// The eight-offset table and the per-background anchors that preceded this are kept in
    /// [`TRANSITION_RING_OFFSETS`] and [`TERRAIN_TRANSITIONS`]: they are an independent measurement
    /// that landed on the same numbers this file declares, and a test holds the two against each
    /// other.
    pub fn plan_terrain_paint(
        &self,
        rect: (u32, u32, u32, u32),
        terrain_type: u32,
        tile_set: &TileSetDefinition,
        selector: TileSelector,
    ) -> Result<TerrainPaintPlan, PaintRefusal> {
        let (x0, y0, x1, y1) = rect;
        if x0 > x1 || y0 > y1 {
            return Err(PaintRefusal::NotARectangle { rect });
        }
        if x1 >= self.width || y1 >= self.height {
            return Err(PaintRefusal::OutsideMap {
                rect,
                width: self.width,
                height: self.height,
            });
        }
        // Tiles, not the `TERRAINTYPE=` list: a terrain the file declares and never draws is as
        // unpaintable as one it never mentions. The fixture's terrain 3 is exactly that shape.
        if !tile_set
            .tiles
            .values()
            .any(|tile| tile.terrain_type == terrain_type)
        {
            return Err(PaintRefusal::TerrainTypeNotInTileSet { terrain_type });
        }

        let in_region = |x: u32, y: u32| x >= x0 && x <= x1 && y >= y0 && y <= y1;

        // The terrain field after the paint, read through the tileset rather than through the
        // eleven-slot representative table. This is the step that used to refuse on shipped maps.
        //
        // It is built for the whole region grown by two: a ring cell's own decision reads *its*
        // eight neighbours, which reach one cell further out than the ring itself.
        let low_x = i64::from(x0) - 2;
        let low_y = i64::from(y0) - 2;
        let high_x = i64::from(x1) + 2;
        let high_y = i64::from(y1) + 2;
        let terrain_at = |x: i64, y: i64| -> Result<Option<u32>, PaintRefusal> {
            let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
                return Ok(None);
            };
            let Some(cell) = self.cell(x, y) else {
                return Ok(None);
            };
            if in_region(x, y) {
                return Ok(Some(terrain_type));
            }
            let tile_index = cell.tile_index();
            tile_set
                .terrain_type_of_tile(tile_index)
                .map(Some)
                .ok_or(PaintRefusal::BackgroundTileUnrecognised {
                    at: (x, y),
                    tile_index,
                })
        };

        // **Which region cells actually change terrain**, not merely whether any does. A ring cell
        // is re-selected because a neighbour's terrain moved; one whose eight neighbours all stayed
        // put has no reason to be touched, and the engine does not touch it -- on every
        // `painted == background` row of the captures the region was rewritten from the interior
        // family while the ring was left exactly as it was.
        //
        // A single global "did anything change" flag got this wrong for a *mixed* region. Painting
        // plains over a rectangle that was already half plains re-selected the whole rectangular
        // ring, and a ring cell holding a non-lowest interior member moved for no terrain reason:
        // tile 387 became 384 at a cell three columns away from the only terrain that moved.
        let mut changed: BTreeSet<(u32, u32)> = BTreeSet::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let tile_index = self
                    .cell(x, y)
                    .ok_or(PaintRefusal::OutsideMap {
                        rect,
                        width: self.width,
                        height: self.height,
                    })?
                    .tile_index();
                let before = tile_set.terrain_type_of_tile(tile_index).ok_or(
                    PaintRefusal::BackgroundTileUnrecognised {
                        at: (x, y),
                        tile_index,
                    },
                )?;
                if before != terrain_type {
                    changed.insert((x, y));
                }
            }
        }
        // Whether a cell has a neighbour whose terrain moved. Its own cell counts: a region cell is
        // always re-selected, and this is only consulted for cells outside the region.
        let touches_a_change = |x: u32, y: u32| {
            Direction::ALL.iter().any(|direction| {
                let (dx, dy) = direction.offset();
                let (Ok(nx), Ok(ny)) = (
                    u32::try_from(i64::from(x) + i64::from(dx)),
                    u32::try_from(i64::from(y) + i64::from(dy)),
                ) else {
                    return false;
                };
                changed.contains(&(nx, ny))
            })
        };

        let select = |x: u32, y: u32, cell_terrain: u32| -> Result<PaintCell, PaintRefusal> {
            let mut failure = None;
            let neighbours = Neighbourhood::from_lookup(|dx, dy| {
                if failure.is_some() {
                    return None;
                }
                match terrain_at(i64::from(x) + i64::from(dx), i64::from(y) + i64::from(dy)) {
                    Ok(terrain) => terrain,
                    Err(refusal) => {
                        failure = Some(refusal);
                        None
                    }
                }
            });
            if let Some(refusal) = failure {
                return Err(refusal);
            }
            // The cell's current tile is handed in, because a cell that already holds a valid tile
            // keeps it -- see `TileChoice::Kept`.
            let current_tile = self.cell(x, y).map(|cell| cell.tile_index());
            let choice =
                tile_set.select_tile(cell_terrain, &neighbours, current_tile, selector, (x, y));
            let tile_index = choice.tile().ok_or_else(|| PaintRefusal::NoMatchingTile {
                at: (x, y),
                terrain_type: cell_terrain,
                neighbours: Direction::ALL.map(|direction| neighbours.get(direction)),
            })?;
            if tile_index >= TILE_INDEX_LIMIT {
                return Err(PaintRefusal::RingTileOutsideTagField { tile_index });
            }
            Ok(PaintCell {
                x,
                y,
                terrain_type: cell_terrain,
                tile_index,
                choice,
                neighbours_off_map: neighbours.off_map(),
            })
        };

        let mut region = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                region.push(select(x, y, terrain_type)?);
            }
        }

        let mut ring = Vec::new();
        for y in low_y + 1..=high_y - 1 {
            for x in low_x + 1..=high_x - 1 {
                let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
                    continue;
                };
                if x >= self.width || y >= self.height || in_region(x, y) {
                    continue;
                }
                if !touches_a_change(x, y) {
                    continue;
                }
                let tile_index = self
                    .cell(x, y)
                    .expect("bounds were just checked")
                    .tile_index();
                let cell_terrain = tile_set.terrain_type_of_tile(tile_index).ok_or(
                    PaintRefusal::BackgroundTileUnrecognised {
                        at: (x, y),
                        tile_index,
                    },
                )?;
                ring.push(select(x, y, cell_terrain)?);
            }
        }

        Ok(TerrainPaintPlan {
            rect,
            terrain_type,
            region,
            ring,
        })
    }

    /// Paint a rectangular region and re-select every tile the paint disturbed.
    ///
    /// Nothing is written on a refusal: the whole decision is made by
    /// [`plan_terrain_paint`](Self::plan_terrain_paint) before the first cell is touched.
    ///
    /// The unknown fields are untouched, as everywhere else in this writer: only the tile field of
    /// the tag moves, tag bit `0x00800000` is preserved per cell, and the elevation word and the
    /// whole trailing section are left exactly as they were read.
    pub fn paint_terrain(
        &mut self,
        rect: (u32, u32, u32, u32),
        terrain_type: u32,
        tile_set: &TileSetDefinition,
        selector: TileSelector,
    ) -> Result<TerrainPaint, MapError> {
        let plan = self.plan_terrain_paint(rect, terrain_type, tile_set, selector)?;
        let mut cells_changed = 0_usize;
        for cell in plan.region.iter().chain(plan.ring.iter()) {
            let index = self.cell_index_checked(cell.x, cell.y)?;
            let before = self.cells[index].tag;
            self.set_tile(cell.x, cell.y, cell.tile_index)?;
            if self.cells[index].tag != before {
                cells_changed += 1;
            }
        }
        let ring_cells_written = plan.ring.len();
        Ok(TerrainPaint {
            plan,
            cells_changed,
            ring_cells_written,
        })
    }

    /// Set or clear tag bit `0x00800000` on one cell.
    ///
    /// **This exists to run an experiment, not because the bit is understood.** Its meaning is
    /// Unknown; what is measured is its placement, and that measurement is total: across the 146
    /// corpus files that carry it, the flagged cells are *exactly* the perimeter ring — every edge
    /// cell, no interior cell, zero mismatches in 146 of 146 files. That makes "map border" the
    /// obvious reading and gives a sharp test the corpus cannot run: set it on an **interior** cell,
    /// hand the map to the engine, and see what the engine does with it.
    ///
    /// No ordinary edit calls this. `set_tile` preserves whatever the bit already was.
    pub fn set_high_flag(&mut self, x: u32, y: u32, set: bool) -> Result<(), MapError> {
        let index = self.cell_index_checked(x, y)?;
        let cell = &mut self.cells[index];
        cell.tag = if set {
            cell.tag | CELL_TAG_HIGH_FLAG
        } else {
            cell.tag & !CELL_TAG_HIGH_FLAG
        };
        Ok(())
    }

    /// The packed indexes of this map's perimeter ring.
    pub fn border_ring(&self) -> Vec<usize> {
        (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .filter(|(x, y)| {
                *x == 0 || *y == 0 || *x == self.width - 1 || *y == self.height - 1
            })
            .filter_map(|(x, y)| self.cell_index(x, y))
            .collect()
    }

    /// Set one cell's second word.
    ///
    /// That word is **Inferred** to be elevation, and its runtime units are Unknown; every corpus
    /// value is a finite `f32` in `0..20`. Non-finite values are rejected because no corpus value
    /// is non-finite and a `NaN` would defeat byte comparisons downstream.
    pub fn set_elevation(&mut self, x: u32, y: u32, value: f32) -> Result<(), MapError> {
        if !value.is_finite() {
            return Err(MapError::new(format!("elevation {value} is not finite")));
        }
        let index = self.cell_index_checked(x, y)?;
        let cell = &mut self.cells[index];
        cell.value = value;
        cell.value_bits = value.to_bits();
        Ok(())
    }

    fn placed_sprites_mut(&mut self) -> Result<&mut PlacedSpriteSection49, MapError> {
        self.placed_sprites_49.as_mut().ok_or_else(|| {
            MapError::new(
                "this map's trailing section is not the decoded 49-byte placed-sprite family, \
                 so its objects cannot be edited",
            )
        })
    }

    /// Place a terrain sprite of `sprite_type` at `(x, y)`, returning its new instance id.
    ///
    /// Refused when a sprite already occupies the cell: `cell_index` is unique across all 16,628
    /// corpus records in every file, so duplicating one would write a record shape the engine has
    /// never been observed to produce.
    pub fn place_sprite(&mut self, x: u32, y: u32, sprite_type: u32) -> Result<u32, MapError> {
        let cell_index = u32::try_from(self.cell_index_checked(x, y)?)
            .map_err(|_| MapError::new("cell index exceeds the 32-bit record field"))?;
        let section = self.placed_sprites_mut()?;
        if section
            .records
            .iter()
            .any(|record| record.cell_index == cell_index)
        {
            return Err(MapError::new(format!(
                "({x}, {y}) already holds a placed sprite"
            )));
        }
        let instance_id = section.next_instance_id().ok_or_else(|| {
            MapError::new("this map has used every instance id up to u32::MAX")
        })?;
        section.instance_id_high_water = Some(instance_id);
        section
            .records
            .push(PlacedSpriteRecord49::new(cell_index, instance_id, sprite_type));
        Ok(instance_id)
    }

    /// Remove the placed sprite with `instance_id`.
    ///
    /// **Observed in gameplay, 2026-09-17:** placing three sprites and then destroying all three
    /// gave a file byte-identical to the one saved before any were placed, so removal really does
    /// leave no residue and this is not an approximation of what the engine does.
    pub fn remove_sprite(&mut self, instance_id: u32) -> Result<(), MapError> {
        let section = self.placed_sprites_mut()?;
        let before = section.records.len();
        section
            .records
            .retain(|record| record.instance_id != instance_id);
        if section.records.len() == before {
            return Err(MapError::new(format!(
                "no placed sprite has instance id {instance_id}"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CELL_TAG_HIGH_FLAG, MapAsset, OBSERVED_TILE_TERRAIN_TYPES, TERRAIN_TYPES,
        base_tile_terrain_type, terrain_type_base_tile,
    };
    use crate::tile::{Neighbourhood, TileChoice, TileSelector, TileSetDefinition};

    /// The cell tag a cell holding `tile` has in these fixtures: the tile plus the unknown bit,
    /// which `uniform_map` sets and every edit must preserve.
    fn tagged(tile: u32) -> u32 {
        tile | CELL_TAG_HIGH_FLAG
    }

    /// Build a deliberately **non-square** map: `width x height` cells whose tag is their own
    /// packed index, plus one 49-byte placed-sprite record on `cell_index`.
    ///
    /// Non-square is the whole point. Every shipped map is square, which is exactly why an X-major
    /// reader survived a full-corpus regression suite for months.
    fn non_square_map_with_record(width: u32, height: u32, cell_index: u32) -> Vec<u8> {
        let mut source = Vec::new();
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&width.to_le_bytes());
        source.extend_from_slice(&height.to_le_bytes());
        source.extend_from_slice(&8_u32.to_le_bytes());
        for index in 0..width * height {
            source.extend_from_slice(&index.to_le_bytes());
            source.extend_from_slice(&0.0_f32.to_bits().to_le_bytes());
        }
        source.extend_from_slice(&1_u32.to_le_bytes());
        source.extend_from_slice(&1_u32.to_le_bytes());
        source.extend_from_slice(&1_u32.to_le_bytes());
        source.extend_from_slice(&cell_index.to_le_bytes());
        source.extend_from_slice(&u32::MAX.to_le_bytes());
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&200_u32.to_le_bytes());
        source.extend_from_slice(&1_u32.to_le_bytes());
        source.extend_from_slice(&470_u32.to_le_bytes());
        source.extend_from_slice(&0x01ff_u16.to_le_bytes());
        source.extend_from_slice(&(-1_i32).to_le_bytes());
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&u32::MAX.to_le_bytes());
        source.extend_from_slice(&[0; 3]);
        source.extend_from_slice(&1_u32.to_le_bytes());
        source
    }

    #[test]
    fn parses_header_cells_and_bounded_trailing_section() {
        let mut source = Vec::new();
        source.extend_from_slice(&42_u32.to_le_bytes());
        source.extend_from_slice(&2_u32.to_le_bytes());
        source.extend_from_slice(&1_u32.to_le_bytes());
        source.extend_from_slice(&8_u32.to_le_bytes());
        source.extend_from_slice(&7_u32.to_le_bytes());
        source.extend_from_slice(&1.5_f32.to_bits().to_le_bytes());
        source.extend_from_slice(&9_u32.to_le_bytes());
        source.extend_from_slice(&(-2.0_f32).to_bits().to_le_bytes());
        source.extend_from_slice(&3_u32.to_le_bytes());
        source.extend_from_slice(&[0xaa, 0xbb]);

        let map = MapAsset::parse(&source).unwrap();

        assert_eq!(map.metadata, 42);
        assert_eq!((map.width, map.height, map.bits_per_pixel), (2, 1, 8));
        assert_eq!(map.cells[0].tag, 7);
        assert_eq!(map.cells[0].value, 1.5);
        assert_eq!(map.cells[1].value_bits, (-2.0_f32).to_bits());
        assert_eq!(map.trailing_offset(), 32);
        assert_eq!(map.trailing_bytes(), 6);
        assert_eq!(map.trailing_head_u32(), Some(3));
        assert!(map.placed_sprites_49.is_none());
    }

    #[test]
    fn rejects_a_truncated_cell_grid() {
        let mut source = Vec::new();
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&2_u32.to_le_bytes());
        source.extend_from_slice(&2_u32.to_le_bytes());
        source.extend_from_slice(&8_u32.to_le_bytes());
        source.extend_from_slice(&[0; 24]);

        assert_eq!(
            MapAsset::parse(&source).unwrap_err().to_string(),
            "map cell grid is truncated: expected 4 cells"
        );
    }

    #[test]
    fn recognizes_only_exact_candidate_tail_record_layouts() {
        let mut source = Vec::new();
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&1_u32.to_le_bytes());
        source.extend_from_slice(&1_u32.to_le_bytes());
        source.extend_from_slice(&8_u32.to_le_bytes());
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&0.0_f32.to_bits().to_le_bytes());
        source.extend_from_slice(&2_u32.to_le_bytes());
        source.extend_from_slice(&[0; 98]);
        source.extend_from_slice(&0_u32.to_le_bytes());

        let map = MapAsset::parse(&source).unwrap();
        let layouts = map.candidate_tail_layouts();

        assert_eq!(layouts.len(), 1);
        assert_eq!(layouts[0].record_size, 49);
        assert_eq!(layouts[0].total_fixed_bytes, 8);
    }

    #[test]
    fn masks_the_unexplained_high_tag_flag_out_of_the_tile_index() {
        let flagged = super::MapCell {
            tag: CELL_TAG_HIGH_FLAG | 619,
            value_bits: 0,
            value: 0.0,
        };
        let forced = super::MapCell {
            tag: 623,
            value_bits: 0,
            value: 0.0,
        };

        assert_eq!(flagged.tile_index(), 619);
        assert!(flagged.high_flag_set());
        assert_eq!(flagged.tag, 0x0080_026b);
        // Observed in gameplay, 2026-09-17: a cell the editor forced a texture into carries the
        // bare slot and no high flag. Asserting the flag here would re-assert the refuted claim.
        assert_eq!(forced.tile_index(), 623);
        assert!(!forced.high_flag_set());
    }

    #[test]
    fn paints_each_terrain_type_with_the_tile_the_engine_used() {
        // Observed in gameplay, 2026-09-17: `setterrain` 0..=10 along one row of a fresh map.
        let observed = [
            (0, 175),
            (1, 392),
            (2, 111),
            (3, 159),
            (4, 207),
            (5, 255),
            (6, 15),
            (7, 303),
            (8, 351),
            (9, 459),
            (10, 469),
        ];

        for (terrain_type, base_tile) in observed {
            assert_eq!(terrain_type_base_tile(terrain_type), Some(base_tile));
            assert_eq!(base_tile_terrain_type(base_tile), Some(terrain_type));
        }
        assert_eq!(TERRAIN_TYPES.len(), observed.len());
        assert_eq!(terrain_type_base_tile(11), None);
        assert_eq!(base_tile_terrain_type(1), None);
        for entry in TERRAIN_TYPES {
            assert!(!entry.script_names.is_empty());
        }
    }

    #[test]
    fn reads_terrain_types_back_out_of_tiles_that_were_never_given_one() {
        // Observed in gameplay, 2026-09-17: `getterrain` on cells whose tile alone was forced.
        // Terrain type is therefore derived from the tile through the tileset, not stored in the
        // cell -- so these samples must not be reconstructible from the painted-tile table alone.
        for (tile_index, terrain_type) in OBSERVED_TILE_TERRAIN_TYPES {
            assert!(terrain_type <= 10, "{tile_index} answered {terrain_type}");
            if let Some(painted) = base_tile_terrain_type(tile_index) {
                assert_eq!(painted, terrain_type, "tile {tile_index} disagrees");
            }
        }
        assert_eq!(base_tile_terrain_type(392), Some(1));
        assert_eq!(base_tile_terrain_type(48), None);
    }

    #[test]
    fn reads_the_trailing_count_first_and_the_footer_last() {
        // Observed in gameplay, 2026-09-17: the same map saved with no sprites has an 8-byte tail
        // of `00000000 01000000`, and with three sprites a 155-byte tail of `03000000` + 147 +
        // `01000000`. The count leads, the footer trails, and the footer did not move.
        let mut empty = Vec::new();
        empty.extend_from_slice(&0_u32.to_le_bytes());
        empty.extend_from_slice(&5_u32.to_le_bytes());
        empty.extend_from_slice(&3_u32.to_le_bytes());
        empty.extend_from_slice(&8_u32.to_le_bytes());
        empty.extend_from_slice(&[0; 120]);
        empty.extend_from_slice(&0_u32.to_le_bytes());
        empty.extend_from_slice(&1_u32.to_le_bytes());

        let map = MapAsset::parse(&empty).unwrap();
        let section = map.placed_sprites_49.as_ref().unwrap();

        assert_eq!(map.trailing_bytes(), 8);
        assert!(section.records.is_empty());
        assert_eq!(section.footer, 1);

        let populated = non_square_map_with_record(5, 3, 7);
        let map = MapAsset::parse(&populated).unwrap();
        let section = map.placed_sprites_49.as_ref().unwrap();

        assert_eq!(map.trailing_bytes(), 4 + 49 + 4);
        assert_eq!(section.records.len(), 1);
        assert_eq!(section.footer, 1);
        assert_eq!(section.records[0].sprite_type, 470);
        assert_eq!(section.records[0].instance_id, 200);
    }

    #[test]
    fn indexes_cells_packed_y_major_on_a_non_square_map() {
        // 5 wide, 3 tall: `y * width + x` and the refuted `x * height + y` disagree everywhere off
        // the diagonal. A square fixture agrees with both and so can never fail this.
        let source = non_square_map_with_record(5, 3, 0);
        let map = MapAsset::parse(&source).unwrap();

        assert_eq!(map.cell_index(3, 1), Some(8));
        assert_eq!(map.cell(3, 1).unwrap().tag, 8);
        assert_eq!(map.cell_index(1, 2), Some(11));
        assert_eq!(map.cell(1, 2).unwrap().tag, 11);
        assert_eq!(map.cell_index(4, 2), Some(14));
        assert!(map.cell(3, 4).is_none());
        assert!(map.cell(5, 0).is_none());
        for y in 0..map.height {
            for x in 0..map.width {
                assert_eq!(map.cell_index(x, y), Some((y * map.width + x) as usize));
            }
        }
    }

    #[test]
    fn unpacks_record_coordinates_the_same_way_cells_are_indexed() {
        // Observed in gameplay, 2026-09-17: sprites placed at (20,30),(21,30),(20,31) on a 64x64
        // map wrote cell indexes 1940, 1941, 2004 -- `y * 64 + x`, not `x * 64 + y`.
        let source = non_square_map_with_record(5, 3, 11);
        let map = MapAsset::parse(&source).unwrap();
        let record = &map.placed_sprites_49.as_ref().unwrap().records[0];

        assert_eq!(record.coordinates(map.width), (1, 2));
        assert_eq!(
            map.cell_index(1, 2),
            Some(record.cell_index as usize),
            "record coordinates must round-trip through the cell index"
        );
        for index in 0..map.cells.len() as u32 {
            let source = non_square_map_with_record(5, 3, index);
            let map = MapAsset::parse(&source).unwrap();
            let record = &map.placed_sprites_49.as_ref().unwrap().records[0];
            let (x, y) = record.coordinates(map.width);
            assert_eq!(map.cell_index(x, y), Some(index as usize));
        }
    }

    #[test]
    fn parses_49_byte_placed_sprite_records_and_packed_coordinates() {
        let mut source = Vec::new();
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&3_u32.to_le_bytes());
        source.extend_from_slice(&2_u32.to_le_bytes());
        source.extend_from_slice(&8_u32.to_le_bytes());
        source.extend_from_slice(&[0; 48]);
        source.extend_from_slice(&1_u32.to_le_bytes());
        source.extend_from_slice(&1_u32.to_le_bytes());
        source.extend_from_slice(&1_u32.to_le_bytes());
        source.extend_from_slice(&5_u32.to_le_bytes());
        source.extend_from_slice(&u32::MAX.to_le_bytes());
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&207_u32.to_le_bytes());
        source.extend_from_slice(&0xa000_0000_u32.to_le_bytes());
        source.extend_from_slice(&105_u32.to_le_bytes());
        source.extend_from_slice(&0x01ff_u16.to_le_bytes());
        source.extend_from_slice(&12_i32.to_le_bytes());
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&u32::MAX.to_le_bytes());
        source.extend_from_slice(&[0; 3]);
        source.extend_from_slice(&3_u32.to_le_bytes());

        let map = MapAsset::parse(&source).unwrap();
        let section = map.placed_sprites_49.as_ref().unwrap();
        let record = &section.records[0];

        assert_eq!(record.cell_index, 5);
        assert_eq!(record.coordinates(map.width), (2, 1));
        assert_eq!(record.instance_id, 207);
        assert_eq!(record.attribute_code_candidate(), 10);
        assert_eq!(record.sprite_type, 105);
        assert_eq!(record.procedure_id_candidate, 12);
        assert_eq!(record.raw.len(), 49);
        assert_eq!(section.footer, 3);
        assert_eq!(map.cell(2, 1), Some(&map.cells[5]));
        assert!(map.cell(3, 0).is_none());
    }

    #[test]
    fn record_coordinates_come_from_the_map_not_a_caller_supplied_dimension() {
        // 5 wide, 3 tall. Under the corrected packing, index 7 is (2, 1). Passing `height` (3)
        // instead of `width` (5) yields (1, 2) -- a y equal to the height, which cannot exist.
        // On any square map the two agree, which is exactly why this went unnoticed until a
        // reviewer looked, and why this fixture is deliberately not square.
        let map = MapAsset::parse(&non_square_map_with_one_record(7)).expect("parse");
        let section = map
            .placed_sprites_49
            .as_ref()
            .expect("the fixture writes one 49-byte record");
        let record = &section.records[0];
        assert_eq!(map.record_coordinates(record), (2, 1));
        assert_eq!(record.coordinates(map.width), (2, 1));
        assert_ne!(record.coordinates(map.height), (2, 1));
        let (_, y) = map.record_coordinates(record);
        assert!(y < map.height, "a record cannot sit outside the map");
    }

    fn non_square_map_with_one_record(cell_index: u32) -> Vec<u8> {
        let (width, height) = (5_u32, 3_u32);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x6f_u32.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&8_u32.to_le_bytes());
        for _ in 0..width * height {
            bytes.extend_from_slice(&0_u32.to_le_bytes());
            bytes.extend_from_slice(&0_f32.to_le_bytes());
        }
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        let mut record = [0_u8; 49];
        record[0..4].copy_from_slice(&1_u32.to_le_bytes());
        record[4..8].copy_from_slice(&1_u32.to_le_bytes());
        record[8..12].copy_from_slice(&cell_index.to_le_bytes());
        record[12..16].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
        record[32..34].copy_from_slice(&0x01ff_u16.to_le_bytes());
        record[34..38].copy_from_slice(&(-1_i32).to_le_bytes());
        record[42..46].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
        bytes.extend_from_slice(&record);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes
    }

    #[test]
    fn cells_with_a_non_finite_elevation_still_compare_equal_to_themselves() {
        let quiet_nan = super::MapCell {
            tag: 392,
            value_bits: 0x7fc0_0000,
            value: f32::from_bits(0x7fc0_0000),
        };
        assert!(quiet_nan.value.is_nan());
        assert!(quiet_nan.has_same_bytes(&quiet_nan));
        // The derived comparison is what a diff would reach for, and it is wrong here.
        assert_ne!(quiet_nan, quiet_nan);
        let other = super::MapCell {
            tag: 392,
            value_bits: 0x7fc0_0001,
            value: f32::from_bits(0x7fc0_0001),
        };
        assert!(!quiet_nan.has_same_bytes(&other));
    }

    /// A map whose trailing section this project does **not** decode.
    ///
    /// Deliberately `3x2` and deliberately opaque-tailed: the writer must reproduce tails it never
    /// understood, and a fixture that only exercises the decoded 49-byte family would never say so.
    fn non_square_map_with_opaque_tail(width: u32, height: u32) -> Vec<u8> {
        let mut source = Vec::new();
        source.extend_from_slice(&0x6c_u32.to_le_bytes());
        source.extend_from_slice(&width.to_le_bytes());
        source.extend_from_slice(&height.to_le_bytes());
        source.extend_from_slice(&8_u32.to_le_bytes());
        for index in 0..width * height {
            source.extend_from_slice(&(index | CELL_TAG_HIGH_FLAG).to_le_bytes());
            source.extend_from_slice(&(index as f32).to_bits().to_le_bytes());
        }
        // Not a multiple of any known record size, with a leading word that is not a usable count.
        source.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 0x03]);
        source
    }

    #[test]
    fn an_unedited_map_re_encodes_to_the_input_bytes() {
        let source = non_square_map_with_record(5, 3, 7);
        let map = MapAsset::parse(&source).unwrap();
        assert_eq!(map.to_bytes().unwrap(), source);
    }

    #[test]
    fn a_tail_this_project_cannot_decode_still_writes_back_unchanged() {
        let source = non_square_map_with_opaque_tail(3, 2);
        let map = MapAsset::parse(&source).unwrap();
        assert!(
            map.placed_sprites_49.is_none(),
            "the fixture must exercise the undecoded path"
        );
        assert_eq!(map.to_bytes().unwrap(), source);
    }

    #[test]
    fn a_parsed_record_rebuilds_its_own_bytes_from_its_fields() {
        let source = non_square_map_with_record(5, 3, 7);
        let map = MapAsset::parse(&source).unwrap();
        let record = &map.placed_sprites_49.as_ref().unwrap().records[0];
        assert_eq!(record.to_bytes(), record.raw);
    }

    /// The edit must land at `16 + (y * width + x) * 8`, checked as a byte offset rather than
    /// through the same `cell_index` the writer used.
    ///
    /// Non-square on purpose, and asserted against the *bytes*: going back through `map.cell(x, y)`
    /// would agree with an X-major writer just as happily, which is exactly how the transposed
    /// reading survived a full-corpus suite for months.
    #[test]
    fn an_edited_cell_lands_at_the_y_major_byte_offset() {
        let width = 5;
        let height = 3;
        let (x, y) = (3, 2);
        let source = non_square_map_with_record(width, height, 0);
        let mut map = MapAsset::parse(&source).unwrap();
        map.set_tile(x, y, 392).unwrap();
        let bytes = map.to_bytes().unwrap();

        let expected_offset = 16 + ((y * width + x) as usize) * 8;
        assert_eq!(
            u32::from_le_bytes(bytes[expected_offset..expected_offset + 4].try_into().unwrap()),
            392
        );
        let transposed_offset = 16 + ((x * height + y) as usize) * 8;
        assert_ne!(
            transposed_offset, expected_offset,
            "the fixture must distinguish the two packings"
        );
        assert_ne!(
            u32::from_le_bytes(
                bytes[transposed_offset..transposed_offset + 4]
                    .try_into()
                    .unwrap()
            ),
            392
        );
    }

    #[test]
    fn forcing_a_tile_preserves_the_unknown_high_bit() {
        let source = non_square_map_with_opaque_tail(3, 2);
        let mut map = MapAsset::parse(&source).unwrap();
        assert!(map.cell(1, 1).unwrap().high_flag_set());
        map.set_tile(1, 1, 392).unwrap();
        let cell = map.cell(1, 1).unwrap();
        assert_eq!(cell.tile_index(), 392);
        assert!(
            cell.high_flag_set(),
            "bit 0x00800000 means something unknown; an edit must not silently drop it"
        );
    }

    #[test]
    fn a_tile_index_that_collides_with_the_high_bit_is_rejected() {
        let source = non_square_map_with_record(5, 3, 0);
        let mut map = MapAsset::parse(&source).unwrap();
        assert!(map.set_tile(0, 0, CELL_TAG_HIGH_FLAG | 1).is_err());
    }

    #[test]
    fn every_terrain_type_writes_its_measured_base_tile() {
        let source = non_square_map_with_record(11, 3, 0);
        let mut map = MapAsset::parse(&source).unwrap();
        for entry in TERRAIN_TYPES {
            map.set_terrain(entry.terrain_type, 1, entry.terrain_type)
                .unwrap();
            assert_eq!(
                map.cell(entry.terrain_type, 1).unwrap().tile_index(),
                entry.base_tile
            );
        }
        assert!(map.set_terrain(0, 0, 11).is_err());
    }

    /// `fill_terrain` repeats `set_tile`'s high-bit preservation, so it needs its own test on a
    /// fixture that actually has the bit set -- otherwise the duplicated line can be deleted and
    /// the suite stays green.
    /// The halo is a direction table, and the probe measured all eight directions exactly once.
    /// The one table must reproduce every measured ring, or it is not the rule.
    ///
    /// These eight rings came off the 2026-09-17 saved maps. Reproducing all eight from a single
    /// offset table plus an anchor is the entire claim -- eight independent tables could each be a
    /// coincidence; one table that regenerates all eight cannot.
    #[test]
    fn one_offset_table_reproduces_every_measured_ring() {
        // (background terrain, the ring as read from zr{n}.scn, in N S W E NW NE SW SE order)
        const MEASURED: [(u32, [u32; 8]); 8] = [
            (1, [50, 49, 52, 51, 66, 67, 65, 64]),
            (2, [98, 97, 100, 99, 114, 115, 113, 112]),
            (3, [146, 145, 148, 147, 162, 163, 161, 160]),
            (4, [194, 193, 196, 195, 210, 211, 209, 208]),
            (5, [242, 241, 244, 243, 258, 259, 257, 256]),
            (6, [2, 1, 4, 3, 18, 19, 17, 16]),
            (7, [290, 289, 292, 291, 306, 307, 305, 304]),
            (8, [338, 337, 340, 339, 354, 355, 353, 352]),
        ];
        for (background, expected) in MEASURED {
            // Any painted terrain other than this background and road gives the same ring -- that
            // is the finding -- so assert it for every one of them rather than a favourite.
            for painted in 0..=10 {
                if painted == background || painted == super::ROAD_TERRAIN {
                    continue;
                }
                assert_eq!(
                    super::transition_ring(background, painted),
                    Some(expected),
                    "background {background} painted with {painted}"
                );
            }
            // And the two exceptions must refuse rather than hand back a plausible ring.
            assert_eq!(super::transition_ring(background, background), None, "diagonal");
            assert_eq!(
                super::transition_ring(background, super::ROAD_TERRAIN),
                None,
                "road is ragged, so there is no per-direction ring"
            );
        }
        // The three backgrounds with no uniform ring: dirt and impassible blend nothing, road
        // depends on the painted terrain and has its own measured table.
        for background in [0, 9, 10] {
            assert_eq!(super::transition_ring(background, 6), None, "{background}");
        }
        assert_eq!(super::transition_ring(11, 6), None, "not a terrain type");
    }

    /// Road as a background is a second measured table, verified 11 of 11 against `zr9.scn`.
    #[test]
    fn the_road_background_ring_reproduces_its_measured_edges() {
        // (painted terrain, N, S, W, E) read off zr9.scn; corners are always the background.
        const MEASURED: [(u32, u32, u32, u32, u32); 8] = [
            (0, 456, 454, 455, 457),
            (2, 488, 486, 487, 489),
            (3, 504, 502, 503, 505),
            (4, 520, 518, 519, 521),
            (5, 536, 534, 535, 537),
            (6, 552, 550, 551, 553),
            (7, 568, 566, 567, 569),
            (8, 584, 582, 583, 585),
        ];
        let corner = terrain_type_base_tile(super::ROAD_TERRAIN).unwrap();
        assert_eq!(corner, 459);
        for (painted, north, south, west, east) in MEASURED {
            let ring = super::road_background_ring(painted)
                .unwrap_or_else(|| panic!("painted {painted} should have a road ring"));
            assert_eq!(ring, [north, south, west, east, corner, corner, corner, corner]);
        }
        // Water, road and impassible produce no ring on a road background.
        for painted in [1, 9, 10] {
            assert_eq!(super::road_background_ring(painted), None, "{painted}");
        }
    }

    /// The interior family, and the fact that it cannot be reproduced.
    #[test]
    fn the_interior_family_follows_the_block_index() {
        // (painted terrain, family low) -- low = 384 + 8 * (anchor - 15) / 48.
        for (painted, low) in [(6, 384), (1, 392), (2, 400), (3, 408), (4, 416), (5, 424), (7, 432), (8, 440)] {
            assert_eq!(super::interior_tile_family(painted), Some((low, low + 7)), "{painted}");
        }
        // Non-blending terrains have no family, because they have no anchor.
        for painted in [0, 9, 10, 11] {
            assert_eq!(super::interior_tile_family(painted), None, "{painted}");
        }
        // The two runs that measured terrain 6 on a tile-15 background produced 385 and 390 for
        // the same centre cell. Both are in the family; neither is predictable. A writer that
        // claimed to reproduce an interior would be claiming to reproduce a random draw.
        let (low, high) = super::interior_tile_family(6).unwrap();
        for observed in [385_u32, 390] {
            assert!((low..=high).contains(&observed), "{observed}");
        }
        assert_ne!(385, 390, "the point is that these differ between runs");
    }

    /// The first measured table must still agree with the general rule.
    #[test]
    fn the_land_constant_agrees_with_the_general_ring() {
        let ring = super::transition_ring(6, 1).unwrap();
        for (entry, tile) in super::LAND_TRANSITION_TILES.iter().zip(ring) {
            assert_eq!(entry.tile, tile, "direction {:?}", entry.direction);
        }
        assert_eq!(super::LAND_BACKGROUND_TILE, terrain_type_base_tile(6).unwrap());
    }

    #[test]
    fn every_blending_anchor_sits_on_the_atlas_block_grid() {
        let anchors: Vec<u32> = super::TERRAIN_TRANSITIONS
            .iter()
            .filter_map(|entry| match entry.behaviour {
                super::TransitionBehaviour::Blends { anchor } => Some(anchor),
                _ => None,
            })
            .collect();
        assert_eq!(anchors.len(), 8, "eight backgrounds blend");
        for anchor in &anchors {
            assert_eq!(
                anchor % super::TRANSITION_BLOCK_STRIDE,
                super::TRANSITION_ANCHOR_RESIDUE,
                "anchor {anchor} is off the block grid"
            );
        }
        let mut sorted = anchors.clone();
        sorted.sort_unstable();
        // A contiguous run of blocks 0..7, which is what makes "the atlas is laid out in 48-tile
        // terrain blocks" a statement about the atlas rather than about eight loose numbers.
        for (index, anchor) in sorted.iter().enumerate() {
            let expected = super::TRANSITION_ANCHOR_RESIDUE
                + super::TRANSITION_BLOCK_STRIDE * u32::try_from(index).unwrap();
            assert_eq!(*anchor, expected);
        }
        // Seven of the eight anchors ARE the terrain's representative tile; water is the sole
        // exception, and conflating the two would paint water transitions from the wrong block.
        let mut same = 0;
        for entry in super::TERRAIN_TRANSITIONS {
            if let Some(anchor) = super::transition_anchor(entry.terrain_type)
                && terrain_type_base_tile(entry.terrain_type) == Some(anchor)
            {
                same += 1;
            }
        }
        assert_eq!(same, 7, "seven anchors match the representative tile");
        assert_eq!(terrain_type_base_tile(1), Some(392));
        assert_eq!(super::transition_anchor(1), Some(63), "water is the exception");
    }

    #[test]
    fn the_offset_table_covers_every_neighbour_exactly_once() {
        let directions: std::collections::BTreeSet<(i32, i32)> = super::TRANSITION_RING_OFFSETS
            .iter()
            .map(|entry| entry.direction)
            .collect();
        let expected: std::collections::BTreeSet<(i32, i32)> = (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
            .filter(|(dx, dy)| *dx != 0 || *dy != 0)
            .collect();
        assert_eq!(directions, expected);
        let offsets: std::collections::BTreeSet<i32> =
            super::TRANSITION_RING_OFFSETS.iter().map(|e| e.offset).collect();
        assert_eq!(offsets.len(), 8, "eight directions, eight distinct offsets");
    }

    #[test]
    fn the_sprite_type_table_resolves_names_in_both_directions() {
        assert_eq!(super::terrain_sprite_type("castle1"), Some(0));
        assert_eq!(super::terrain_sprite_type("CASTLE1"), Some(0), "case-insensitive");
        assert_eq!(super::terrain_sprite_name(0), Some("castle1"));
        assert_eq!(super::terrain_sprite_type("not_a_sprite"), None);
        // Ids are unique, names are unique: 178 of each, which is what the run reported.
        let ids: std::collections::BTreeSet<u32> =
            super::TERRAIN_SPRITE_TYPES.iter().map(|(_, id)| *id).collect();
        let names: std::collections::BTreeSet<&str> =
            super::TERRAIN_SPRITE_TYPES.iter().map(|(name, _)| *name).collect();
        assert_eq!(ids.len(), 178);
        assert_eq!(names.len(), 178);
        // Every id round-trips through its own name.
        for (name, id) in super::TERRAIN_SPRITE_TYPES {
            assert_eq!(super::terrain_sprite_type(name), Some(id));
            assert_eq!(super::terrain_sprite_name(id), Some(name));
        }
        // The arrays are listed but deliberately not resolved -- they are per-faith tables and
        // enumerating them is a separate probe.
        assert!(super::TERRAIN_SPRITE_ARRAYS.contains(&"keep_array"));
        assert_eq!(super::terrain_sprite_type("keep_array"), None);
    }

    /// The table has gaps, shipped maps use them heavily, and that is not a misread.
    ///
    /// An earlier version of this test carried a comment claiming every corpus sprite id was either
    /// in the table or above its top, "never a gap inside it". A reviewer measured the installed
    /// corpus: **31 distinct ids and 113 records land inside the gaps**, concentrated at 95..118 and
    /// 135..141 -- which is exactly where the log shows `keep_array`, `vilg_array` and
    /// `leader_ttype_array` sitting in enumeration order. The gaps are the **per-faith types the
    /// arrays hold**, eight apiece, and they are the commonest objects on a real map.
    ///
    /// So a gap is expected, and an id inside one is a faith-specific object this dump did not
    /// resolve -- not a corrupt record, and not a runtime registration either.
    #[test]
    fn the_table_has_gaps_where_the_per_faith_arrays_sit() {
        let ids: std::collections::BTreeSet<u32> =
            super::TERRAIN_SPRITE_TYPES.iter().map(|(_, id)| *id).collect();
        let highest = *ids.last().unwrap();
        assert_eq!(highest, 237);
        let gaps: Vec<u32> = (0..=highest).filter(|id| !ids.contains(id)).collect();
        assert_eq!(gaps.len(), 60, "the gaps are real and must not be explained away");
        // The two runs of gaps the shipped corpus actually uses.
        for id in [95, 100, 118, 135, 141] {
            assert!(gaps.contains(&id), "{id} should be a gap");
            assert_eq!(super::terrain_sprite_name(id), None);
        }
        // 470 is above the top: that one really is a runtime registration, minted by the probe
        // with `addterrainspritetype` during the run.
        assert!(470 > highest);
        assert_eq!(super::terrain_sprite_name(470), None);
    }

    #[test]
    fn the_land_transition_halo_covers_every_direction_once() {
        let table = super::LAND_TRANSITION_TILES;
        let directions: std::collections::BTreeSet<(i32, i32)> =
            table.iter().map(|entry| entry.direction).collect();
        let expected: std::collections::BTreeSet<(i32, i32)> = (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
            .filter(|(dx, dy)| *dx != 0 || *dy != 0)
            .collect();
        assert_eq!(directions, expected, "all eight neighbours, and no centre");
        // The four edges and the four corners come from separate tile runs, which is what makes
        // this a direction table rather than a single blended value.
        let edges: Vec<u32> = table
            .iter()
            .filter(|e| e.direction.0 == 0 || e.direction.1 == 0)
            .map(|e| e.tile)
            .collect();
        let corners: Vec<u32> = table
            .iter()
            .filter(|e| e.direction.0 != 0 && e.direction.1 != 0)
            .map(|e| e.tile)
            .collect();
        assert_eq!(edges.len(), 4);
        assert_eq!(corners.len(), 4);
        assert!(edges.iter().all(|tile| (1..=4).contains(tile)), "{edges:?}");
        assert!(corners.iter().all(|tile| (16..=19).contains(tile)), "{corners:?}");
        // Every tile distinct: eight directions, eight tiles.
        let tiles: std::collections::BTreeSet<u32> = table.iter().map(|e| e.tile).collect();
        assert_eq!(tiles.len(), 8);
        // And the background itself is a tile of tt_land.
        assert_eq!(base_tile_terrain_type(super::LAND_BACKGROUND_TILE), Some(6));
    }

    #[test]
    fn a_created_map_composes_only_observed_byte_patterns() {
        let map = MapAsset::create(96, 64, 1).unwrap();
        let bytes = map.to_bytes().unwrap();
        // Header word, dimensions, depth -- all values the engine was observed writing.
        assert_eq!(&bytes[0..4], &super::GENERATED_HEADER_WORD.to_le_bytes());
        assert_eq!(&bytes[4..8], &96_u32.to_le_bytes());
        assert_eq!(&bytes[8..12], &64_u32.to_le_bytes());
        assert_eq!(&bytes[12..16], &8_u32.to_le_bytes());
        // The exact eight-byte empty tail the engine saved for a sprite-less map.
        assert_eq!(&bytes[bytes.len() - 8..], &[0, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(bytes.len(), 16 + 96 * 64 * 8 + 8);
        // It parses, and re-encodes to itself.
        let reparsed = MapAsset::parse(&bytes).unwrap();
        assert_eq!(reparsed.to_bytes().unwrap(), bytes);
        assert_eq!(reparsed.cells.iter().filter(|c| c.tile_index() == 392).count(), 96 * 64);
        assert!(reparsed.cells.iter().all(|c| !c.high_flag_set()));
    }

    #[test]
    fn a_created_map_is_editable_and_addressed_the_same_way() {
        let mut map = MapAsset::create(96, 64, 6).unwrap();
        // Non-square, so an index error cannot hide: (95, 63) is valid and (63, 95) is not.
        map.set_tile(95, 63, 392).unwrap();
        assert!(map.set_tile(63, 95, 392).is_err());
        assert_eq!(map.place_sprite(10, 20, 470).unwrap(), 200);
        let written = MapAsset::parse(&map.to_bytes().unwrap()).unwrap();
        let record = &written.placed_sprites_49.as_ref().unwrap().records[0];
        assert_eq!(record.cell_index, 20 * 96 + 10);
        assert_eq!(written.record_coordinates(record), (10, 20));
        assert!(MapAsset::create(0, 64, 1).is_err());
        assert!(MapAsset::create(96, 64, 11).is_err());
    }

    /// The border ring is what the corpus flags, in 146 of 146 files, with zero exceptions. An
    /// interior flag is the shape the engine has never been given, and the probe's whole question.
    #[test]
    fn the_high_flag_can_be_set_on_the_border_and_on_the_interior() {
        let mut map = MapAsset::create(5, 3, 6).unwrap();
        for index in map.border_ring() {
            let (x, y) = (index as u32 % 5, index as u32 / 5);
            map.set_high_flag(x, y, true).unwrap();
        }
        // On a 5x3 map every cell except (1,1), (2,1), (3,1) is on the ring.
        assert_eq!(map.border_ring().len(), 12);
        assert!(!map.cell(2, 1).unwrap().high_flag_set());
        assert!(map.cell(0, 0).unwrap().high_flag_set());
        assert!(map.cell(4, 2).unwrap().high_flag_set());

        map.set_high_flag(2, 1, true).unwrap();
        assert!(map.cell(2, 1).unwrap().high_flag_set());
        assert_eq!(map.cell(2, 1).unwrap().tile_index(), 15, "the tile must survive");
        map.set_high_flag(2, 1, false).unwrap();
        assert!(!map.cell(2, 1).unwrap().high_flag_set());
        assert!(map.set_high_flag(9, 9, true).is_err());
    }

    #[test]
    fn filling_also_preserves_the_unknown_high_bit() {
        let source = non_square_map_with_opaque_tail(3, 2);
        let mut map = MapAsset::parse(&source).unwrap();
        assert!(map.cells.iter().all(|cell| cell.high_flag_set()));
        map.fill_terrain(1).unwrap();
        assert!(
            map.cells.iter().all(|cell| cell.high_flag_set()),
            "a fill must not silently drop a bit whose meaning is Unknown"
        );
        assert!(map.cells.iter().all(|cell| cell.tag == (CELL_TAG_HIGH_FLAG | 392)));
    }

    #[test]
    fn a_tile_index_no_corpus_cell_could_hold_is_rejected() {
        let source = non_square_map_with_record(5, 3, 0);
        let mut map = MapAsset::parse(&source).unwrap();
        // The realistic input: a digit too many.
        assert!(map.set_tile(0, 0, 3920).is_err());
        assert!(map.set_tile(0, 0, 4_000_000_000).is_err());
        assert!(map.set_tile(0, 0, super::TILE_INDEX_LIMIT).is_err());
        // The largest slot the shipped tileset declares still fits.
        map.set_tile(0, 0, 623).unwrap();
        assert_eq!(map.cell(0, 0).unwrap().tile_index(), 623);
    }

    #[test]
    fn filling_writes_the_base_tile_into_every_cell() {
        let source = non_square_map_with_record(5, 3, 0);
        let mut map = MapAsset::parse(&source).unwrap();
        map.fill_terrain(1).unwrap();
        assert!(map.cells.iter().all(|cell| cell.tile_index() == 392));
        assert_eq!(map.cells.len(), 15);
    }

    #[test]
    fn elevation_rejects_values_no_corpus_cell_holds() {
        let source = non_square_map_with_record(5, 3, 0);
        let mut map = MapAsset::parse(&source).unwrap();
        assert!(map.set_elevation(0, 0, f32::NAN).is_err());
        assert!(map.set_elevation(0, 0, f32::INFINITY).is_err());
        map.set_elevation(0, 0, 2.5).unwrap();
        assert_eq!(map.cell(0, 0).unwrap().value_bits, 2.5_f32.to_bits());
    }

    #[test]
    fn placing_and_removing_a_sprite_restores_the_original_bytes() {
        let source = non_square_map_with_record(5, 3, 7);
        let mut map = MapAsset::parse(&source).unwrap();
        let instance = map.place_sprite(2, 2, 470).unwrap();
        assert_ne!(map.to_bytes().unwrap(), source);
        map.remove_sprite(instance).unwrap();
        assert_eq!(
            map.to_bytes().unwrap(),
            source,
            "the engine leaves no residue when a sprite is destroyed; neither may this"
        );
    }

    /// Pins the **real** cross-invocation behaviour, which is that a freed id *is* reissued.
    ///
    /// The high-water mark is not serialized and cannot be, so it dies with the process. The test
    /// below covers the in-parse scope; this one covers what a modder actually gets from a sequence
    /// of CLI calls, by round-tripping through bytes between every edit. The in-memory test alone
    /// passed while the shipped tool reissued ids -- a test that cannot fail on the axis it names.
    #[test]
    fn a_freed_instance_id_comes_back_once_the_map_has_been_written_and_re_read() {
        let mut source = empty_record_map(5, 3);
        let reparse = |bytes: &Vec<u8>| MapAsset::parse(bytes).unwrap();

        let mut map = reparse(&source);
        assert_eq!(map.place_sprite(0, 0, 470).unwrap(), 200);
        source = map.to_bytes().unwrap();

        let mut map = reparse(&source);
        assert_eq!(map.place_sprite(1, 0, 470).unwrap(), 201);
        source = map.to_bytes().unwrap();

        let mut map = reparse(&source);
        map.remove_sprite(201).unwrap();
        source = map.to_bytes().unwrap();

        let mut map = reparse(&source);
        assert_eq!(
            map.place_sprite(2, 0, 470).unwrap(),
            201,
            "the freed id returns across a write, which is the documented limitation"
        );
        let written = reparse(&map.to_bytes().unwrap());
        let section = written.placed_sprites_49.as_ref().unwrap();
        let reissued = section
            .records
            .iter()
            .find(|record| record.instance_id == 201)
            .unwrap();
        assert_eq!(
            written.record_coordinates(reissued),
            (2, 0),
            "and it now names a different cell than the sprite that first held it"
        );
    }

    /// A map with a decoded but empty 49-byte section: `count` 0, then the footer.
    fn empty_record_map(width: u32, height: u32) -> Vec<u8> {
        let mut source = Vec::new();
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&width.to_le_bytes());
        source.extend_from_slice(&height.to_le_bytes());
        source.extend_from_slice(&8_u32.to_le_bytes());
        for _ in 0..width * height {
            source.extend_from_slice(&15_u32.to_le_bytes());
            source.extend_from_slice(&1.0_f32.to_bits().to_le_bytes());
        }
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&1_u32.to_le_bytes());
        source
    }

    #[test]
    fn instance_ids_start_at_200_and_do_not_reuse_a_removed_id_within_one_parse() {
        let source = empty_record_map(5, 3);
        let mut map = MapAsset::parse(&source).unwrap();
        assert_eq!(map.place_sprite(0, 0, 470).unwrap(), 200);
        assert_eq!(map.place_sprite(1, 0, 470).unwrap(), 201);
        map.remove_sprite(201).unwrap();
        assert_eq!(
            map.place_sprite(2, 0, 470).unwrap(),
            202,
            "within one parse the high-water mark holds the freed id back"
        );
    }

    #[test]
    fn a_second_sprite_on_one_cell_is_refused() {
        let source = non_square_map_with_record(5, 3, 7);
        let mut map = MapAsset::parse(&source).unwrap();
        map.place_sprite(2, 2, 470).unwrap();
        assert!(map.place_sprite(2, 2, 471).is_err());
        assert!(map.remove_sprite(9_999).is_err());
    }

    #[test]
    fn a_map_whose_tail_is_not_decoded_refuses_sprite_edits() {
        let source = non_square_map_with_opaque_tail(3, 2);
        let mut map = MapAsset::parse(&source).unwrap();
        assert!(map.place_sprite(0, 0, 470).is_err());
        assert!(map.remove_sprite(200).is_err());
        // A cell edit is still fine, and must still write the tail back untouched.
        map.set_tile(0, 0, 15).unwrap();
        assert_eq!(&map.to_bytes().unwrap()[map.trailing_offset()..], &map.trailing_raw[..]);
    }

    #[test]
    fn an_edit_outside_the_map_is_refused_on_both_axes() {
        let source = non_square_map_with_record(5, 3, 0);
        let mut map = MapAsset::parse(&source).unwrap();
        // 4 and 2 are in range; swapping them is not, which a square fixture could not show.
        map.set_tile(4, 2, 15).unwrap();
        assert!(map.set_tile(2, 4, 15).is_err());
        assert!(map.place_sprite(2, 4, 470).is_err());
        assert!(map.set_elevation(2, 4, 1.0).is_err());
    }

    // --- setterrain-style painting -------------------------------------------------------

    /// A **non-square** map every cell of which holds `tile`, with tag bit `0x00800000` set and a
    /// per-cell elevation, so a paint that clobbered either is visible.
    ///
    /// Non-square because every shipped map is square and that hid a transposed index for months;
    /// 11x3 also makes the N/S rows and the W/E columns different byte distances apart, so a paint
    /// that swapped an axis cannot land on the right bytes by accident.
    fn uniform_map(width: u32, height: u32, tile: u32) -> Vec<u8> {
        let mut source = Vec::new();
        source.extend_from_slice(&0x6c_u32.to_le_bytes());
        source.extend_from_slice(&width.to_le_bytes());
        source.extend_from_slice(&height.to_le_bytes());
        source.extend_from_slice(&8_u32.to_le_bytes());
        for index in 0..width * height {
            source.extend_from_slice(&(tile | CELL_TAG_HIGH_FLAG).to_le_bytes());
            source.extend_from_slice(&(index as f32).to_bits().to_le_bytes());
        }
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&1_u32.to_le_bytes());
        source
    }

    /// The tag word at `16 + (y * width + x) * 8`, read out of the encoded bytes.
    ///
    /// Deliberately not `map.cell(x, y)`: reading back through the accessor the writer used agrees
    /// with a transposed writer just as happily.
    fn tag_at(bytes: &[u8], width: u32, x: u32, y: u32) -> u32 {
        let offset = 16 + ((y * width + x) as usize) * 8;
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn elevation_at(bytes: &[u8], width: u32, x: u32, y: u32) -> f32 {
        let offset = 16 + ((y * width + x) as usize) * 8 + 4;
        f32::from_bits(u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()))
    }

    /// A tileset fixture built to be **unlike** `tilesb01.til` wherever a shape claim is at stake.
    ///
    /// Every shipped map is square, and a transposed cell index survived months because every
    /// fixture was square too. The same trap applies to a tileset, so this one differs from the
    /// real file in each way that could hide a bug:
    ///
    /// - **The atlas is not square**: 12 columns by 3 rows, where `tilesb01.til` is 16 by 39.
    /// - **Interior families are not eight wide.** Grass has three interior tiles and stone has
    ///   one, where every real terrain has eight. Code that assumed `384 + 8k` cannot pass.
    /// - **The eight edge constraints are asymmetric**, and fully specified rather than
    ///   `*`-padded, so a mirrored direction convention picks a different tile instead of the same
    ///   one by symmetry. `n` and `s` are distinct tiles, as are `e`/`w` and each diagonal pair.
    /// - **Tile 0's trailing `index` column is 99**, and tile 12's is 5, so neither equals its
    ///   `tilenum`. Code that read the trailing column as the atlas slot cannot pass.
    /// - Terrain 3 is declared and given no tiles, and the `~` and `|` constraint forms are both
    ///   exercised on the paint path rather than only in a parser test.
    /// - **Terrain 4 is three all-`*` slots and nothing else**, which is `tt_dirt`'s real shape --
    ///   six wildcard slots, no forced tile, and so a background the engine never re-tiles.
    ///
    /// Terrain 1 is grass, 2 is stone. It is not the game's numbering either, deliberately.
    const FIXTURE_TILESET: &[u8] = br#"
LBM=fixture.lbm
TILES= 12, 3
TILESIZE= 8, 8
TERRAINTYPE= 1, 40, "grass",  0, 100, 200, 11, 12, 5, 13, 14
TERRAINTYPE= 2, 41, "stone",  2, 300, 400, 21, 22, 9, 23, 24
TERRAINTYPE= 3, 42, "path",   0, 0, 9999, 0, 0, 1, 0, 0
TERRAINTYPE= 4, 43, "dust",   0, 0, 9999, 0, 0, 1, 0, 0
;         self, n,    ne,  e,    se,  s,    sw,  w,    nw,   index
TILE=  0,    1, 1,    1,   1,    1,   1,    1,   1,    1,    99
TILE=  1,    1, 1,    1,   1,    1,   1,    1,   1,    1,    99
TILE=  2,    1, 1,    1,   1,    1,   1,    1,   1,    1,    99
TILE=  3,    1, 2,    *,   1,    *,   1|3,  *,   1,    *,    3
TILE=  4,    1, 1,    *,   1,    *,   2,    *,   1,    *,    4
TILE=  5,    1, 1,    *,   1,    *,   1,    *,   2,    *,    5
TILE=  6,    1, 1,    *,   2,    *,   1,    *,   1,    *,    6
TILE=  7,    1, 1,    1,   1,    1,   1,    1,   1,    2,    7
TILE=  8,    1, 1,    2,   1,    1,   1,    1,   1,    1,    8
TILE=  9,    1, 1,    1,   1,    1,   1,    2,   1,    1,    9
TILE= 10,    1, 1,    1,   1,    2,   1,    1,   1,    1,    10
TILE= 11,    2, ~1,   ~1,  ~1,   ~1,  ~1,   ~1,  ~1,   ~1,   11
TILE= 12,    2, 1,    1,   1,    1,   1,    1,   1,    1,    5
TILE= 13,    2, 1,    1,   2,    1,   1,    1,   1,    1,    13
TILE= 14,    2, 1,    1,   1,    1,   1,    1,   2,    1,    14
TILE= 15,    1, 2,    2,   1,    1,   1,    2,   2,    2,    15
TILE= 16,    1, 2,    2,   2,    2,   1,    1,   1,    2,    16
TILE= 17,    1, 1,    1,   1,    2,   2,    2,   2,    2,    17
TILE= 18,    1, 1,    2,   2,    2,   2,    2,   1,    1,    18
TILE= 19,    2, 1,    *,   2,    *,   2,    *,   2,    *,    19
TILE= 20,    2, 2,    *,   2,    *,   1,    *,   2,    *,    20
TILE= 21,    2, 2,    *,   2,    *,   2,    *,   1,    *,    21
TILE= 22,    2, 2,    *,   1,    *,   2,    *,   2,    *,    22
TILE= 23,    2, 2,    2,   2,    1,   2,    2,   2,    2,    23
TILE= 24,    2, 2,    2,   2,    2,   2,    1,   2,    2,    24
TILE= 25,    2, 2,    1,   2,    2,   2,    2,   2,    2,    25
TILE= 26,    2, 2,    2,   2,    2,   2,    2,   2,    1,    26
TILE= 30,    4, *,    *,   *,    *,   *,    *,   *,    *,    30
TILE= 31,    4, *,    *,   *,    *,   *,    *,   *,    *,    31
TILE= 32,    4, *,    *,   *,    *,   *,    *,   *,    *,    32
"#;

    fn fixture_tileset() -> TileSetDefinition {
        TileSetDefinition::parse(FIXTURE_TILESET).unwrap()
    }

    /// The whole of the derived rule on the fixture, asserted at computed byte offsets, direction
    /// by direction.
    ///
    /// `N` and `S` take **different** tiles and so do `W` and `E` and each diagonal pair, which is
    /// what makes an axis flip or a sign flip fail here rather than pass by symmetry. This is the
    /// test that pins [`crate::tile::Direction::offset`].
    #[test]
    fn a_painted_cell_re_selects_every_ring_tile_by_its_own_direction() {
        let tile_set = fixture_tileset();
        let (width, height) = (7, 5);
        let source = uniform_map(width, height, 0);
        let mut map = MapAsset::parse(&source).unwrap();

        // One cell of stone in the middle, far enough from every edge to have a full ring whose
        // own neighbours are all on the map.
        let paint = map
            .paint_terrain((3, 2, 3, 2), 2, &tile_set, TileSelector::LowestSlot)
            .unwrap();
        assert_eq!(paint.plan.region.len(), 1);
        assert_eq!(paint.ring_cells_written, 8);
        assert_eq!(paint.cells_changed, 9);
        // Every one of the nine was decided by a single candidate. Nothing here is a guess.
        assert_eq!(paint.plan.ambiguous_cells(), 0);
        assert_eq!(paint.plan.cells_touching_a_map_edge(), 0);

        let bytes = map.to_bytes().unwrap();
        let flag = CELL_TAG_HIGH_FLAG;
        // The region: stone entirely surrounded by grass is tile 12, not stone's interior 11.
        assert_eq!(tag_at(&bytes, width, 3, 2), 12 | flag, "region");
        // The ring. Each cell holds the tile whose own constraint points *back* at the paint: the
        // cell to the north of the paint has stone to its south, so it takes tile 4.
        assert_eq!(tag_at(&bytes, width, 3, 1), 4 | flag, "N of the paint");
        assert_eq!(tag_at(&bytes, width, 3, 3), 3 | flag, "S of the paint");
        assert_eq!(tag_at(&bytes, width, 2, 2), 6 | flag, "W of the paint");
        assert_eq!(tag_at(&bytes, width, 4, 2), 5 | flag, "E of the paint");
        assert_eq!(tag_at(&bytes, width, 2, 1), 10 | flag, "NW of the paint");
        assert_eq!(tag_at(&bytes, width, 4, 1), 9 | flag, "NE of the paint");
        assert_eq!(tag_at(&bytes, width, 2, 3), 8 | flag, "SW of the paint");
        assert_eq!(tag_at(&bytes, width, 4, 3), 7 | flag, "SE of the paint");
        // Two cells out is untouched, so the footprint is exactly the region plus one.
        for x in 0..width {
            assert_eq!(tag_at(&bytes, width, x, 0), tagged(0), "row 0 x={x}");
            assert_eq!(tag_at(&bytes, width, x, 4), tagged(0), "row 4 x={x}");
        }
        for y in 0..height {
            for x in [0, 6] {
                assert_eq!(tag_at(&bytes, width, x, y), tagged(0), "({x}, {y})");
            }
        }
        // Nothing but the tile field moved: elevations and the tail are as they were read.
        for y in 0..height {
            for x in 0..width {
                assert_eq!(elevation_at(&bytes, width, x, y), (y * width + x) as f32);
            }
        }
        assert_eq!(&bytes[map.trailing_offset()..], &source[source.len() - 8..]);
    }

    /// The same paint read through the plan rather than the bytes, including the `~` and `|` forms.
    ///
    /// A 1x2 region: both stone cells have stone on one side and grass on the other, so neither
    /// takes stone's `~1`-constrained interior tile, and the two take *different* tiles.
    #[test]
    fn a_wider_region_re_selects_each_of_its_own_cells_not_one_tile_for_all() {
        let tile_set = fixture_tileset();
        let source = uniform_map(7, 5, 0);
        let map = MapAsset::parse(&source).unwrap();
        let plan = map
            .plan_terrain_paint((2, 2, 3, 2), 2, &tile_set, TileSelector::LowestSlot)
            .unwrap();

        assert_eq!(plan.region.len(), 2);
        // Left cell has stone to its east -> tile 13. Right cell has stone to its west -> 14.
        assert_eq!(plan.region[0].tile_index, 13);
        assert_eq!(plan.region[1].tile_index, 14);
        assert_ne!(plan.region[0].tile_index, plan.region[1].tile_index);
        assert_eq!(plan.ambiguous_cells(), 0);
        // A 1x2 region has ten ring cells, not eight: the north and south edges are two wide.
        assert_eq!(plan.ring.len(), 10);
        let north: Vec<u32> = plan
            .ring
            .iter()
            .filter(|cell| cell.y == 1 && (2..=3).contains(&cell.x))
            .map(|cell| cell.tile_index)
            .collect();
        assert_eq!(north, vec![4, 4], "both cells of the north edge see stone to the south");
        // And planning touched nothing.
        assert_eq!(map.to_bytes().unwrap(), source);
    }

    /// A **newly painted** interior with several equal members is the random draw, and says so.
    ///
    /// Grass has three interior tiles in this fixture, and the stone field the paint lands on holds
    /// none of them, so nothing can be kept and one has to be picked. That is the case the engine
    /// decides at random -- the same 3x3 painted twice gave centre tiles 385 and 390 -- and the only
    /// case this writer calls unreproducible. There is deliberately no test asserting a byte-exact
    /// match against an engine-painted interior, because none is possible.
    #[test]
    fn a_newly_painted_interior_is_a_draw_and_the_selector_decides_it() {
        let tile_set = fixture_tileset();
        // A field of stone with a 3x3 of grass painted into it: the centre cell has grass on all
        // eight sides and so matches grass's whole three-tile interior family.
        let source = uniform_map(9, 9, 11);
        let map = MapAsset::parse(&source).unwrap();

        let lowest = map
            .plan_terrain_paint((3, 3, 5, 5), 1, &tile_set, TileSelector::LowestSlot)
            .unwrap();
        let centre = lowest
            .region
            .iter()
            .find(|cell| (cell.x, cell.y) == (4, 4))
            .unwrap();
        assert_eq!(
            centre.choice,
            TileChoice::Drawn {
                chosen: 0,
                candidates: vec![0, 1, 2]
            }
        );
        assert_eq!(centre.tile_index, 0, "LowestSlot takes the lowest candidate");
        assert!(
            !centre.choice.is_reproducible(),
            "a newly painted interior must not claim to reproduce the engine"
        );
        // Exactly one cell of the nine is ambiguous: the eight perimeter cells are determined.
        assert_eq!(lowest.region.iter().filter(|c| c.choice.is_ambiguous()).count(), 1);

        // A seed must choose from the same candidate set, and some seed must choose differently --
        // otherwise the selector is decorative.
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..64 {
            let plan = map
                .plan_terrain_paint((3, 3, 5, 5), 1, &tile_set, TileSelector::Seeded(seed))
                .unwrap();
            let tile = plan
                .region
                .iter()
                .find(|cell| (cell.x, cell.y) == (4, 4))
                .unwrap()
                .tile_index;
            assert!([0, 1, 2].contains(&tile), "seed {seed} chose {tile}, not a candidate");
            seen.insert(tile);
        }
        assert!(seen.len() > 1, "no seed ever chose differently: {seen:?}");
        // And one seed is one map: the same seed twice is the same tile.
        let first = map
            .plan_terrain_paint((3, 3, 5, 5), 1, &tile_set, TileSelector::Seeded(7))
            .unwrap();
        let again = map
            .plan_terrain_paint((3, 3, 5, 5), 1, &tile_set, TileSelector::Seeded(7))
            .unwrap();
        assert_eq!(first, again);
    }

    /// Painting a terrain onto itself rewrites the region and leaves the ring alone.
    ///
    /// **This is the engine's behaviour, not a shortcut.** On every `painted == background` row of
    /// the `terrainrings` captures the ring kept the background tile while the region was rewritten
    /// from the terrain's interior family -- background 2 cleared to tile 111 came back with a core
    /// of `400, 403, 405, 407` and a ring of pure 111.
    #[test]
    fn painting_a_terrain_onto_itself_rewrites_the_region_and_not_the_ring() {
        let tile_set = fixture_tileset();
        let source = uniform_map(7, 5, 3);
        let mut map = MapAsset::parse(&source).unwrap();
        let paint = map
            .paint_terrain((2, 2, 3, 2), 1, &tile_set, TileSelector::LowestSlot)
            .unwrap();
        assert_eq!(paint.ring_cells_written, 0, "no terrain moved, so no ring");
        assert_eq!(paint.plan.region.len(), 2);
        let bytes = map.to_bytes().unwrap();
        let flag = CELL_TAG_HIGH_FLAG;
        // The region took grass's interior family; every other cell still holds the tile it had.
        for x in 2..=3 {
            assert_eq!(tag_at(&bytes, 7, x, 2), tagged(0), "region x={x}");
        }
        for (x, y) in [(1, 2), (4, 2), (2, 1), (3, 1), (2, 3), (3, 3), (1, 1), (4, 3)] {
            assert_eq!(tag_at(&bytes, 7, x, y), 3 | flag, "ring ({x}, {y}) is untouched");
        }
    }

    /// The ring is clipped to the map, not wrapped round it, and the clipping is **reported**.
    ///
    /// A region in the corner has a ring on two sides only, and on a non-square fixture a wrapped
    /// `W` would land in the row above. The cells with an off-map neighbour also leaned on the
    /// off-map assumption, so the plan must say how many did.
    #[test]
    fn a_region_against_the_edge_has_its_ring_clipped_not_wrapped() {
        let tile_set = fixture_tileset();
        let (width, height) = (5, 3);
        let source = uniform_map(width, height, 0);
        let mut map = MapAsset::parse(&source).unwrap();

        let paint = map
            .paint_terrain((0, 0, 0, 0), 2, &tile_set, TileSelector::LowestSlot)
            .unwrap();
        // Ring cells: E, SE and S. No N, no W, no NW, no NE, no SW.
        assert_eq!(paint.ring_cells_written, 3);
        let positions: Vec<(u32, u32)> =
            paint.plan.ring.iter().map(|cell| (cell.x, cell.y)).collect();
        assert_eq!(positions, vec![(1, 0), (0, 1), (1, 1)]);
        // Three of the four written cells have a neighbour off the map -- the region cell and the
        // two ring cells on the edges -- and the plan says so rather than presenting the decision
        // as fully grounded. The diagonal ring cell at (1, 1) is fully surrounded and does not.
        assert_eq!(paint.plan.cells_touching_a_map_edge(), 3);

        let bytes = map.to_bytes().unwrap();
        // The far end of the first row, where a wrapped W would have landed, is untouched.
        assert_eq!(tag_at(&bytes, width, 4, 0), tagged(0), "no wrap into the row's far end");
        assert_eq!(tag_at(&bytes, width, 3, 1), tagged(0));
        assert_eq!(tag_at(&bytes, width, 2, 0), tagged(0));
    }

    /// What the tileset genuinely cannot decide, and what it now decides that it once refused.
    #[test]
    fn the_cases_outside_the_tileset_refuse_and_write_nothing() {
        let tile_set = fixture_tileset();
        let grass = uniform_map(11, 3, 0);
        let expect_refusal = |source: &[u8], rect, terrain, expected: super::PaintRefusal| {
            let mut map = MapAsset::parse(source).unwrap();
            let before = map.to_bytes().unwrap();
            assert_eq!(
                map.plan_terrain_paint(rect, terrain, &tile_set, TileSelector::LowestSlot)
                    .unwrap_err(),
                expected
            );
            let error = map
                .paint_terrain(rect, terrain, &tile_set, TileSelector::LowestSlot)
                .unwrap_err();
            assert_eq!(error.to_string(), expected.to_string());
            assert_eq!(
                map.to_bytes().unwrap(),
                before,
                "a refused paint must leave the map untouched"
            );
        };

        // A terrain the tileset declares no tiles for. `tilesa01.til` stops at 9 where
        // `tilesb01.til` reaches 10, so this is a real difference between shipped tilesets.
        expect_refusal(
            &grass,
            (4, 1, 6, 1),
            7,
            super::PaintRefusal::TerrainTypeNotInTileSet { terrain_type: 7 },
        );
        // Declared as a TERRAINTYPE but given no tiles: there is a terrain and no way to draw it.
        expect_refusal(
            &grass,
            (4, 1, 6, 1),
            3,
            super::PaintRefusal::TerrainTypeNotInTileSet { terrain_type: 3 },
        );
        // A tile the tileset does not define. `tilesb01.til` leaves seven slots commented out, so
        // a map authored against another tileset lands here.
        let mut foreign = MapAsset::parse(&grass).unwrap();
        foreign.set_tile(3, 0, 28).unwrap();
        let foreign = foreign.to_bytes().unwrap();
        expect_refusal(
            &foreign,
            (4, 1, 6, 1),
            2,
            super::PaintRefusal::BackgroundTileUnrecognised { at: (3, 0), tile_index: 28 },
        );
        // And the input checks, on both axes of a non-square map.
        expect_refusal(
            &grass,
            (4, 1, 4, 4),
            2,
            super::PaintRefusal::OutsideMap { rect: (4, 1, 4, 4), width: 11, height: 3 },
        );
        expect_refusal(
            &grass,
            (6, 1, 4, 1),
            2,
            super::PaintRefusal::NotARectangle { rect: (6, 1, 4, 1) },
        );
    }

    /// A cell for which the tileset declares **no** tile refuses instead of inventing one.
    ///
    /// **Derived from the captures.** This is exactly the road and impassable case: across the 121
    /// `terrainrings` rows there are 512 cells where no constraint of the required terrain is
    /// satisfied, and the engine filled them from the terrain's block regardless. Imitating that
    /// would be invention. The fixture reproduces the shape: stone tile 11 requires every neighbour
    /// to be non-grass and tiles 12..14 require specific grass neighbours, so a 3x1 run of stone
    /// leaves its middle cell -- stone to east and west, grass above and below -- with nothing.
    #[test]
    fn a_cell_with_no_candidate_tile_refuses_rather_than_inventing_one() {
        let tile_set = fixture_tileset();
        let source = uniform_map(9, 5, 0);
        let map = MapAsset::parse(&source).unwrap();
        let before = map.to_bytes().unwrap();

        let refusal = map
            .plan_terrain_paint((3, 2, 5, 2), 2, &tile_set, TileSelector::LowestSlot)
            .unwrap_err();
        let super::PaintRefusal::NoMatchingTile { at, terrain_type, .. } = refusal else {
            panic!("expected NoMatchingTile, got {refusal:?}");
        };
        assert_eq!(at, (4, 2), "the middle cell of the run is the one with no tile");
        assert_eq!(terrain_type, 2);
        // The message names the neighbourhood it could not satisfy, so the refusal is actionable.
        let message = refusal.to_string();
        assert!(message.contains("no tile of terrain 2"), "{message}");
        assert!(message.contains("e=2"), "{message}");
        assert!(message.contains("n=1"), "{message}");
        assert_eq!(map.to_bytes().unwrap(), before);
    }

    /// Painting onto an all-wildcard background leaves the ring **untouched**, which is what
    /// `TERRAIN_TRANSITIONS` says the engine does.
    ///
    /// `tt_dirt` (0) is the real case: its six slots 31, 79, 127, 175, 223 and 271 are `self = 0`
    /// with `*` in all eight columns, so every dirt cell has six candidates and none of them is
    /// forced. A lowest-slot tie-break moved all sixteen ring cells of a 3x3 plains paint from 175
    /// to 31, on a map created by `--map-create 9 9 0`, while
    /// [`TransitionBehaviour::NoTransition`] sat in the same file recording that the engine leaves
    /// them alone -- a saved-artifact measurement nothing consulted.
    ///
    /// Keeping a valid existing tile makes that a no-op without a special case. The fixture's
    /// terrain 4 has three all-wildcard slots for the same reason.
    #[test]
    fn painting_onto_an_all_wildcard_background_leaves_its_ring_alone() {
        let tile_set = fixture_tileset();
        // The behaviour table this reproduces, as a measurement rather than a memory.
        assert_eq!(
            super::transition_behaviour(0),
            Some(super::TransitionBehaviour::NoTransition),
            "tt_dirt was measured as blending nothing; this test is that claim on the fixture",
        );

        let (width, height) = (9, 7);
        // Tile 30 is terrain 4's middle wildcard slot, so a lowest-slot re-selection would move
        // every ring cell to 30's sibling 30... to the lowest, which is 30 itself. Use 31 so the
        // drift would be visible.
        let source = uniform_map(width, height, 31);
        let mut map = MapAsset::parse(&source).unwrap();
        let paint = map
            .paint_terrain((3, 2, 5, 4), 2, &tile_set, TileSelector::LowestSlot)
            .unwrap();

        // The ring is planned -- these cells really are beside changed terrain -- and every one of
        // them keeps what it held.
        assert_eq!(paint.plan.ring.len(), 16);
        for cell in &paint.plan.ring {
            assert_eq!(
                cell.tile_index, 31,
                "({}, {}) must keep the wildcard tile it held, not drift to the lowest",
                cell.x, cell.y
            );
            assert!(matches!(cell.choice, TileChoice::Kept { .. }));
        }
        assert_eq!(paint.ring_cells_written, 16);
        // And nothing outside the painted rectangle moved a byte.
        let bytes = map.to_bytes().unwrap();
        for y in 0..height {
            for x in 0..width {
                if (3..=5).contains(&x) && (2..=4).contains(&y) {
                    continue;
                }
                assert_eq!(tag_at(&bytes, width, x, y), tagged(31), "({x}, {y})");
            }
        }
    }

    /// A **mixed** region re-selects only the ring beside terrain that actually moved.
    ///
    /// Previous tests covered all-changed and none-changed, and a single global "did anything
    /// change" flag passes both. It fails in between: painting plains over a rectangle that was
    /// already half plains re-selected the whole rectangular ring, and a ring cell holding a
    /// non-lowest interior member moved for no terrain reason -- tile 387 became 384 three columns
    /// away from the only cell whose terrain changed.
    #[test]
    fn a_mixed_region_only_rings_the_cells_whose_terrain_moved() {
        let tile_set = fixture_tileset();
        let (width, height) = (11, 7);
        // A field of grass interior tile 2 -- the *highest* of grass's three interior slots, so a
        // lowest-slot re-selection would visibly move it to 0.
        let source = uniform_map(width, height, 2);
        let mut map = MapAsset::parse(&source).unwrap();
        // One stone cell at (8, 3). Everything else is grass.
        map.set_tile(8, 3, 11).unwrap();
        let before = map.to_bytes().unwrap();

        // Paint grass over (4, 3)..(8, 3): only (8, 3) changes terrain.
        let paint = map
            .paint_terrain((4, 3, 8, 3), 1, &tile_set, TileSelector::LowestSlot)
            .unwrap();

        // The ring is the eight cells around (8, 3) that lie outside the rectangle, not the
        // twenty-two of the full rectangle's perimeter.
        let ring: std::collections::BTreeSet<(u32, u32)> =
            paint.plan.ring.iter().map(|cell| (cell.x, cell.y)).collect();
        assert_eq!(
            ring,
            [(7, 2), (8, 2), (9, 2), (9, 3), (7, 4), (8, 4), (9, 4)]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            "only cells beside the one changed cell may be ringed"
        );

        let bytes = map.to_bytes().unwrap();
        // Every grass cell that already held a valid interior keeps tile 2, inside the rectangle
        // and out. Nothing drifts to the lowest slot.
        for y in 0..height {
            for x in 0..width {
                if (x, y) == (8, 3) {
                    continue;
                }
                assert_eq!(
                    tag_at(&bytes, width, x, y),
                    tagged(2),
                    "({x}, {y}) moved for no terrain reason"
                );
            }
        }
        // The one cell whose terrain moved took a grass tile.
        assert_eq!(tile_set.terrain_type_of_tile(tag_at(&bytes, width, 8, 3) & 0x7f_ffff), Some(1));
        assert_ne!(map.to_bytes().unwrap(), before);
        assert_eq!(paint.cells_changed, 1, "exactly one cell had a reason to change");
    }

    /// A whole-map paint of one terrain is a legitimate operation and must be a **no-op** on a map
    /// already holding that terrain's interior tile.
    ///
    /// This is where reading the map edge open goes visibly wrong. An off-map neighbour satisfies
    /// even a negated constraint, so along every edge the one-sided boundary tiles compete with the
    /// interior family and a lowest-slot tie-break takes the most wrong legal option. On
    /// `tilesb01.til` a whole-map water paint wrote a complete phantom coastline -- row 0 all tile
    /// 49, which asserts *land* to the north, row 8 tile 50, column 0 tile 51, column 8 tile 52 --
    /// and a whole-map road paint wrote nine different road tiles for a uniform road field.
    #[test]
    fn a_whole_map_paint_of_the_terrain_already_there_changes_nothing() {
        let tile_set = fixture_tileset();
        let (width, height) = (7, 5);
        let source = uniform_map(width, height, 0);
        let mut map = MapAsset::parse(&source).unwrap();

        let paint = map
            .paint_terrain((0, 0, width - 1, height - 1), 1, &tile_set, TileSelector::LowestSlot)
            .unwrap();
        assert_eq!(paint.cells_changed, 0, "a uniform field is already correct");
        assert_eq!(paint.plan.ring.len(), 0, "nothing moved, so nothing to ring");
        assert_eq!(paint.plan.drawn_cells(), 0, "every cell kept a valid tile");
        assert_eq!(map.to_bytes().unwrap(), source);
        // And the boundary tiles never got a look in, though the open reading would have let them:
        // tile 3 asserts stone to the north and every top-row cell has no north at all.
        let top_left = Neighbourhood::from_lookup(|dx, dy| {
            if dx < 0 || dy < 0 { None } else { Some(1) }
        });
        assert!(tile_set.candidates(1, &top_left).contains(&3), "open reading admits tile 3");
        assert!(
            !tile_set
                .candidates(1, &top_left.closed_with(1))
                .contains(&3),
            "closed reading must exclude a tile that asserts terrain beyond the edge"
        );
    }

    /// The empirical offset table and the tileset's declared constraints must **agree**.
    ///
    /// The 2026-09-17 `terrainrings` run measured 56 ring tiles -- eight directions on each of
    /// seven blending backgrounds -- and they collapsed to one offset table on a per-background
    /// anchor: `N-13 S-14 W-11 E-12 NW+3 NE+4 SW+2 SE+1`. That measurement is now understood as a
    /// *lookup into declared data*: the `.til` file says the same thing, and the offsets are what
    /// you get by subtracting the anchor's slot from the matching tile's slot inside a 48-tile
    /// terrain block.
    ///
    /// **The measurement does not become wrong; it becomes derived.** It is independent
    /// corroboration -- 56 constraints landing on seven numbers is real evidence -- so it is kept,
    /// and this test holds the two against each other so they cannot drift apart.
    ///
    /// The tileset here is synthetic, laid out the way `tilesb01.til` lays out a real terrain
    /// block, at block base 48. That base is deliberate: **48 + 15 = 63 is water's anchor**, the
    /// one that looked like an exception because water's *representative* tile is 392. It is not an
    /// exception at all -- 63 is water's block-index-15 slot exactly like every other terrain's,
    /// and 392 is water's interior slot. The two were never the same kind of thing.
    #[test]
    fn the_measured_offset_table_is_what_the_declared_constraints_produce() {
        const BLOCK: u32 = super::TRANSITION_BLOCK_STRIDE; // 48
        let anchor = BLOCK + super::TRANSITION_ANCHOR_RESIDUE; // 63, water's anchor
        // Background terrain 1, painted terrain 6, and the atlas big enough for slot 392 -- which
        // is where water's interior family really sits. 25x16, so not square.
        let mut source = String::from("LBM=x.lbm\nTILES= 25, 16\nTILESIZE= 32, 32\n");
        source.push_str("TERRAINTYPE= 1, 112, \"water\", 1, 0, 1, 4, 15, 1, 2, 2\n");
        source.push_str("TERRAINTYPE= 6, 125, \"plains\", 0, 0, 9999, 6, 10, 1, 3, 2\n");
        // The anchor: all four cardinals are not the background's own terrain.
        source.push_str(&format!("TILE= {anchor}, 1, ~1, *, ~1, *, ~1, *, ~1, *, 15\n"));
        // The four cardinal-edge slots at block+1..+4 and the four diagonal slots at block+16..+19,
        // each naming the ONE side on which the background stops -- exactly the shape the real
        // file uses. Nothing here mentions an offset; the offsets are the arithmetic that falls out.
        for (slot, columns) in [
            (BLOCK + 1, ["~1", "*", "1", "*", "1", "*", "1", "*"]),
            (BLOCK + 2, ["1", "*", "1", "*", "~1", "*", "1", "*"]),
            (BLOCK + 3, ["1", "*", "1", "*", "1", "*", "~1", "*"]),
            (BLOCK + 4, ["1", "*", "~1", "*", "1", "*", "1", "*"]),
            (BLOCK + 16, ["1", "1", "1", "1", "1", "1", "1", "~1"]),
            (BLOCK + 17, ["1", "~1", "1", "1", "1", "1", "1", "1"]),
            (BLOCK + 18, ["1", "1", "1", "~1", "1", "1", "1", "1"]),
            (BLOCK + 19, ["1", "1", "1", "1", "1", "~1", "1", "1"]),
        ] {
            source.push_str(&format!("TILE= {slot}, 1, {}, 0\n", columns.join(", ")));
        }
        // Water's interior slot, and one plains tile for the painted cell.
        source.push_str("TILE= 392, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0\n");
        source.push_str("TILE= 12, 6, 1, 1, 1, 1, 1, 1, 1, 1, 0\n");
        let tile_set = TileSetDefinition::parse(source.as_bytes()).unwrap();

        // A field of water's interior tile with one plains cell painted into the middle.
        let (width, height) = (9, 7);
        let map_source = uniform_map(width, height, 392);
        let map = MapAsset::parse(&map_source).unwrap();
        let plan = map
            .plan_terrain_paint((4, 3, 4, 3), 6, &tile_set, TileSelector::LowestSlot)
            .unwrap();
        assert_eq!(plan.ring.len(), 8);
        assert_eq!(plan.ambiguous_cells(), 0, "every ring cell must be determined");

        // Every direction of the measured table, checked against the tile the constraints chose.
        for offset in super::TRANSITION_RING_OFFSETS {
            let (dx, dy) = offset.direction;
            let (x, y) = (4 + dx, 3 + dy);
            let cell = plan
                .ring
                .iter()
                .find(|cell| (cell.x as i32, cell.y as i32) == (x, y))
                .unwrap_or_else(|| panic!("no ring cell at ({dx}, {dy})"));
            let expected = u32::try_from(i64::from(anchor) + i64::from(offset.offset)).unwrap();
            assert_eq!(
                cell.tile_index, expected,
                "direction ({dx}, {dy}): the tileset chose {} where the measured offset {} from \
                 anchor {anchor} says {expected}",
                cell.tile_index, offset.offset
            );
        }

        // The measured behaviour table says this background blends, and names the anchor the
        // offsets are relative to. Both accessors are exercised here rather than left as
        // unreferenced public functions, which is how a retained measurement goes stale.
        assert_eq!(
            super::transition_behaviour(1),
            Some(super::TransitionBehaviour::Blends { anchor }),
        );
        assert_eq!(super::transition_anchor(1), Some(anchor));

        // And the measured ring function agrees cell for cell, so the two routes to the same
        // answer are pinned to each other rather than merely both existing.
        let measured = super::transition_ring(1, 6).unwrap();
        for offset in super::TRANSITION_RING_OFFSETS {
            let expected = u32::try_from(i64::from(anchor) + i64::from(offset.offset)).unwrap();
            assert_eq!(
                super::ring_tile(&measured, offset.direction),
                Some(expected),
                "ring_tile must index the measured ring by direction"
            );
        }
        for (index, offset) in super::TRANSITION_RING_OFFSETS.iter().enumerate() {
            let (dx, dy) = offset.direction;
            let cell = plan
                .ring
                .iter()
                .find(|cell| (cell.x as i32, cell.y as i32) == (4 + dx, 3 + dy))
                .unwrap();
            assert_eq!(cell.tile_index, measured[index], "direction ({dx}, {dy})");
        }
    }

    /// The tileset reads a cell's terrain for essentially the whole atlas, which is the thing that
    /// removes the old "cannot read a shipped map's background" refusal.
    #[test]
    fn the_tileset_reads_a_terrain_type_for_every_slot_it_declares() {
        let tile_set = fixture_tileset();
        assert_eq!(tile_set.terrain_type_of_tile(0), Some(1));
        assert_eq!(tile_set.terrain_type_of_tile(10), Some(1));
        assert_eq!(tile_set.terrain_type_of_tile(11), Some(2));
        assert_eq!(tile_set.terrain_type_of_tile(14), Some(2));
        assert_eq!(tile_set.terrain_type_of_tile(18), Some(1));
        assert_eq!(tile_set.terrain_type_of_tile(26), Some(2));
        // A slot inside the atlas that the file does not declare is not guessed at.
        assert_eq!(tile_set.terrain_type_of_tile(27), None);
        assert_eq!(tile_set.terrain_type_of_tile(29), None);
    }
}
