use std::fmt;

const HEADER_SIZE: usize = 16;
const CELL_SIZE: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapCell {
    pub tag: u32,
    pub value_bits: u32,
    pub value: f32,
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

        Ok(Self {
            metadata,
            width,
            height,
            bits_per_pixel,
            cells,
            trailing_offset,
            trailing_bytes,
            trailing_head_u32,
        })
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
    use super::MapAsset;

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
}
