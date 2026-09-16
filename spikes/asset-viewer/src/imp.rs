use std::collections::BTreeMap;
use std::fmt;

const FILE_HEADER_SIZE: usize = 32;
const SEQUENCE_RECORD_SIZE: usize = 16;
const CYCLE_RECORD_SIZE: usize = 8;
const FRAME_RECORD_SIZE: usize = 16;
const HOTSPOT_RECORD_SIZE: usize = 6;
const HOTSPOT_ALIGNMENT: usize = 8;
const PALETTE_COLORS: usize = 256;
const PALETTE_BYTES: usize = PALETTE_COLORS * 4;
const FRAME_FLAG_SHARED_PIXELS: u8 = 0x04;
const FRAME_FLAG_DUPLICATE: u8 = 0x08;
const FILE_FLAG_RLE: u8 = 0x01;
const FILE_FLAG_DEPTH: u8 = 0x30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpFrame {
    pub flags: u8,
    pub width: u16,
    pub height: u16,
    pub origin_x: Option<i16>,
    pub origin_y: Option<i16>,
    pub hotspots: Vec<ImpHotspot>,
    pub palette_indices: Vec<u8>,
    pub rgba: Vec<u8>,
    pub source_frame: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpHotspot {
    pub id: u16,
    pub x: i16,
    pub y: i16,
    pub raw: [u8; HOTSPOT_RECORD_SIZE],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpCycle {
    pub metadata: u16,
    pub first_frame: usize,
    pub frame_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpSequence {
    pub metadata: [u8; 11],
    pub first_cycle: usize,
    pub cycle_count: usize,
    pub first_frame: usize,
    pub frame_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpSprite {
    pub file_flags: u8,
    pub record_variant: u8,
    pub compressed: bool,
    pub bits_per_pixel: u8,
    pub maximum_width: u16,
    pub maximum_height: u16,
    pub sequence_count: usize,
    pub cycle_count: usize,
    pub frame_count: usize,
    pub duplicate_frame_count: usize,
    /// Frames carrying only `FRAME_FLAG_DUPLICATE` (0x08), i.e. true back-references
    /// to an earlier frame index. Excludes `FRAME_FLAG_SHARED_PIXELS` (0x04) frames,
    /// which `duplicate_frame_count` also counts.
    ///
    /// Kept separate for analysis only. Validating this against the generated header's
    /// "Duplicate bitmaps found" statistic instead of `duplicate_frame_count` raises
    /// corpus failures from 10 to 112, so that statistic provably counts both flags.
    pub back_reference_frame_count: usize,
    pub hotspot_count: usize,
    pub hotspot_bytes: u64,
    pub raw_pixel_bytes: u64,
    pub stored_pixel_bytes: u64,
    pub palette: Vec<[u8; 4]>,
    pub sequences: Vec<ImpSequence>,
    pub cycles: Vec<ImpCycle>,
    pub frames: Vec<ImpFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpHeaderStats {
    pub sequence_name: String,
    pub sequence_labels: Vec<Vec<String>>,
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
        let compressed = file_flags & FILE_FLAG_RLE != 0;
        let bits_per_pixel = match file_flags & FILE_FLAG_DEPTH {
            0x00 => 8,
            0x10 => 1,
            0x20 => 2,
            0x30 => 4,
            _ => unreachable!("depth mask covers every possible value"),
        };
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

        let palette: Vec<[u8; 4]> = source[palette_offset..palette_offset + PALETTE_BYTES]
            .chunks_exact(4)
            .map(|bgra| [bgra[2], bgra[1], bgra[0], 255])
            .collect();
        let mut cycle_count = 0_usize;
        let mut frame_count = 0_usize;
        let mut hotspot_count = 0_usize;
        let mut hotspot_bytes = 0_u64;
        let mut duplicate_frame_count = 0_usize;
        let mut back_reference_frame_count = 0_usize;
        let mut raw_pixel_bytes = 0_u64;
        let mut stored_pixel_bytes = 0_u64;
        let mut sequences = Vec::with_capacity(sequence_count);
        let mut cycles = Vec::new();
        let mut frames = Vec::new();
        let mut pixel_sources = BTreeMap::<usize, usize>::new();

        for sequence_index in 0..sequence_count {
            let sequence_offset = sequence_table_offset + sequence_index * SEQUENCE_RECORD_SIZE;
            let sequence_metadata = source[sequence_offset..sequence_offset + 11]
                .try_into()
                .expect("sequence metadata range was checked");
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
            let sequence_first_cycle = cycles.len();
            let sequence_first_frame = frames.len();

            for cycle_index in 0..sequence_cycles {
                let cycle_offset = cycle_table_offset + cycle_index * CYCLE_RECORD_SIZE;
                let cycle_metadata = read_u16(source, cycle_offset)?;
                let cycle_frames = usize::from(read_u16(source, cycle_offset + 2)?);
                let frame_table_offset = read_u32(source, cycle_offset + 4)? as usize;
                require_range(
                    source,
                    frame_table_offset,
                    1,
                    FRAME_RECORD_SIZE,
                    "frame table",
                )?;
                let repeated_cycle = source[frame_table_offset] & FRAME_FLAG_SHARED_PIXELS != 0;
                if !repeated_cycle {
                    require_range(
                        source,
                        frame_table_offset,
                        cycle_frames,
                        FRAME_RECORD_SIZE,
                        "frame table",
                    )?;
                }
                let cycle_first_frame = frames.len();
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
                    let empty_frame = width == 0 && height == 0;
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
                    let hotspots = if frame_hotspots > 0 {
                        let frame_hotspot_bytes = hotspot_bytes_for(frame_hotspots)?;
                        require_range(source, auxiliary, 1, frame_hotspot_bytes, "frame hotspots")?;
                        hotspot_bytes = hotspot_bytes
                            .checked_add(frame_hotspot_bytes as u64)
                            .ok_or_else(|| ImpError::new("IMP hotspot byte count overflow"))?;
                        parse_hotspots(source, auxiliary, frame_hotspots)?
                    } else {
                        Vec::new()
                    };
                    hotspot_count = hotspot_count
                        .checked_add(frame_hotspots)
                        .ok_or_else(|| ImpError::new("IMP hotspot count overflow"))?;
                    let shared_pixels = frame_flags & FRAME_FLAG_SHARED_PIXELS != 0;
                    let duplicate_frame = shared_pixels || frame_flags & FRAME_FLAG_DUPLICATE != 0;
                    if duplicate_frame {
                        let source_frame = if shared_pixels {
                            pixel_sources.get(&pixels_offset).copied().ok_or_else(|| {
                                ImpError::new(format!(
                                    "IMP repeated cycle references unknown pixel offset {pixels_offset}"
                                ))
                            })?
                        } else {
                            if pixels_offset >= frames.len() {
                                return Err(ImpError::new(format!(
                                    "IMP duplicate frame reference {pixels_offset} is out of range"
                                )));
                            }
                            pixels_offset
                        };
                        duplicate_frame_count = duplicate_frame_count
                            .checked_add(1)
                            .ok_or_else(|| ImpError::new("IMP duplicate frame count overflow"))?;
                        if !shared_pixels {
                            back_reference_frame_count = back_reference_frame_count
                                .checked_add(1)
                                .ok_or_else(|| {
                                    ImpError::new("IMP back reference frame count overflow")
                                })?;
                        }
                        frames.push(ImpFrame {
                            flags: frame_flags,
                            width: 0,
                            height: 0,
                            origin_x: None,
                            origin_y: None,
                            hotspots,
                            palette_indices: Vec::new(),
                            rgba: Vec::new(),
                            source_frame: Some(source_frame),
                        });
                        continue;
                    }
                    let pixel_count = usize::from(width)
                        .checked_mul(usize::from(height))
                        .ok_or_else(|| ImpError::new("IMP frame pixel count overflow"))?;
                    let packed_sizes = packed_sizes(width, height, bits_per_pixel)?;
                    let (packed_pixels, consumed) = if empty_frame {
                        (Vec::new(), 0)
                    } else if compressed {
                        if record_variant == 0 {
                            let available = source.get(pixels_offset..).ok_or_else(|| {
                                ImpError::new("IMP frame pixel offset is invalid")
                            })?;
                            decode_rle_until_size(available, &packed_sizes)?
                        } else {
                            require_range(source, pixels_offset, 1, encoded_size, "frame pixels")?;
                            let available = &source[pixels_offset..pixels_offset + encoded_size];
                            (decode_rle_exact(available, &packed_sizes)?, encoded_size)
                        }
                    } else {
                        let packed_size = if record_variant != 0 {
                            if !packed_sizes.contains(&encoded_size) {
                                return Err(ImpError::new(format!(
                                    "IMP raw frame declares unsupported packed size {encoded_size}"
                                )));
                            }
                            encoded_size
                        } else {
                            *packed_sizes
                                .iter()
                                .find(|size| {
                                    **size * 8 >= pixel_count * usize::from(bits_per_pixel)
                                })
                                .ok_or_else(|| ImpError::new("IMP raw frame has no packed size"))?
                        };
                        require_range(source, pixels_offset, 1, packed_size, "frame pixels")?;
                        (
                            source[pixels_offset..pixels_offset + packed_size].to_vec(),
                            packed_size,
                        )
                    };
                    let palette_indices =
                        unpack_pixels(&packed_pixels, width, height, bits_per_pixel)?;
                    let rgba = palette_indices
                        .iter()
                        .flat_map(|index| palette[usize::from(*index)])
                        .collect();
                    let logical_index = frames.len();
                    if !empty_frame {
                        pixel_sources.entry(pixels_offset).or_insert(logical_index);
                    }
                    raw_pixel_bytes = raw_pixel_bytes
                        .checked_add(pixel_count as u64)
                        .ok_or_else(|| ImpError::new("IMP raw pixel size overflow"))?;
                    stored_pixel_bytes = stored_pixel_bytes
                        .checked_add(consumed as u64)
                        .ok_or_else(|| ImpError::new("IMP stored pixel size overflow"))?;
                    frames.push(ImpFrame {
                        flags: frame_flags,
                        width,
                        height,
                        origin_x: (frame_hotspots == 0)
                            .then(|| read_i16(source, frame_offset + 8))
                            .transpose()?,
                        origin_y: (frame_hotspots == 0)
                            .then(|| read_i16(source, frame_offset + 10))
                            .transpose()?,
                        hotspots,
                        palette_indices,
                        rgba,
                        source_frame: None,
                    });
                }
                frame_count = frame_count
                    .checked_add(cycle_frames)
                    .ok_or_else(|| ImpError::new("IMP frame count overflow"))?;
                cycles.push(ImpCycle {
                    metadata: cycle_metadata,
                    first_frame: cycle_first_frame,
                    frame_count: cycle_frames,
                });
            }
            sequences.push(ImpSequence {
                metadata: sequence_metadata,
                first_cycle: sequence_first_cycle,
                cycle_count: sequence_cycles,
                first_frame: sequence_first_frame,
                frame_count: frames.len() - sequence_first_frame,
            });
        }

        Ok(Self {
            file_flags,
            record_variant,
            compressed,
            bits_per_pixel,
            maximum_width,
            maximum_height,
            sequence_count,
            cycle_count,
            frame_count,
            duplicate_frame_count,
            back_reference_frame_count,
            hotspot_count,
            hotspot_bytes,
            raw_pixel_bytes,
            stored_pixel_bytes,
            palette,
            sequences,
            cycles,
            frames,
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
        if let Some(expected) = stats.compressed_pixel_bytes {
            check_equal("stored pixel bytes", self.stored_pixel_bytes, expected)?;
        }
        Ok(())
    }

    pub fn resolved_frame(&self, index: usize) -> Result<&ImpFrame, ImpError> {
        let mut current = index;
        for _ in 0..=self.frames.len() {
            let frame = self.frames.get(current).ok_or_else(|| {
                ImpError::new(format!("IMP frame index {current} is out of range"))
            })?;
            match frame.source_frame {
                Some(source) => current = source,
                None => return Ok(frame),
            }
        }
        Err(ImpError::new(
            "IMP duplicate-frame references contain a cycle",
        ))
    }

    pub fn frame_location(&self, frame_index: usize) -> Result<(usize, usize, usize), ImpError> {
        if frame_index >= self.frames.len() {
            return Err(ImpError::new(format!(
                "IMP frame index {frame_index} is out of range"
            )));
        }
        for (sequence_index, sequence) in self.sequences.iter().enumerate() {
            if !(sequence.first_frame..sequence.first_frame + sequence.frame_count)
                .contains(&frame_index)
            {
                continue;
            }
            for cycle_index in sequence.first_cycle..sequence.first_cycle + sequence.cycle_count {
                let cycle = &self.cycles[cycle_index];
                if (cycle.first_frame..cycle.first_frame + cycle.frame_count).contains(&frame_index)
                {
                    return Ok((sequence_index, cycle_index, frame_index - cycle.first_frame));
                }
            }
        }
        Err(ImpError::new(format!(
            "IMP frame index {frame_index} is not owned by a cycle"
        )))
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

fn parse_hotspots(source: &[u8], offset: usize, count: usize) -> Result<Vec<ImpHotspot>, ImpError> {
    let byte_count = count
        .checked_mul(HOTSPOT_RECORD_SIZE)
        .ok_or_else(|| ImpError::new("IMP hotspot byte count overflow"))?;
    require_range(source, offset, 1, byte_count, "frame hotspots")?;
    Ok(source[offset..offset + byte_count]
        .chunks_exact(HOTSPOT_RECORD_SIZE)
        .map(|bytes| {
            let raw: [u8; HOTSPOT_RECORD_SIZE] =
                bytes.try_into().expect("hotspot chunk size was checked");
            ImpHotspot {
                id: u16::from_le_bytes([raw[0], raw[1]]),
                x: i16::from_le_bytes([raw[2], raw[3]]),
                y: i16::from_le_bytes([raw[4], raw[5]]),
                raw,
            }
        })
        .collect())
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

        let sequence_count = required_stat(text, "Total number of 'Sequences'")? as usize;
        Ok(Self {
            sequence_labels: parse_sequence_labels(text, &sequence_name, sequence_count),
            sequence_name,
            sequence_count,
            frame_count: required_stat(text, "Total number of 'Frames'")? as usize,
            duplicate_frame_count: required_stat(text, "Duplicate bitmaps found")? as usize,
            raw_pixel_bytes: required_stat(text, "Bitmap raw memory usage")?,
            hotspot_bytes: required_stat(text, "Hotspot raw memory usage")?,
            compressed_pixel_bytes,
        })
    }
}

fn parse_sequence_labels(text: &str, root_name: &str, count: usize) -> Vec<Vec<String>> {
    let prefix = format!("{}_", root_name.to_ascii_uppercase());
    let mut labels = vec![Vec::new(); count];
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("#define") {
            continue;
        }
        let Some(symbol) = fields.next() else {
            continue;
        };
        let Some(value) = fields.next().and_then(|value| value.parse::<usize>().ok()) else {
            continue;
        };
        let Some(label) = symbol.strip_prefix(&prefix) else {
            continue;
        };
        if value < labels.len() && !label.is_empty() {
            labels[value].push(label.to_owned());
        }
    }
    labels
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

fn packed_sizes(width: u16, height: u16, bits_per_pixel: u8) -> Result<Vec<usize>, ImpError> {
    let pixel_count = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| ImpError::new("IMP frame pixel count overflow"))?;
    let bits = pixel_count
        .checked_mul(usize::from(bits_per_pixel))
        .ok_or_else(|| ImpError::new("IMP packed pixel size overflow"))?;
    let tight_floor = bits / 8;
    let tight_ceil = bits
        .checked_add(7)
        .map(|padded| padded / 8)
        .ok_or_else(|| ImpError::new("IMP packed pixel size overflow"))?;
    let row_bits = usize::from(width)
        .checked_mul(usize::from(bits_per_pixel))
        .ok_or_else(|| ImpError::new("IMP packed row size overflow"))?;
    let row_bytes = row_bits
        .checked_add(7)
        .map(|padded| padded / 8)
        .ok_or_else(|| ImpError::new("IMP packed row size overflow"))?;
    let row_padded = row_bytes
        .checked_mul(usize::from(height))
        .ok_or_else(|| ImpError::new("IMP packed row storage overflow"))?;
    let mut sizes = if bits_per_pixel == 1 {
        vec![tight_floor, tight_ceil, row_padded]
    } else {
        vec![tight_ceil, row_padded]
    };
    sizes.sort_unstable();
    sizes.dedup();
    Ok(sizes)
}

fn decode_rle_exact(source: &[u8], acceptable_sizes: &[usize]) -> Result<Vec<u8>, ImpError> {
    let mut input = 0_usize;
    let mut output = Vec::new();
    while input < source.len() {
        decode_rle_packet(source, &mut input, &mut output)?;
    }
    if acceptable_sizes.contains(&output.len()) {
        Ok(output)
    } else {
        Err(ImpError::new(format!(
            "IMP RLE expands to unsupported packed size {}",
            output.len()
        )))
    }
}

fn decode_rle_until_size(
    source: &[u8],
    acceptable_sizes: &[usize],
) -> Result<(Vec<u8>, usize), ImpError> {
    let maximum_size = acceptable_sizes.iter().copied().max().unwrap_or(0);
    let mut input = 0_usize;
    let mut output = Vec::with_capacity(maximum_size);
    if acceptable_sizes.contains(&0) {
        return Ok((output, input));
    }
    loop {
        decode_rle_packet(source, &mut input, &mut output)?;
        if acceptable_sizes.contains(&output.len()) {
            return Ok((output, input));
        }
        if output.len() > maximum_size {
            return Err(ImpError::new(format!(
                "IMP RLE expands beyond the largest supported packed size {maximum_size}"
            )));
        }
    }
}

fn decode_rle_packet(
    source: &[u8],
    input: &mut usize,
    output: &mut Vec<u8>,
) -> Result<(), ImpError> {
    let control = *source
        .get(*input)
        .ok_or_else(|| ImpError::new("IMP RLE control byte is truncated"))?;
    *input += 1;
    if control < 0x80 {
        let count = usize::from(control) + 3;
        let value = *source
            .get(*input)
            .ok_or_else(|| ImpError::new("IMP RLE repeated value is truncated"))?;
        *input += 1;
        output.extend(std::iter::repeat_n(value, count));
    } else {
        let count = 0x100_usize - usize::from(control);
        let end = input
            .checked_add(count)
            .ok_or_else(|| ImpError::new("IMP RLE literal offset overflow"))?;
        let literal = source
            .get(*input..end)
            .ok_or_else(|| ImpError::new("IMP RLE literal is truncated"))?;
        output.extend_from_slice(literal);
        *input = end;
    }
    Ok(())
}

fn unpack_pixels(
    packed: &[u8],
    width: u16,
    height: u16,
    bits_per_pixel: u8,
) -> Result<Vec<u8>, ImpError> {
    let acceptable_sizes = packed_sizes(width, height, bits_per_pixel)?;
    if !acceptable_sizes.contains(&packed.len()) {
        return Err(ImpError::new(format!(
            "IMP packed pixels have unsupported size {}",
            packed.len()
        )));
    }
    let pixel_count = usize::from(width) * usize::from(height);
    if bits_per_pixel == 8 {
        return Ok(packed.to_vec());
    }

    let pixels_per_byte = 8 / usize::from(bits_per_pixel);
    let mask = (1_u8 << bits_per_pixel) - 1;
    let mut pixels = Vec::with_capacity(pixel_count);
    let row_bytes = (usize::from(width) * usize::from(bits_per_pixel)).div_ceil(8);
    let tight_size = (pixel_count * usize::from(bits_per_pixel)).div_ceil(8);
    let row_padded_size = row_bytes * usize::from(height);
    if packed.len() == row_padded_size && row_padded_size > tight_size {
        for row in packed.chunks_exact(row_bytes) {
            let row_start = pixels.len();
            for byte in row {
                unpack_byte(*byte, bits_per_pixel, mask, pixels_per_byte, &mut pixels);
            }
            pixels.truncate(row_start + usize::from(width));
        }
    } else {
        for byte in packed {
            unpack_byte(*byte, bits_per_pixel, mask, pixels_per_byte, &mut pixels);
            if pixels.len() >= pixel_count {
                break;
            }
        }
    }
    pixels.truncate(pixel_count);
    pixels.resize(pixel_count, 0);
    Ok(pixels)
}

fn unpack_byte(
    byte: u8,
    bits_per_pixel: u8,
    mask: u8,
    pixels_per_byte: usize,
    pixels: &mut Vec<u8>,
) {
    for subpixel in 0..pixels_per_byte {
        let shift = 8 - usize::from(bits_per_pixel) * (subpixel + 1);
        pixels.push((byte >> shift) & mask);
    }
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

fn read_i16(source: &[u8], offset: usize) -> Result<i16, ImpError> {
    read_u16(source, offset).map(|value| i16::from_le_bytes(value.to_le_bytes()))
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
        source[32..43].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
        source[32 + 11] = 1;
        source[32 + 12..32 + 16].copy_from_slice(&48_u32.to_le_bytes());
        source[48..50].copy_from_slice(&7_u16.to_le_bytes());
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
        assert_eq!(
            sprite.sequences[0].metadata,
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        );
        assert_eq!(sprite.sequences[0].first_cycle, 0);
        assert_eq!(sprite.sequences[0].frame_count, 1);
        assert_eq!(sprite.cycles[0].metadata, 7);
        assert_eq!(sprite.cycles[0].first_frame, 0);
        assert_eq!(sprite.cycles[0].frame_count, 1);
        assert_eq!(sprite.frame_location(0).unwrap(), (0, 0, 0));
        assert_eq!(sprite.raw_pixel_bytes, 2);
        assert_eq!(sprite.stored_pixel_bytes, 2);
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
        source.splice(72..72, [0_u8; FRAME_RECORD_SIZE]);
        source[8..12].copy_from_slice(&88_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[56 + 12..56 + 16].copy_from_slice(&1112_u32.to_le_bytes());
        source[72] = FRAME_FLAG_DUPLICATE;

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frame_count, 2);
        assert_eq!(sprite.duplicate_frame_count, 1);
        assert_eq!(sprite.raw_pixel_bytes, 2);
        assert_eq!(sprite.stored_pixel_bytes, 2);
        assert_eq!(
            sprite.resolved_frame(1).unwrap().palette_indices,
            [0xaa, 0xbb]
        );
    }

    #[test]
    fn shared_pixel_records_resolve_by_payload_offset_inside_a_cycle() {
        let mut source = synthetic_imp();
        source.splice(72..72, [0_u8; FRAME_RECORD_SIZE]);
        source[8..12].copy_from_slice(&88_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[56 + 12..56 + 16].copy_from_slice(&1112_u32.to_le_bytes());
        source[72] = FRAME_FLAG_SHARED_PIXELS;
        source[72 + 12..72 + 16].copy_from_slice(&1112_u32.to_le_bytes());

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frame_count, 2);
        assert_eq!(sprite.duplicate_frame_count, 1);
        assert_eq!(sprite.frames[1].source_frame, Some(0));
        assert_eq!(
            sprite.resolved_frame(1).unwrap().palette_indices,
            [0xaa, 0xbb]
        );
    }

    #[test]
    fn repeated_cycle_record_represents_each_logical_frame() {
        let mut source = synthetic_imp();
        source.splice(56..56, [0_u8; CYCLE_RECORD_SIZE + FRAME_RECORD_SIZE]);
        source[8..12].copy_from_slice(&96_u32.to_le_bytes());
        source[32 + 11] = 2;
        source[48 + 4..48 + 8].copy_from_slice(&80_u32.to_le_bytes());
        source[56 + 2..56 + 4].copy_from_slice(&5_u16.to_le_bytes());
        source[56 + 4..56 + 8].copy_from_slice(&64_u32.to_le_bytes());
        source[64] = FRAME_FLAG_SHARED_PIXELS;
        source[64 + 12..64 + 16].copy_from_slice(&1120_u32.to_le_bytes());
        source[80 + 12..80 + 16].copy_from_slice(&1120_u32.to_le_bytes());

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frame_count, 6);
        assert_eq!(sprite.duplicate_frame_count, 5);
        assert_eq!(sprite.sequences[0].cycle_count, 2);
        assert_eq!(sprite.sequences[0].frame_count, 6);
        assert_eq!(sprite.cycles[1].first_frame, 1);
        assert_eq!(sprite.cycles[1].frame_count, 5);
        assert_eq!(sprite.frame_location(5).unwrap(), (0, 1, 4));
        assert_eq!(sprite.raw_pixel_bytes, 2);
        assert_eq!(sprite.stored_pixel_bytes, 2);
        assert_eq!(
            sprite.resolved_frame(5).unwrap().palette_indices,
            [0xaa, 0xbb]
        );
    }

    #[test]
    fn decodes_literal_and_repeated_rle_packets() {
        let source = [0x00, 0xaa, 0xfe, 0xbb, 0xcc];
        let decoded = decode_rle_exact(&source, &[5]).unwrap();
        assert_eq!(decoded, [0xaa, 0xaa, 0xaa, 0xbb, 0xcc]);
    }

    #[test]
    fn expands_packed_pixels_most_significant_bits_first() {
        assert_eq!(
            unpack_pixels(&[0b0001_1011], 4, 1, 2).unwrap(),
            [0, 1, 2, 3]
        );
        assert_eq!(unpack_pixels(&[0b1010_0000], 3, 1, 1).unwrap(), [1, 0, 1]);
        assert_eq!(unpack_pixels(&[0xab], 2, 1, 4).unwrap(), [0x0a, 0x0b]);
        assert_eq!(
            unpack_pixels(&[0b1010_1010], 11, 1, 1).unwrap(),
            [1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0]
        );
    }

    #[test]
    fn decodes_hotspot_id_and_signed_offsets_without_discarding_raw_bytes() {
        let source = [0x07, 0x00, 0xfc, 0xff, 0xe0, 0xff];
        let hotspots = parse_hotspots(&source, 0, 1).unwrap();

        assert_eq!(hotspots[0].id, 7);
        assert_eq!(hotspots[0].x, -4);
        assert_eq!(hotspots[0].y, -32);
        assert_eq!(hotspots[0].raw, source);
    }

    #[test]
    fn parses_generated_header_statistics() {
        let header = b"// Sprite headers for sequence dragon\r\n\
//Cycle-name defines\r\n\
#define DRAGON_MOVE 0\r\n\
#define DRAGON_STAND 1\r\n\
#define DRAGON_IDLE 1\r\n\
// Total number of 'Sequences': 2\r\n\
// Total number of 'Frames': 45\r\n\
// Duplicate bitmaps found : 3\r\n\
// Bitmap raw memory usage : 144878\r\n\
// Hotspot raw memory usage : 16\r\n\
// Bitmap RLE memory usage : 60875\r\n";
        let stats = ImpHeaderStats::parse(header).unwrap();
        assert_eq!(stats.sequence_name, "dragon");
        assert_eq!(
            stats.sequence_labels,
            [
                vec!["MOVE".to_owned()],
                vec!["STAND".to_owned(), "IDLE".to_owned()]
            ]
        );
        assert_eq!(stats.sequence_count, 2);
        assert_eq!(stats.frame_count, 45);
        assert_eq!(stats.duplicate_frame_count, 3);
        assert_eq!(stats.raw_pixel_bytes, 144_878);
        assert_eq!(stats.hotspot_bytes, 16);
        assert_eq!(stats.compressed_pixel_bytes, Some(60_875));
    }
}
