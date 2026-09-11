use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PbmImage {
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    pub palette: Vec<[u8; 3]>,
    pub palette_entries: usize,
    pub compression: u8,
    pub masking: u8,
    pub transparent_color: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PbmError(String);

impl PbmError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for PbmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PbmError {}

impl PbmImage {
    pub fn decode(source: &[u8]) -> Result<Self, PbmError> {
        if source.len() < 12 || &source[0..4] != b"FORM" || &source[8..12] != b"PBM " {
            return Err(PbmError::new("not an IFF FORM PBM image"));
        }

        let form_size = read_u32(source, 4)? as usize;
        let form_end = 8_usize
            .checked_add(form_size)
            .ok_or_else(|| PbmError::new("FORM size overflow"))?;
        if form_end > source.len() {
            return Err(PbmError::new("truncated FORM"));
        }

        let mut header = None;
        let mut palette = None;
        let mut body = None;
        let mut cursor = 12;
        while cursor + 8 <= form_end {
            let chunk_id = &source[cursor..cursor + 4];
            let chunk_size = read_u32(source, cursor + 4)? as usize;
            let chunk_start = cursor + 8;
            let chunk_end = chunk_start
                .checked_add(chunk_size)
                .ok_or_else(|| PbmError::new("chunk size overflow"))?;
            if chunk_end > form_end {
                return Err(PbmError::new("truncated IFF chunk"));
            }
            match chunk_id {
                b"BMHD" => header = Some(&source[chunk_start..chunk_end]),
                b"CMAP" => palette = Some(&source[chunk_start..chunk_end]),
                b"BODY" => body = Some(&source[chunk_start..chunk_end]),
                _ => {}
            }
            cursor = chunk_end + (chunk_size & 1);
        }

        let header = header.ok_or_else(|| PbmError::new("PBM has no BMHD chunk"))?;
        if header.len() < 20 {
            return Err(PbmError::new("PBM BMHD chunk is too short"));
        }
        let width = read_u16(header, 0)?;
        let height = read_u16(header, 2)?;
        let masking = header[9];
        let compression = header[10];
        let transparent_color = read_u16(header, 12)? as u8;
        if width == 0 || height == 0 {
            return Err(PbmError::new("PBM dimensions must be nonzero"));
        }

        let palette = palette.ok_or_else(|| PbmError::new("PBM has no CMAP chunk"))?;
        if palette.len() < 3 {
            return Err(PbmError::new("PBM palette is empty"));
        }
        let palette_entries = palette.len() / 3;
        let palette_colors: Vec<[u8; 3]> = palette
            .chunks_exact(3)
            .map(|color| [color[0], color[1], color[2]])
            .collect();
        let body = body.ok_or_else(|| PbmError::new("PBM has no BODY chunk"))?;
        let row_bytes = (usize::from(width) + 1) & !1;
        let decoded_size = row_bytes
            .checked_mul(usize::from(height))
            .ok_or_else(|| PbmError::new("PBM dimensions overflow"))?;
        let indices = match compression {
            0 => {
                if body.len() < decoded_size {
                    return Err(PbmError::new("uncompressed PBM BODY is truncated"));
                }
                let mut packed = Vec::with_capacity(usize::from(width) * usize::from(height));
                for row in body[..decoded_size].chunks_exact(row_bytes) {
                    packed.extend_from_slice(&row[..usize::from(width)]);
                }
                packed
            }
            1 => decode_byte_run1_rows(body, usize::from(width), usize::from(height))?,
            value => {
                return Err(PbmError::new(format!(
                    "unsupported PBM compression {value}"
                )));
            }
        };

        let pixel_count = usize::from(width) * usize::from(height);
        let mut rgba = Vec::with_capacity(pixel_count * 4);
        for y in 0..usize::from(height) {
            for x in 0..usize::from(width) {
                let index = indices[y * usize::from(width) + x];
                let palette_offset = usize::from(index) * 3;
                if palette_offset + 2 >= palette.len() {
                    return Err(PbmError::new(format!(
                        "palette index {index} exceeds {palette_entries} entries"
                    )));
                }
                rgba.extend_from_slice(&[
                    palette[palette_offset],
                    palette[palette_offset + 1],
                    palette[palette_offset + 2],
                    if masking == 2 && index == transparent_color {
                        0
                    } else {
                        255
                    },
                ]);
            }
        }

        Ok(Self {
            width,
            height,
            rgba,
            palette: palette_colors,
            palette_entries,
            compression,
            masking,
            transparent_color,
        })
    }
}

fn decode_byte_run1_rows(source: &[u8], width: usize, height: usize) -> Result<Vec<u8>, PbmError> {
    let expected_size = width
        .checked_mul(height)
        .ok_or_else(|| PbmError::new("PBM dimensions overflow"))?;
    let mut output = Vec::with_capacity(expected_size);
    let mut cursor = 0;
    for _ in 0..height {
        let row_start = output.len();
        while output.len() - row_start < width {
            let control = *source
                .get(cursor)
                .ok_or_else(|| PbmError::new("truncated ByteRun1 control byte"))?
                as i8;
            cursor += 1;
            let remaining = width - (output.len() - row_start);
            match control {
                0..=127 => {
                    let encoded_count = control as usize + 1;
                    let end = cursor
                        .checked_add(encoded_count)
                        .ok_or_else(|| PbmError::new("ByteRun1 literal size overflow"))?;
                    let literal = source
                        .get(cursor..end)
                        .ok_or_else(|| PbmError::new("truncated ByteRun1 literal"))?;
                    output.extend_from_slice(&literal[..encoded_count.min(remaining)]);
                    cursor = end;
                }
                -127..=-1 => {
                    let value = *source
                        .get(cursor)
                        .ok_or_else(|| PbmError::new("truncated ByteRun1 repeat"))?;
                    cursor += 1;
                    let encoded_count = 1_usize + usize::from(control.unsigned_abs());
                    output.resize(output.len() + encoded_count.min(remaining), value);
                }
                -128 => {}
            }
        }
    }
    Ok(output)
}

fn read_u16(source: &[u8], offset: usize) -> Result<u16, PbmError> {
    let bytes: [u8; 2] = source
        .get(offset..offset + 2)
        .ok_or_else(|| PbmError::new("truncated big-endian u16"))?
        .try_into()
        .expect("slice length was checked");
    Ok(u16::from_be_bytes(bytes))
}

fn read_u32(source: &[u8], offset: usize) -> Result<u32, PbmError> {
    let bytes: [u8; 4] = source
        .get(offset..offset + 4)
        .ok_or_else(|| PbmError::new("truncated big-endian u32"))?
        .try_into()
        .expect("slice length was checked");
    Ok(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pbm(compression: u8, body: &[u8]) -> Vec<u8> {
        let mut chunks = Vec::new();
        chunks.extend_from_slice(b"BMHD");
        chunks.extend_from_slice(&20_u32.to_be_bytes());
        chunks.extend_from_slice(&2_u16.to_be_bytes());
        chunks.extend_from_slice(&1_u16.to_be_bytes());
        chunks.extend_from_slice(&[0, 0, 0, 0, 8, 0, compression, 0]);
        chunks.extend_from_slice(&0_u16.to_be_bytes());
        chunks.extend_from_slice(&[1, 1]);
        chunks.extend_from_slice(&2_u16.to_be_bytes());
        chunks.extend_from_slice(&1_u16.to_be_bytes());
        chunks.extend_from_slice(b"CMAP");
        chunks.extend_from_slice(&6_u32.to_be_bytes());
        chunks.extend_from_slice(&[10, 20, 30, 40, 50, 60]);
        chunks.extend_from_slice(b"BODY");
        chunks.extend_from_slice(&(body.len() as u32).to_be_bytes());
        chunks.extend_from_slice(body);
        if body.len() % 2 == 1 {
            chunks.push(0);
        }

        let mut source = Vec::new();
        source.extend_from_slice(b"FORM");
        source.extend_from_slice(&((chunks.len() + 4) as u32).to_be_bytes());
        source.extend_from_slice(b"PBM ");
        source.extend_from_slice(&chunks);
        source
    }

    #[test]
    fn decodes_uncompressed_pbm() {
        let image = PbmImage::decode(&pbm(0, &[0, 1])).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.rgba, [10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn decodes_byte_run1_pbm() {
        let image = PbmImage::decode(&pbm(1, &[1, 0, 1])).unwrap();
        assert_eq!(image.rgba, [10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn clamps_byte_run1_packets_at_scanline_boundaries() {
        let image = PbmImage::decode(&pbm(1, &[3, 0, 1, 1, 0])).unwrap();
        assert_eq!(image.rgba, [10, 20, 30, 255, 40, 50, 60, 255]);
    }
}
