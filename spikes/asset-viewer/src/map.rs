use std::fmt;

const HEADER_SIZE: usize = 16;
const CELL_SIZE: usize = 8;
const PLACED_SPRITE_RECORD_49_SIZE: usize = 49;
const PLACED_SPRITE_SECTION_49_FIXED_BYTES: usize = 8;

pub const CELL_TAG_FORCED_TEXTURE: u32 = 0x0080_0000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapCell {
    pub tag: u32,
    pub value_bits: u32,
    pub value: f32,
}

impl MapCell {
    pub fn tile_index_candidate(&self) -> u32 {
        self.tag & !CELL_TAG_FORCED_TEXTURE
    }

    pub fn forced_texture_candidate(&self) -> bool {
        self.tag & CELL_TAG_FORCED_TEXTURE != 0
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
    pub instance_id: u32,
    pub attribute_bits: u32,
    pub sprite_type_candidate: u32,
    pub marker_32: u16,
    pub procedure_id_candidate: i32,
    pub unknown_38: u32,
    pub unknown_42: u32,
    pub unknown_46: [u8; 3],
}

impl PlacedSpriteRecord49 {
    pub fn attribute_code_candidate(&self) -> u8 {
        (self.attribute_bits >> 28) as u8
    }

    pub fn coordinates(&self, map_height: u32) -> (u32, u32) {
        (self.cell_index / map_height, self.cell_index % map_height)
    }
}

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

    pub fn cell_index(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        usize::try_from(x)
            .ok()?
            .checked_mul(usize::try_from(self.height).ok()?)?
            .checked_add(usize::try_from(y).ok()?)
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
            sprite_type_candidate: read_u32(source, offset + 28)?,
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
    use super::{CELL_TAG_FORCED_TEXTURE, MapAsset};

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
    fn decodes_forced_texture_tag_without_losing_raw_value() {
        let cell = super::MapCell {
            tag: CELL_TAG_FORCED_TEXTURE | 619,
            value_bits: 0,
            value: 0.0,
        };

        assert_eq!(cell.tile_index_candidate(), 619);
        assert!(cell.forced_texture_candidate());
        assert_eq!(cell.tag, 0x0080_026b);
    }

    #[test]
    fn parses_49_byte_placed_sprite_records_and_x_major_coordinates() {
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
        assert_eq!(record.coordinates(map.height), (2, 1));
        assert_eq!(record.instance_id, 207);
        assert_eq!(record.attribute_code_candidate(), 10);
        assert_eq!(record.sprite_type_candidate, 105);
        assert_eq!(record.procedure_id_candidate, 12);
        assert_eq!(record.raw.len(), 49);
        assert_eq!(section.footer, 3);
        assert_eq!(map.cell(2, 1), Some(&map.cells[5]));
        assert!(map.cell(3, 0).is_none());
    }
}
