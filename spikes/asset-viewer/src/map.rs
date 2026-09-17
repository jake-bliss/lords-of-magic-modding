use std::fmt;

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
    pub trailing_offset: usize,
    pub trailing_bytes: usize,
    pub trailing_head_u32: Option<u32>,
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
        let trailing_head_u32 = (trailing_bytes >= 4)
            .then(|| read_u32(source, trailing_offset))
            .transpose()?;
        let placed_sprites_49 =
            parse_placed_sprites_49(source, trailing_offset, trailing_bytes, cell_count)?;
        let trailing_raw = source[trailing_offset..].to_vec();

        Ok(Self {
            metadata,
            width,
            height,
            bits_per_pixel,
            cells,
            trailing_offset,
            trailing_bytes,
            trailing_head_u32,
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
            .trailing_head_u32
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
                    .filter(|expected| *expected == self.trailing_bytes)
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
        match &self.placed_sprites_49 {
            Some(section) => bytes.extend_from_slice(&section.to_bytes()?),
            None => bytes.extend_from_slice(&self.trailing_raw),
        }
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
    /// Bit `0x00800000` of the existing tag is **preserved**. The 2026-09-17 probe showed
    /// `forcetexture` never *sets* the bit -- zero of 4,096 forced cells -- but no probe cell had
    /// it set beforehand, so whether the engine clears it is unmeasured. Its meaning is Unknown,
    /// and preserving an unknown bit is the conservative half of an unmeasured choice.
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
        assert_eq!(map.trailing_offset, 32);
        assert_eq!(map.trailing_bytes, 6);
        assert_eq!(map.trailing_head_u32, Some(3));
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

        assert_eq!(map.trailing_bytes, 8);
        assert!(section.records.is_empty());
        assert_eq!(section.footer, 1);

        let populated = non_square_map_with_record(5, 3, 7);
        let map = MapAsset::parse(&populated).unwrap();
        let section = map.placed_sprites_49.as_ref().unwrap();

        assert_eq!(map.trailing_bytes, 4 + 49 + 4);
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
        assert_eq!(&map.to_bytes().unwrap()[map.trailing_offset..], &map.trailing_raw[..]);
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
}
