use std::fmt;

const FILE_HEADER_SIZE: usize = 32;
const SEQUENCE_RECORD_SIZE: usize = 16;
const CYCLE_RECORD_SIZE: usize = 8;
const FRAME_RECORD_SIZE: usize = 16;
const HOTSPOT_RECORD_SIZE: usize = 6;
const HOTSPOT_ALIGNMENT: usize = 8;
const PALETTE_COLORS: usize = 256;
const PALETTE_BYTES: usize = PALETTE_COLORS * 4;
const FRAME_FLAG_REPEATED_CYCLE: u8 = 0x04;
const FRAME_FLAG_DUPLICATE: u8 = 0x08;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpSprite {
    pub file_flags: u8,
    pub record_variant: u8,
    pub maximum_width: u16,
    pub maximum_height: u16,
    pub sequence_count: usize,
    pub cycle_count: usize,
    pub frame_count: usize,
    pub duplicate_frame_count: usize,
    pub hotspot_count: usize,
    pub hotspot_bytes: u64,
    pub raw_pixel_bytes: u64,
    pub stored_pixel_bytes: Option<u64>,
    pub palette: Vec<[u8; 4]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpHeaderStats {
    pub sequence_name: String,
    pub sequence_count: usize,
    pub frame_count: usize,
    pub duplicate_frame_count: usize,
    pub raw_pixel_bytes: u64,
    pub hotspot_bytes: u64,
    pub compressed_pixel_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpError(String);

impl ImpError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ImpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ImpError {}

impl ImpSprite {
    pub fn parse(source: &[u8]) -> Result<Self, ImpError> {
        if source.len() < FILE_HEADER_SIZE {
            return Err(ImpError::new("IMP file header is truncated"));
        }

        let file_flags = source[0];
        let record_variant = source[2];
        let maximum_width = read_u16(source, 4)?;
        let maximum_height = read_u16(source, 6)?;
        let palette_offset = read_u32(source, 8)? as usize;
        let sequence_count = usize::from(read_u16(source, 26)?);
        let sequence_table_offset = read_u32(source, 28)? as usize;
        if maximum_width == 0 || maximum_height == 0 {
            return Err(ImpError::new("IMP maximum dimensions must be nonzero"));
        }
        if sequence_count == 0 {
            return Err(ImpError::new("IMP has no animation sequences"));
        }
        require_range(
            source,
            sequence_table_offset,
            sequence_count,
            SEQUENCE_RECORD_SIZE,
            "sequence table",
        )?;
        require_range(source, palette_offset, 1, PALETTE_BYTES, "palette")?;

        let palette = source[palette_offset..palette_offset + PALETTE_BYTES]
            .chunks_exact(4)
            .map(|bgra| [bgra[2], bgra[1], bgra[0], 255])
            .collect();
        let mut cycle_count = 0_usize;
        let mut frame_count = 0_usize;
        let mut hotspot_count = 0_usize;
        let mut hotspot_bytes = 0_u64;
        let mut duplicate_frame_count = 0_usize;
        let mut raw_pixel_bytes = 0_u64;
        let mut stored_pixel_bytes = 0_u64;

        for sequence_index in 0..sequence_count {
            let sequence_offset = sequence_table_offset + sequence_index * SEQUENCE_RECORD_SIZE;
            let sequence_cycles = usize::from(source[sequence_offset + 11]);
            let cycle_table_offset = read_u32(source, sequence_offset + 12)? as usize;
            if sequence_cycles == 0 {
                return Err(ImpError::new(format!(
                    "IMP sequence {sequence_index} has no cycles"
                )));
            }
            require_range(
                source,
                cycle_table_offset,
                sequence_cycles,
                CYCLE_RECORD_SIZE,
                "cycle table",
            )?;
            cycle_count = cycle_count
                .checked_add(sequence_cycles)
                .ok_or_else(|| ImpError::new("IMP cycle count overflow"))?;

            for cycle_index in 0..sequence_cycles {
                let cycle_offset = cycle_table_offset + cycle_index * CYCLE_RECORD_SIZE;
                let cycle_frames = usize::from(read_u16(source, cycle_offset + 2)?);
                let frame_table_offset = read_u32(source, cycle_offset + 4)? as usize;
                require_range(
                    source,
                    frame_table_offset,
                    1,
                    FRAME_RECORD_SIZE,
                    "frame table",
                )?;
                let repeated_cycle = source[frame_table_offset] & FRAME_FLAG_REPEATED_CYCLE != 0;
                if !repeated_cycle {
                    require_range(
                        source,
                        frame_table_offset,
                        cycle_frames,
                        FRAME_RECORD_SIZE,
                        "frame table",
                    )?;
                }
                frame_count = frame_count
                    .checked_add(cycle_frames)
                    .ok_or_else(|| ImpError::new("IMP frame count overflow"))?;

                for frame_index in 0..cycle_frames {
                    let frame_offset = if repeated_cycle {
                        frame_table_offset
                    } else {
                        frame_table_offset + frame_index * FRAME_RECORD_SIZE
                    };
                    let frame_hotspots = usize::from(source[frame_offset + 1]);
                    let frame_flags = source[frame_offset];
                    let width = read_u16(source, frame_offset + 2)?;
                    let height = read_u16(source, frame_offset + 4)?;
                    let encoded_size = usize::from(read_u16(source, frame_offset + 6)?);
                    let auxiliary = read_u32(source, frame_offset + 8)? as usize;
                    let pixels_offset = read_u32(source, frame_offset + 12)? as usize;
                    let empty_frame = width == 0 && height == 0 && encoded_size == 0;
                    if !empty_frame && (width == 0 || height == 0) {
                        return Err(ImpError::new(format!(
                            "IMP frame {frame_index} in cycle {cycle_index} has partial zero dimensions"
                        )));
                    }
                    if width > maximum_width || height > maximum_height {
                        return Err(ImpError::new(format!(
                            "IMP frame {frame_index} in cycle {cycle_index} exceeds maximum dimensions"
                        )));
                    }
                    let duplicate_frame = repeated_cycle || frame_flags & FRAME_FLAG_DUPLICATE != 0;
                    if duplicate_frame {
                        if !repeated_cycle && pixels_offset >= frame_count {
                            return Err(ImpError::new(format!(
                                "IMP duplicate frame reference {pixels_offset} is out of range"
                            )));
                        }
                        duplicate_frame_count = duplicate_frame_count
                            .checked_add(1)
                            .ok_or_else(|| ImpError::new("IMP duplicate frame count overflow"))?;
                    } else if !empty_frame {
                        require_range(source, pixels_offset, 1, encoded_size, "frame pixels")?;
                    }
                    if frame_hotspots > 0 {
                        let frame_hotspot_bytes = hotspot_bytes_for(frame_hotspots)?;
                        require_range(source, auxiliary, 1, frame_hotspot_bytes, "frame hotspots")?;
                        hotspot_bytes = hotspot_bytes
                            .checked_add(frame_hotspot_bytes as u64)
                            .ok_or_else(|| ImpError::new("IMP hotspot byte count overflow"))?;
                    }
                    hotspot_count = hotspot_count
                        .checked_add(frame_hotspots)
                        .ok_or_else(|| ImpError::new("IMP hotspot count overflow"))?;
                    if !duplicate_frame {
                        raw_pixel_bytes = raw_pixel_bytes
                            .checked_add(u64::from(width) * u64::from(height))
                            .ok_or_else(|| ImpError::new("IMP raw pixel size overflow"))?;
                        stored_pixel_bytes = stored_pixel_bytes
                            .checked_add(encoded_size as u64)
                            .ok_or_else(|| ImpError::new("IMP stored pixel size overflow"))?;
                    }
                }
            }
        }

        Ok(Self {
            file_flags,
            record_variant,
            maximum_width,
            maximum_height,
            sequence_count,
            cycle_count,
            frame_count,
            duplicate_frame_count,
            hotspot_count,
            hotspot_bytes,
            raw_pixel_bytes,
            stored_pixel_bytes: (record_variant != 0).then_some(stored_pixel_bytes),
            palette,
        })
    }

    pub fn validate_against(&self, stats: &ImpHeaderStats) -> Result<(), ImpError> {
        check_equal("sequence count", self.sequence_count, stats.sequence_count)?;
        check_equal("frame count", self.frame_count, stats.frame_count)?;
        check_equal(
            "duplicate frame count",
            self.duplicate_frame_count,
            stats.duplicate_frame_count,
        )?;
        check_equal(
            "raw pixel bytes",
            self.raw_pixel_bytes,
            stats.raw_pixel_bytes,
        )?;
        check_equal("hotspot bytes", self.hotspot_bytes, stats.hotspot_bytes)?;
        if let Some(stored) = self.stored_pixel_bytes {
            let expected = stats
                .compressed_pixel_bytes
                .unwrap_or(stats.raw_pixel_bytes);
            check_equal("stored pixel bytes", stored, expected)?;
        }
        Ok(())
    }
}

fn check_equal<T>(label: &str, actual: T, expected: T) -> Result<(), ImpError>
where
    T: fmt::Display + PartialEq,
{
    if actual == expected {
        Ok(())
    } else {
        Err(ImpError::new(format!(
            "IMP {label} mismatch: binary={actual}, header={expected}"
        )))
    }
}

impl ImpHeaderStats {
    pub fn parse(source: &[u8]) -> Result<Self, ImpError> {
        let text = std::str::from_utf8(source)
            .map_err(|_| ImpError::new("IMP generated header is not UTF-8/ASCII"))?;
        let sequence_name = text
            .lines()
            .find_map(|line| line.strip_prefix("// Sprite headers for sequence "))
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| ImpError::new("IMP generated header has no sequence name"))?
            .to_owned();
        let compressed_pixel_bytes = stat_value(text, "Bitmap RLE memory usage")?;
        let no_compression = text
            .lines()
            .any(|line| line.trim() == "// No compression scheme used");
        if compressed_pixel_bytes.is_none() && !no_compression {
            return Err(ImpError::new(
                "IMP generated header has no compression statistic",
            ));
        }

        Ok(Self {
            sequence_name,
            sequence_count: required_stat(text, "Total number of 'Sequences'")? as usize,
            frame_count: required_stat(text, "Total number of 'Frames'")? as usize,
            duplicate_frame_count: required_stat(text, "Duplicate bitmaps found")? as usize,
            raw_pixel_bytes: required_stat(text, "Bitmap raw memory usage")?,
            hotspot_bytes: required_stat(text, "Hotspot raw memory usage")?,
            compressed_pixel_bytes,
        })
    }
}

fn required_stat(text: &str, label: &str) -> Result<u64, ImpError> {
    stat_value(text, label)?
        .ok_or_else(|| ImpError::new(format!("IMP generated header has no {label} statistic")))
}

fn stat_value(text: &str, label: &str) -> Result<Option<u64>, ImpError> {
    let Some(value) = text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("//")?
            .trim()
            .strip_prefix(label)?
            .trim()
            .strip_prefix(':')
            .map(str::trim)
    }) else {
        return Ok(None);
    };
    value
        .parse()
        .map(Some)
        .map_err(|_| ImpError::new(format!("IMP {label} statistic is not an integer")))
}

fn hotspot_bytes_for(count: usize) -> Result<usize, ImpError> {
    count
        .checked_mul(HOTSPOT_RECORD_SIZE)
        .and_then(|size| size.checked_add(HOTSPOT_ALIGNMENT - 1))
        .map(|size| size & !(HOTSPOT_ALIGNMENT - 1))
        .ok_or_else(|| ImpError::new("IMP hotspot size overflow"))
}

fn require_range(
    source: &[u8],
    offset: usize,
    count: usize,
    item_size: usize,
    label: &str,
) -> Result<(), ImpError> {
    let size = count
        .checked_mul(item_size)
        .ok_or_else(|| ImpError::new(format!("IMP {label} size overflow")))?;
    let end = offset
        .checked_add(size)
        .ok_or_else(|| ImpError::new(format!("IMP {label} offset overflow")))?;
    if end > source.len() {
        return Err(ImpError::new(format!("IMP {label} is truncated")));
    }
    Ok(())
}

fn read_u16(source: &[u8], offset: usize) -> Result<u16, ImpError> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| ImpError::new("IMP u16 offset overflow"))?;
    let bytes: [u8; 2] = source
        .get(offset..end)
        .ok_or_else(|| ImpError::new("IMP u16 is truncated"))?
        .try_into()
        .expect("slice length was checked");
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(source: &[u8], offset: usize) -> Result<u32, ImpError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| ImpError::new("IMP u32 offset overflow"))?;
    let bytes: [u8; 4] = source
        .get(offset..end)
        .ok_or_else(|| ImpError::new("IMP u32 is truncated"))?
        .try_into()
        .expect("slice length was checked");
    Ok(u32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_imp() -> Vec<u8> {
        let mut source = vec![0_u8; 32 + 16 + 8 + 16];
        source[2] = 1;
        source[4..6].copy_from_slice(&2_u16.to_le_bytes());
        source[6..8].copy_from_slice(&1_u16.to_le_bytes());
        let palette_offset = source.len();
        source[8..12].copy_from_slice(&(palette_offset as u32).to_le_bytes());
        source[26..28].copy_from_slice(&1_u16.to_le_bytes());
        source[28..32].copy_from_slice(&32_u32.to_le_bytes());
        source[32 + 11] = 1;
        source[32 + 12..32 + 16].copy_from_slice(&48_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&1_u16.to_le_bytes());
        source[48 + 4..48 + 8].copy_from_slice(&56_u32.to_le_bytes());
        source[56 + 2..56 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[56 + 4..56 + 6].copy_from_slice(&1_u16.to_le_bytes());
        source[56 + 6..56 + 8].copy_from_slice(&2_u16.to_le_bytes());
        let pixel_offset = palette_offset + PALETTE_BYTES;
        source[56 + 12..56 + 16].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        source.resize(pixel_offset, 0);
        source[palette_offset..palette_offset + 4].copy_from_slice(&[3, 2, 1, 0]);
        source.extend_from_slice(&[0xaa, 0xbb]);
        source
    }

    #[test]
    fn parses_tables_palette_and_frame_totals() {
        let sprite = ImpSprite::parse(&synthetic_imp()).unwrap();
        assert_eq!((sprite.maximum_width, sprite.maximum_height), (2, 1));
        assert_eq!(sprite.sequence_count, 1);
        assert_eq!(sprite.cycle_count, 1);
        assert_eq!(sprite.frame_count, 1);
        assert_eq!(sprite.raw_pixel_bytes, 2);
        assert_eq!(sprite.stored_pixel_bytes, Some(2));
        assert_eq!(sprite.palette[0], [1, 2, 3, 255]);
    }

    #[test]
    fn rejects_out_of_bounds_frame_data() {
        let mut source = synthetic_imp();
        source[56 + 12..56 + 16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            ImpSprite::parse(&source).unwrap_err().to_string(),
            "IMP frame pixels is truncated"
        );
    }

    #[test]
    fn duplicate_frames_reference_existing_frame_indices_without_pixel_payloads() {
        let mut source = synthetic_imp();
        source[56] = FRAME_FLAG_DUPLICATE;
        source[56 + 12..56 + 16].copy_from_slice(&0_u32.to_le_bytes());
        source.truncate(source.len() - 2);

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frame_count, 1);
        assert_eq!(sprite.duplicate_frame_count, 1);
        assert_eq!(sprite.raw_pixel_bytes, 0);
        assert_eq!(sprite.stored_pixel_bytes, Some(0));
    }

    #[test]
    fn repeated_cycle_record_represents_each_logical_frame() {
        let mut source = synthetic_imp();
        source[48 + 2..48 + 4].copy_from_slice(&5_u16.to_le_bytes());
        source[56] = FRAME_FLAG_REPEATED_CYCLE;
        source[56 + 2..56 + 8].fill(0);
        source.truncate(source.len() - 2);

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frame_count, 5);
        assert_eq!(sprite.duplicate_frame_count, 5);
        assert_eq!(sprite.raw_pixel_bytes, 0);
        assert_eq!(sprite.stored_pixel_bytes, Some(0));
    }

    #[test]
    fn parses_generated_header_statistics() {
        let header = b"// Sprite headers for sequence dragon\r\n\
// Total number of 'Sequences': 2\r\n\
// Total number of 'Frames': 45\r\n\
// Duplicate bitmaps found : 3\r\n\
// Bitmap raw memory usage : 144878\r\n\
// Hotspot raw memory usage : 16\r\n\
// Bitmap RLE memory usage : 60875\r\n";
        let stats = ImpHeaderStats::parse(header).unwrap();
        assert_eq!(stats.sequence_name, "dragon");
        assert_eq!(stats.sequence_count, 2);
        assert_eq!(stats.frame_count, 45);
        assert_eq!(stats.duplicate_frame_count, 3);
        assert_eq!(stats.raw_pixel_bytes, 144_878);
        assert_eq!(stats.hotspot_bytes, 16);
        assert_eq!(stats.compressed_pixel_bytes, Some(60_875));
    }
}
