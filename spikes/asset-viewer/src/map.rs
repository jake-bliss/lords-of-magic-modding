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
    Ok(Some(PlacedSpriteSection49 { records, footer }))
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
}
