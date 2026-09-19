use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PbmImage {
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    /// Raw palette indices, row-major, one per pixel. Kept alongside `rgba` so
    /// callers that need a lossless re-encode (e.g. indexed PNG export) do not
    /// have to reverse-map colours back onto the palette.
    pub indices: Vec<u8>,
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
            indices,
            palette: palette_colors,
            palette_entries,
            compression,
            masking,
            transparent_color,
        })
    }
}

/// One IFF chunk, kept in the order it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PbmChunk {
    pub id: [u8; 4],
    pub data: Vec<u8>,
}

/// A whole `FORM PBM ` file: the decoded image plus every chunk it was built
/// from, verbatim and in order.
///
/// The decoder models `BMHD`, `CMAP` and `BODY` and ignores the rest, but a
/// *writer* that only emitted what it models would silently drop `CRNG` colour
/// cycling, `DPPS`, and the `TINY` thumbnail the game may read. Re-encoding
/// therefore rewrites `BODY` in place and copies every other chunk untouched --
/// except `TINY`, which is derived from `BODY` and is dropped rather than
/// preserved stale when the pixels change. See
/// [`PbmFile::encode_with_indices`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PbmFile {
    pub image: PbmImage,
    pub chunks: Vec<PbmChunk>,
}

impl PbmFile {
    pub fn parse(source: &[u8]) -> Result<Self, PbmError> {
        let image = PbmImage::decode(source)?;
        let chunks = split_chunks(source)?;
        Ok(Self { image, chunks })
    }

    /// Re-encodes the file with its own pixels.
    pub fn encode(&self) -> Result<Vec<u8>, PbmError> {
        let indices = self.image.indices.clone();
        self.encode_with_indices(&indices)
    }

    /// Re-encodes the file with new palette indices of the same dimensions.
    ///
    /// The palette, masking and every unmodelled chunk come from the file this
    /// was parsed from; only the pixels and the `BMHD` compression byte change.
    ///
    /// **`TINY` is the one exception, because it is not independent metadata.**
    /// It is a thumbnail *derived* from `BODY`: the three 640x480 members
    /// checked all carry an 80x60 `TINY`, an exact 8x downscale, and 917 of the
    /// 1,045 shipped PBMs carry one. Copying it verbatim onto rewritten pixels
    /// would ship a file whose thumbnail is the old artwork. Regenerating it
    /// would need a downscaler this project has not written, so a pixel-changing
    /// re-encode drops the chunk instead; that is format-valid on the corpus's
    /// own evidence, since 128 of the 1,045 carry no `TINY` at all
    /// (`fonts\balloon2.lbm` is `BMHD`, `CMAP`, `BODY` and nothing else).
    /// `CRNG` and `DPPS` do not describe pixels and are always preserved.
    pub fn encode_with_indices(&self, indices: &[u8]) -> Result<Vec<u8>, PbmError> {
        let width = usize::from(self.image.width);
        let height = usize::from(self.image.height);
        let expected = width
            .checked_mul(height)
            .ok_or_else(|| PbmError::new("PBM dimensions overflow"))?;
        if indices.len() != expected {
            return Err(PbmError::new(format!(
                "expected {expected} palette indices for {width}x{height}; got {}",
                indices.len()
            )));
        }
        if let Some(index) = indices
            .iter()
            .find(|index| usize::from(**index) >= self.image.palette_entries)
        {
            return Err(PbmError::new(format!(
                "palette index {index} exceeds {} entries",
                self.image.palette_entries
            )));
        }
        let body = encode_byte_run1_rows(indices, width, height)?;
        // Same pixels in means the thumbnail still describes them; the
        // round-trip path (`encode`) therefore keeps `TINY` untouched.
        let pixels_changed = indices != self.image.indices.as_slice();

        let mut chunks = Vec::with_capacity(self.chunks.len());
        let mut wrote_body = false;
        for chunk in &self.chunks {
            match &chunk.id {
                b"BODY" => {
                    chunks.push(PbmChunk {
                        id: *b"BODY",
                        data: body.clone(),
                    });
                    wrote_body = true;
                }
                b"BMHD" => {
                    if chunk.data.len() < 20 {
                        return Err(PbmError::new("PBM BMHD chunk is too short"));
                    }
                    let mut header = chunk.data.clone();
                    // The encoder only emits ByteRun1, so an uncompressed source
                    // must not keep advertising compression 0.
                    header[10] = 1;
                    chunks.push(PbmChunk {
                        id: *b"BMHD",
                        data: header,
                    });
                }
                // A stale thumbnail is worse than no thumbnail: dropping it is
                // format-valid, copying it is a lie about the artwork.
                b"TINY" if pixels_changed => {}
                _ => chunks.push(chunk.clone()),
            }
        }
        if !wrote_body {
            return Err(PbmError::new("PBM has no BODY chunk"));
        }

        Ok(write_form(&chunks))
    }
}

/// Splits a `FORM PBM ` file into its chunks without interpreting any of them.
fn split_chunks(source: &[u8]) -> Result<Vec<PbmChunk>, PbmError> {
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

    let mut chunks = Vec::new();
    let mut cursor = 12;
    while cursor + 8 <= form_end {
        let chunk_size = read_u32(source, cursor + 4)? as usize;
        let chunk_start = cursor + 8;
        let chunk_end = chunk_start
            .checked_add(chunk_size)
            .ok_or_else(|| PbmError::new("chunk size overflow"))?;
        if chunk_end > form_end {
            return Err(PbmError::new("truncated IFF chunk"));
        }
        let id: [u8; 4] = source[cursor..cursor + 4]
            .try_into()
            .expect("slice length was checked");
        chunks.push(PbmChunk {
            id,
            data: source[chunk_start..chunk_end].to_vec(),
        });
        cursor = chunk_end + (chunk_size & 1);
    }
    Ok(chunks)
}

/// Assembles chunks into a `FORM PBM ` file, padding every odd-sized chunk.
fn write_form(chunks: &[PbmChunk]) -> Vec<u8> {
    let mut body = Vec::new();
    for chunk in chunks {
        body.extend_from_slice(&chunk.id);
        body.extend_from_slice(&(chunk.data.len() as u32).to_be_bytes());
        body.extend_from_slice(&chunk.data);
        if chunk.data.len() % 2 == 1 {
            body.push(0);
        }
    }

    let mut output = Vec::with_capacity(body.len() + 12);
    output.extend_from_slice(b"FORM");
    output.extend_from_slice(&((body.len() + 4) as u32).to_be_bytes());
    output.extend_from_slice(b"PBM ");
    output.extend_from_slice(&body);
    output
}

/// The pad byte an odd-width `FORM PBM ` scanline carries so every row occupies
/// an even number of bytes.
///
/// **Observed** in the vanilla `pic.mpq`: `LBM\building\LLBRKS1a.lbm` is 143
/// pixels wide and every one of its 119 rows decodes to exactly 144 bytes, with
/// the 144th equal to `0x00` in all 119 -- never a copy of the row's last pixel,
/// which is what a packet merely overrunning the scanline would have produced.
const ROW_PAD_BYTE: u8 = 0;

/// The longest literal packet ByteRun1 can express: control `127` means "the
/// next 128 bytes are literal".
const MAX_LITERAL_RUN: usize = 128;

/// The longest repeat packet ByteRun1 can express: control `129` (`-127`) means
/// `257 - 129 = 128` copies of the next byte.
const MAX_REPEAT_RUN: usize = 128;

/// The shortest run worth a repeat packet when a literal is already open.
///
/// A repeat always costs two bytes. Two identical pixels appended to an open
/// literal cost two bytes as well, so only a run of three or more actually
/// saves anything; breaking on two would cost the extra literal header.
const MIN_REPEAT_RUN: usize = 3;

/// Encodes 8-bit palette indices as an IFF ByteRun1 `BODY`.
///
/// Rows are encoded independently and no packet ever spans a scanline, which is
/// the property the decoder's per-row clamp depends on. An odd-width row is
/// padded to an even byte length with [`ROW_PAD_BYTE`] before packing, matching
/// what the shipped images do.
pub fn encode_byte_run1_rows(
    indices: &[u8],
    width: usize,
    height: usize,
) -> Result<Vec<u8>, PbmError> {
    if width == 0 || height == 0 {
        return Err(PbmError::new("PBM dimensions must be nonzero"));
    }
    let expected = width
        .checked_mul(height)
        .ok_or_else(|| PbmError::new("PBM dimensions overflow"))?;
    if indices.len() != expected {
        return Err(PbmError::new(format!(
            "expected {expected} palette indices for {width}x{height}; got {}",
            indices.len()
        )));
    }

    let padded_width = (width + 1) & !1;
    let mut output = Vec::with_capacity(expected);
    let mut row = Vec::with_capacity(padded_width);
    for scanline in indices.chunks_exact(width) {
        row.clear();
        row.extend_from_slice(scanline);
        row.resize(padded_width, ROW_PAD_BYTE);
        encode_byte_run1_row(&row, width, &mut output);
    }
    Ok(output)
}

/// Packs one scanline, already padded to `row.len()` from `content_len` pixels.
///
/// **A padded row must never put its pad byte in a packet of its own.** This
/// decoder (Observed) stops a row as soon as it holds `content_len` pixels, so a
/// lone trailing pad packet is never consumed and is read as the *next* row's
/// first packet, shifting every pixel after it. What the *engine's* decoder does
/// is Inferred, not measured: the shipped images are evidence about the original
/// packer's choices, and they are equally consistent with a clamping decoder and
/// with one that fills the padded width. The rule below is safe under both --
/// verified against an independent strict decoder -- which is why it is applied
/// without settling that question.
///
/// A packet is therefore never allowed to end exactly at `content_len` when a
/// pad byte follows: it gives up a byte so the pad travels with a real pixel.
/// It never *swallows* the pad instead, and cannot. A packet that ends at
/// `content_len` while a pad byte remains is by construction a maximal one:
/// [`run_length`] only stops short of [`MAX_REPEAT_RUN`] when the next byte
/// differs, so a repeat that could swallow the pad would have included it
/// already; and the literal loop only stops before the row's end when a repeat
/// starts there or the packet is full, and a single trailing pad byte can never
/// start a repeat. So in both arms the packet is at its maximum and shortening
/// is the only move available.
fn encode_byte_run1_row(row: &[u8], content_len: usize, output: &mut Vec<u8>) {
    let padded = row.len() > content_len;
    let mut cursor = 0;
    while cursor < row.len() {
        let run = run_length(row, cursor);
        if run >= MIN_REPEAT_RUN {
            let mut take = run;
            if padded && cursor + take == content_len {
                take -= 1;
            }
            // `257 - control` copies, so a run of `n` is control `257 - n`.
            output.push((257 - take) as u8);
            output.push(row[cursor]);
            cursor += take;
            continue;
        }
        // Literal: keep taking bytes until a run worth a packet of its own
        // starts, or until the packet is full.
        let start = cursor;
        while cursor < row.len()
            && cursor - start < MAX_LITERAL_RUN
            && run_length(row, cursor) < MIN_REPEAT_RUN
        {
            cursor += 1;
        }
        if padded && cursor == content_len {
            cursor -= 1;
        }
        let literal = &row[start..cursor];
        output.push((literal.len() - 1) as u8);
        output.extend_from_slice(literal);
    }
}

/// How many identical bytes start at `offset`, capped at one repeat packet.
fn run_length(row: &[u8], offset: usize) -> usize {
    let value = row[offset];
    let mut length = 1;
    while length < MAX_REPEAT_RUN && offset + length < row.len() && row[offset + length] == value {
        length += 1;
    }
    length
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

    /// Builds a `FORM PBM ` from chunks given in order, so a test can decide
    /// exactly which chunks a file carries and where.
    fn form(chunks: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        let chunks: Vec<PbmChunk> = chunks
            .iter()
            .map(|(id, data)| PbmChunk {
                id: *id,
                data: data.clone(),
            })
            .collect();
        write_form(&chunks)
    }

    fn bmhd(width: u16, height: u16, compression: u8) -> Vec<u8> {
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[0, 0, 0, 0, 8, 0, compression, 0]);
        header.extend_from_slice(&0_u16.to_be_bytes());
        header.extend_from_slice(&[1, 1]);
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header
    }

    /// 256 distinct-ish entries so any index is legal.
    fn cmap() -> Vec<u8> {
        (0..256_u16)
            .flat_map(|index| [index as u8, (index as u8) ^ 0x5a, (index as u8) ^ 0xa5])
            .collect()
    }

    #[test]
    fn encodes_a_uniform_row_as_one_repeat_packet() {
        let body = encode_byte_run1_rows(&[7; 10], 10, 1).unwrap();
        // 257 - 247 = 10 copies of 7.
        assert_eq!(body, [247, 7]);
        assert_eq!(decode_byte_run1_rows(&body, 10, 1).unwrap(), [7; 10]);
    }

    #[test]
    fn encodes_a_row_with_no_repeats_as_one_literal_packet() {
        let row: Vec<u8> = (0..10).collect();
        let body = encode_byte_run1_rows(&row, 10, 1).unwrap();
        assert_eq!(body[0], 9, "control 9 means the next 10 bytes are literal");
        assert_eq!(&body[1..], row.as_slice());
        assert_eq!(decode_byte_run1_rows(&body, 10, 1).unwrap(), row);
    }

    /// Pins the repeat threshold, which nothing else did -- a mutation to 2
    /// survived the rest of this module.
    ///
    /// A pair costs two bytes either way, so folding it into an open literal
    /// avoids a second packet header. **The shipped images do the opposite**:
    /// counting packets in `File00001070.xxx`, `fonts\belwe10.lbm` and
    /// `LBM\building\LLBRKS1a.lbm` finds 1,975, 214 and 185 repeat packets of
    /// count 2 respectively, so their packer breaks on two. `fonts\balloon2.lbm`
    /// has none and caps its runs at 127 rather than 128, so the corpus does not
    /// even hold one convention. Ours is chosen on cost, not copied, and that is
    /// why this encoder is not byte-identical to theirs.
    #[test]
    fn folds_a_run_of_two_into_the_neighbouring_literal() {
        let body = encode_byte_run1_rows(&[1, 2, 5, 5, 9, 8], 6, 1).unwrap();

        assert_eq!(body, [5, 1, 2, 5, 5, 9, 8]);
    }

    #[test]
    fn encodes_a_run_of_exactly_128_as_a_single_packet() {
        let body = encode_byte_run1_rows(&[3; 128], 128, 1).unwrap();
        // 128 is the largest count ByteRun1 can express: 257 - 129 = 128.
        assert_eq!(body, [129, 3]);
        assert_eq!(decode_byte_run1_rows(&body, 128, 1).unwrap(), [3; 128]);
    }

    #[test]
    fn splits_a_run_of_129_because_128_is_the_packet_maximum() {
        let mut row = vec![3_u8; 129];
        row.push(9);
        let body = encode_byte_run1_rows(&row, 130, 1).unwrap();
        // 128 threes as a repeat, then the 129th three and the 9 as a literal
        // pair -- a repeat of 1 does not exist and a repeat of 2 would cost the
        // same two bytes the literal already spends.
        assert_eq!(body, [129, 3, 1, 3, 9]);
        assert_eq!(decode_byte_run1_rows(&body, 130, 1).unwrap(), row);
    }

    #[test]
    fn caps_a_literal_packet_at_128_bytes() {
        let row: Vec<u8> = (0..200).map(|index| index as u8).collect();
        let body = encode_byte_run1_rows(&row, 200, 1).unwrap();
        assert_eq!(
            body[0], 127,
            "the first literal must be the 128-byte maximum"
        );
        assert_eq!(&body[1..129], &row[..128]);
        assert_eq!(body[129], 71, "72 bytes are left over");
        assert_eq!(&body[130..], &row[128..]);
        assert_eq!(decode_byte_run1_rows(&body, 200, 1).unwrap(), row);
    }

    #[test]
    fn never_lets_a_packet_cross_a_scanline() {
        // Twelve identical pixels, but as two rows of six. A packer that ignored
        // scanlines would emit one count-12 packet (control 245) and the decoder
        // would then read the second row out of the first row's packet.
        let body = encode_byte_run1_rows(&[4; 12], 6, 2).unwrap();
        assert_eq!(body, [251, 4, 251, 4]);
        assert_eq!(decode_byte_run1_rows(&body, 6, 2).unwrap(), [4; 12]);
    }

    #[test]
    fn pads_an_odd_width_row_with_a_zero_byte() {
        let body = encode_byte_run1_rows(&[1, 2, 3], 3, 1).unwrap();
        // The row is packed as four bytes, the fourth being the IFF pad.
        assert_eq!(body, [3, 1, 2, 3, 0]);
        assert_eq!(decode_byte_run1_rows(&body, 3, 1).unwrap(), [1, 2, 3]);
    }

    /// Regression, found by the mixed-image round trip: an odd-width row whose
    /// final run stops exactly at the last pixel used to leave the pad byte in a
    /// packet of its own. The decoder fills the row and never reads that packet,
    /// so the *next* row started one packet late and came out shifted.
    #[test]
    fn never_leaves_the_pad_byte_in_a_packet_of_its_own() {
        let pixels = [1, 2, 7, 7, 7, 4, 5, 6, 7, 8];

        let body = encode_byte_run1_rows(&pixels, 5, 2).unwrap();

        assert_eq!(
            body,
            [
                1, 1, 2, // literal 1,2
                255, 7, // a run of three shortened to two so the pad travels with a pixel
                1, 7, 0, // the last pixel and the pad in one literal
                5, 4, 5, 6, 7, 8, 0, // the second row's literal swallows its own pad
            ]
        );
        assert_eq!(decode_byte_run1_rows(&body, 5, 2).unwrap(), pixels);
    }

    /// The other half of the same rule, on the literal arm: a literal that would
    /// end on the last pixel is always already at its 128-byte maximum -- that
    /// is the only reason the literal loop can stop there -- so it gives up a
    /// byte rather than growing to cover the pad.
    #[test]
    fn shortens_a_full_literal_rather_than_stranding_the_pad() {
        let mut row = vec![6_u8; 3];
        row.extend((0..128).map(|index| (index as u8) | 0x80));
        assert_eq!(row.len(), 131);
        let mut pixels = row.clone();
        pixels.extend(std::iter::repeat_n(9_u8, 131));

        let body = encode_byte_run1_rows(&pixels, 131, 2).unwrap();

        assert_eq!(body[0], 254, "three 6s as one repeat packet");
        assert_eq!(body[2], 126, "the 128-byte literal gives up one byte");
        assert_eq!(decode_byte_run1_rows(&body, 131, 2).unwrap(), pixels);
    }

    #[test]
    fn encodes_a_one_pixel_wide_image_row_by_row() {
        let body = encode_byte_run1_rows(&[5, 6, 7], 1, 3).unwrap();
        assert_eq!(body, [1, 5, 0, 1, 6, 0, 1, 7, 0]);
        assert_eq!(decode_byte_run1_rows(&body, 1, 3).unwrap(), [5, 6, 7]);
    }

    #[test]
    fn round_trips_a_mixed_image_through_the_encoder() {
        let width = 37;
        let height = 11;
        let pixels: Vec<u8> = (0..width * height)
            .map(|index| match index % 17 {
                0..=9 => 200,
                10 | 11 => 3,
                value => (value * 7) as u8,
            })
            .collect();
        let body = encode_byte_run1_rows(&pixels, width, height).unwrap();
        assert_eq!(decode_byte_run1_rows(&body, width, height).unwrap(), pixels);
    }

    #[test]
    fn refuses_a_pixel_count_that_does_not_match_the_dimensions() {
        let error = encode_byte_run1_rows(&[0; 9], 5, 2).unwrap_err();
        assert!(error.to_string().contains("expected 10"), "{error}");
    }

    fn file_with_side_chunks(compression: u8, body: &[u8]) -> Vec<u8> {
        form(&[
            (*b"BMHD", bmhd(4, 2, compression)),
            (*b"CMAP", cmap()),
            (*b"DPPS", vec![9; 7]),
            (*b"CRNG", vec![1, 2, 3, 4, 5, 6, 7, 8]),
            (*b"TINY", vec![42; 5]),
            (*b"BODY", body.to_vec()),
        ])
    }

    #[test]
    fn re_encoding_preserves_every_chunk_the_decoder_ignores() {
        let source = file_with_side_chunks(0, &[1, 2, 3, 4, 5, 6, 7, 8]);
        let file = PbmFile::parse(&source).unwrap();

        let encoded = file.encode().unwrap();

        let chunks = split_chunks(&encoded).unwrap();
        let ids: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.id.as_slice()).collect();
        assert_eq!(
            ids,
            [
                b"BMHD".as_slice(),
                b"CMAP",
                b"DPPS",
                b"CRNG",
                b"TINY",
                b"BODY"
            ],
            "chunk order and membership must survive a re-encode"
        );
        assert_eq!(
            chunks[2].data,
            vec![9; 7],
            "DPPS must survive byte for byte"
        );
        assert_eq!(chunks[3].data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            chunks[4].data,
            vec![42; 5],
            "the TINY thumbnail must survive byte for byte"
        );
    }

    #[test]
    fn re_encoding_an_uncompressed_source_declares_byte_run1() {
        let source = file_with_side_chunks(0, &[1, 2, 3, 4, 5, 6, 7, 8]);
        let file = PbmFile::parse(&source).unwrap();

        let encoded = file.encode().unwrap();

        let reparsed = PbmFile::parse(&encoded).unwrap();
        assert_eq!(
            reparsed.image.compression, 1,
            "the writer only emits ByteRun1, so BMHD must say so"
        );
        assert_eq!(reparsed.image.indices, file.image.indices);
    }

    #[test]
    fn re_encoding_is_pixel_lossless_and_reparses() {
        let source = file_with_side_chunks(1, &[3, 1, 2, 3, 4, 3, 5, 6, 7, 8]);
        let file = PbmFile::parse(&source).unwrap();

        let encoded = file.encode().unwrap();

        let reparsed = PbmFile::parse(&encoded).unwrap();
        assert_eq!(reparsed.image.indices, file.image.indices);
        assert_eq!(reparsed.image.rgba, file.image.rgba);
        assert_eq!(
            (reparsed.image.width, reparsed.image.height),
            (file.image.width, file.image.height)
        );
    }

    #[test]
    fn writes_a_form_size_covering_every_chunk() {
        let source = file_with_side_chunks(1, &[3, 1, 2, 3, 4, 3, 5, 6, 7, 8]);
        let encoded = PbmFile::parse(&source).unwrap().encode().unwrap();

        let form_size = u32::from_be_bytes(encoded[4..8].try_into().unwrap()) as usize;
        assert_eq!(
            form_size + 8,
            encoded.len(),
            "the FORM size must cover the whole file"
        );
    }

    #[test]
    fn pads_an_odd_sized_chunk_in_the_written_file() {
        let source = form(&[
            (*b"BMHD", bmhd(4, 2, 1)),
            (*b"CMAP", cmap()),
            (*b"DPPS", vec![9; 7]),
            (*b"BODY", vec![3, 1, 2, 3, 4, 3, 5, 6, 7, 8]),
        ]);
        let encoded = PbmFile::parse(&source).unwrap().encode().unwrap();

        // A seven-byte DPPS must be followed by one pad byte, or every later
        // chunk reads misaligned.
        let position = encoded
            .windows(4)
            .position(|window| window == b"DPPS")
            .expect("DPPS survives");
        assert_eq!(encoded[position + 8 + 7], 0, "odd chunks carry a pad byte");
        assert_eq!(&encoded[position + 8 + 8..position + 8 + 12], b"BODY");
    }

    #[test]
    fn refuses_indices_that_do_not_fill_the_image() {
        let source = file_with_side_chunks(1, &[3, 1, 2, 3, 4, 3, 5, 6, 7, 8]);
        let file = PbmFile::parse(&source).unwrap();

        let error = file.encode_with_indices(&[0; 7]).unwrap_err();

        assert!(error.to_string().contains("expected 8"), "{error}");
    }

    #[test]
    fn refuses_an_index_the_palette_does_not_contain() {
        let source = form(&[
            (*b"BMHD", bmhd(4, 2, 1)),
            (*b"CMAP", cmap()[..12].to_vec()),
            (*b"BODY", vec![3, 1, 2, 3, 0, 3, 1, 2, 3, 0]),
        ]);
        let file = PbmFile::parse(&source).unwrap();

        let error = file
            .encode_with_indices(&[0, 1, 2, 3, 0, 1, 2, 9])
            .unwrap_err();

        assert!(error.to_string().contains("palette index 9"), "{error}");
    }

    /// `TINY` is a thumbnail of `BODY`, so carrying it across a pixel-changing
    /// re-encode would ship the *old* artwork as the file's own preview. It is
    /// dropped instead; `CRNG` and `DPPS` describe no pixels and stay.
    #[test]
    fn changing_the_pixels_drops_the_stale_tiny_thumbnail() {
        let source = file_with_side_chunks(1, &[3, 1, 2, 3, 4, 3, 5, 6, 7, 8]);
        let file = PbmFile::parse(&source).unwrap();
        let replacement = [9_u8, 9, 9, 9, 200, 201, 202, 203];
        assert_ne!(file.image.indices.as_slice(), replacement.as_slice());

        let encoded = file.encode_with_indices(&replacement).unwrap();

        let chunks = split_chunks(&encoded).unwrap();
        let ids: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.id.as_slice()).collect();
        assert_eq!(
            ids,
            [b"BMHD".as_slice(), b"CMAP", b"DPPS", b"CRNG", b"BODY"],
            "a stale TINY must be dropped, and nothing else with it"
        );
        assert_eq!(chunks[2].data, vec![9; 7], "DPPS is not derived from BODY");
        assert_eq!(chunks[3].data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(PbmFile::parse(&encoded).unwrap().image.indices, replacement);
    }

    /// The other side of the rule: re-encoding the *same* pixels leaves the
    /// thumbnail describing them accurately, so it must survive. This is the
    /// path `--pbm-roundtrip` sweeps the corpus with.
    #[test]
    fn re_encoding_the_same_pixels_through_encode_with_indices_keeps_tiny() {
        let source = file_with_side_chunks(1, &[3, 1, 2, 3, 4, 3, 5, 6, 7, 8]);
        let file = PbmFile::parse(&source).unwrap();
        let same = file.image.indices.clone();

        let encoded = file.encode_with_indices(&same).unwrap();

        let chunks = split_chunks(&encoded).unwrap();
        let tiny = chunks
            .iter()
            .find(|chunk| &chunk.id == b"TINY")
            .expect("an unchanged image keeps its thumbnail");
        assert_eq!(tiny.data, vec![42; 5]);
    }

    #[test]
    fn encode_with_indices_writes_the_new_pixels() {
        let source = file_with_side_chunks(1, &[3, 1, 2, 3, 4, 3, 5, 6, 7, 8]);
        let file = PbmFile::parse(&source).unwrap();
        let replacement = [9_u8, 9, 9, 9, 200, 201, 202, 203];

        let encoded = file.encode_with_indices(&replacement).unwrap();

        assert_eq!(PbmFile::parse(&encoded).unwrap().image.indices, replacement);
    }

    #[test]
    fn clamps_byte_run1_packets_at_scanline_boundaries() {
        let image = PbmImage::decode(&pbm(1, &[3, 0, 1, 1, 0])).unwrap();
        assert_eq!(image.rgba, [10, 20, 30, 255, 40, 50, 60, 255]);
    }
    // -----------------------------------------------------------------------
    // The installed corpus
    // -----------------------------------------------------------------------
    //
    // Run with:
    //   LOM_GAME_DIR=.../English cargo test --release -- --ignored
    //
    // `pic.mpq` carries no listfile, but every PBM is identified by content rather than by name,
    // so the synthesised `File%08u.xxx` names StormLib hands back are enough here. (`imp.mpq` is
    // not: see the note on the IMP sweep.)

    /// **Observed in the corpus, 2026-09-19.** The PBM populations this machine's installs hold.
    ///
    /// There is more than one, and pinning a single number would have been wrong: the stock
    /// `pic.mpq` in the Steam build and in `Lords of Magic Development` holds 1,045 PBM members,
    /// while GS5R3's replacement `pic.mpq` holds 1,377. `docs/native-asset-stage.md` quotes the
    /// first and `docs/agent-handoff.md` the second, and both are right about their own archive.
    ///
    /// Listing them rather than asserting `> 0` keeps the tripwire sharp -- an archive that
    /// yielded nothing, or a decoder regression that stopped recognising a third of the members,
    /// still fails -- while refusing to pretend the corpus has one shape when it has two. A new
    /// install with a third population is a prompt to measure it and add it here with a date.
    const ATTESTED_PBM_POPULATIONS: &[(&str, usize)] =
        &[("stock pic.mpq", 1_045), ("GS5R3 pic.mpq", 1_377)];

    /// **Observed in the corpus, 2026-09-19.** Members whose re-encoded file is byte-identical to
    /// the original, in *both* populations.
    ///
    /// Low, and that is a finding rather than a shortfall: the shipped art was packed by at least
    /// two different ByteRun1 packers and this encoder reproduces neither exactly. What the sweep
    /// gates on is pixel-losslessness and non-`BODY` chunk preservation; this number is asserted so
    /// that a change in the encoder's packet boundaries is visible rather than silent.
    const PBM_BYTE_IDENTICAL_FILES: usize = 8;

    /// **Observed in the corpus, 2026-09-19.** Odd-width members, in both populations. These are
    /// the images that decode only under the "ByteRun1 rows are padded to an even byte count"
    /// rule, so the count is the standing evidence for it.
    const PBM_ODD_WIDTH_MEMBERS: usize = 88;

    fn game_directory() -> std::path::PathBuf {
        let directory = std::env::var_os("LOM_GAME_DIR")
            .map(std::path::PathBuf::from)
            .expect("set LOM_GAME_DIR to the installed English directory");
        assert!(
            directory.join("lomse.exe").is_file(),
            "no lomse.exe under {}",
            directory.display()
        );
        directory
    }

    /// Every PBM member of the installed `pic.mpq` survives a parse/encode/parse round trip with
    /// its pixels intact and its non-`BODY` chunks untouched.
    ///
    /// The pixel assertion is the one that carries the reimport claim. The chunk assertion is the
    /// one that would have caught a regression the pixel check cannot see: deleting the
    /// chunk-preserving arm of the encoder outright still leaves every pixel correct while 917
    /// files silently lose their `CRNG`, `DPPS` and `TINY` chunks.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn every_archived_pbm_round_trips() {
        let archive =
            crate::mpq::Archive::open(&game_directory().join("pic.mpq")).expect("open pic.mpq");
        let entries = archive.entries().expect("enumerate pic.mpq");
        assert!(!entries.is_empty(), "pic.mpq held no members at all");

        let mut checked = 0_usize;
        let mut pixel_lossless = 0_usize;
        let mut file_identical = 0_usize;
        let mut odd_width = 0_usize;
        let mut failures = Vec::new();

        for entry in &entries {
            let bytes = match archive.read(&entry.name) {
                Ok(bytes) => bytes,
                Err(error) => {
                    failures.push(format!("{}: could not read: {error}", entry.name));
                    continue;
                }
            };
            // ILBM is planar and this encoder does not write it, so skipping is honest. The
            // population tripwire below is what stops a skip-everything regression passing.
            if !matches!(
                crate::asset::probe(&entry.name, &bytes).map(|info| info.kind),
                Ok(crate::asset::AssetKind::IffPbm)
            ) {
                continue;
            }
            let file = match PbmFile::parse(&bytes) {
                Ok(file) => file,
                Err(error) => {
                    failures.push(format!("{}: {error}", entry.name));
                    continue;
                }
            };
            checked += 1;
            if file.image.width % 2 == 1 {
                odd_width += 1;
            }

            let encoded = match file.encode() {
                Ok(encoded) => encoded,
                Err(error) => {
                    failures.push(format!("{}: could not re-encode: {error}", entry.name));
                    continue;
                }
            };
            let rewritten = match PbmFile::parse(&encoded) {
                Ok(rewritten) => rewritten,
                Err(error) => {
                    failures.push(format!(
                        "{}: re-encoded file does not parse: {error}",
                        entry.name
                    ));
                    continue;
                }
            };
            if rewritten.image.indices == file.image.indices {
                pixel_lossless += 1;
            } else {
                let at = rewritten
                    .image
                    .indices
                    .iter()
                    .zip(&file.image.indices)
                    .position(|(wrote, read)| wrote != read);
                failures.push(format!(
                    "{}: pixels changed (first differing pixel {at:?})",
                    entry.name
                ));
            }

            let describe = |file: &PbmFile| -> Vec<(String, usize)> {
                file.chunks
                    .iter()
                    .filter(|chunk| &chunk.id != b"BODY")
                    .map(|chunk| {
                        (
                            String::from_utf8_lossy(&chunk.id).into_owned(),
                            chunk.data.len(),
                        )
                    })
                    .collect()
            };
            let theirs = describe(&file);
            let ours = describe(&rewritten);
            if theirs != ours {
                failures.push(format!(
                    "{}: non-BODY chunks changed: theirs={theirs:?} ours={ours:?}",
                    entry.name
                ));
            } else if file
                .chunks
                .iter()
                .zip(&rewritten.chunks)
                .any(|(left, right)| &left.id != b"BODY" && left.data != right.data)
            {
                failures.push(format!(
                    "{}: a non-BODY chunk kept its length but changed its bytes",
                    entry.name
                ));
            }

            if encoded == bytes {
                file_identical += 1;
            }
        }

        assert_eq!(failures, Vec::<String>::new());
        // The tripwire. A sweep over zero members would satisfy every assertion above.
        assert!(
            ATTESTED_PBM_POPULATIONS
                .iter()
                .any(|(_, count)| *count == checked),
            "the PBM corpus holds {checked} members, which matches no attested population \
             ({ATTESTED_PBM_POPULATIONS:?}) -- re-measure and record it rather than widening this"
        );
        assert_eq!(
            pixel_lossless, checked,
            "a member did not survive pixel-lossless"
        );
        assert_eq!(odd_width, PBM_ODD_WIDTH_MEMBERS);
        assert_eq!(file_identical, PBM_BYTE_IDENTICAL_FILES);
    }
}
