use std::io::{BufRead, Seek, Write};

use crate::imp::ImpSprite;
use crate::pbm::PbmImage;

pub fn write_imp_frame_png<W: Write>(
    writer: W,
    sprite: &ImpSprite,
    frame_index: usize,
) -> Result<(), String> {
    let frame = sprite
        .resolved_frame(frame_index)
        .map_err(|error| error.to_string())?;
    let palette_rgb: Vec<[u8; 3]> = sprite
        .palette
        .iter()
        .map(|rgba| [rgba[0], rgba[1], rgba[2]])
        .collect();
    write_indexed_png(
        writer,
        frame.width,
        frame.height,
        &palette_rgb,
        &frame.palette_indices,
        // IMP frames always carry a colour-key shadow index, unlike PBM's
        // per-file masking field, so the key is always transparent here.
        Some(sprite.color_key),
    )
}

/// Writes a decoded PBM/LBM image as an indexed PNG, preserving the original
/// palette indices and palette bytes so the export round-trips losslessly.
///
/// The IFF BMHD `masking` field of 2 (`mskHasTransparentColor`) is the only
/// value that declares a transparent palette index; anything else means the
/// image has no colour-keyed transparency and gets no `tRNS` chunk at all.
pub fn write_pbm_png<W: Write>(writer: W, image: &PbmImage) -> Result<(), String> {
    let transparent_index = (image.masking == 2).then_some(image.transparent_color);
    write_indexed_png(
        writer,
        image.width,
        image.height,
        &image.palette,
        &image.indices,
        transparent_index,
    )
}

/// An indexed PNG read back in, in the terms a PBM re-encode needs.
#[derive(Debug)]
pub struct IndexedPng {
    pub width: u16,
    pub height: u16,
    pub palette: Vec<[u8; 3]>,
    pub indices: Vec<u8>,
}

/// Reads an 8-bit indexed PNG and hands back its raw palette indices.
///
/// Anything else is refused rather than converted. A truecolour or 4-bit PNG
/// carries no palette indices to re-import, and quantising one here would mint
/// pixel values the editor never chose.
///
/// `expected` is the template's size, and it is checked against the PNG's
/// *header* before any pixel buffer is allocated. That ordering is the point:
/// the decode buffer is sized from the header, so a truncated PNG declaring
/// 65535x65535 would otherwise ask for ~4.29 GB and could kill the process
/// before its short IDAT was ever reported. The import already requires the
/// sizes to match, so refusing the mismatch first costs nothing and bounds the
/// allocation by a file the caller already holds.
pub fn read_indexed_png<R: BufRead + Seek>(
    reader: R,
    expected: (u16, u16),
) -> Result<IndexedPng, String> {
    let decoder = png::Decoder::new(reader);
    let mut reader = decoder
        .read_info()
        .map_err(|error| format!("could not read PNG header: {error}"))?;
    let info = reader.info().clone();
    // Before anything is sized from the header, refuse a header that does not
    // describe the template.
    if (info.width, info.height) != (u32::from(expected.0), u32::from(expected.1)) {
        return Err(format!(
            "PNG is {}x{} but the template is {}x{}; the PBM header is inherited, so the sizes \
             must match",
            info.width, info.height, expected.0, expected.1,
        ));
    }
    if info.color_type != png::ColorType::Indexed {
        return Err(format!(
            "expected an indexed PNG; got {:?}. Export with --export-pbm, edit the palette \
             indices, and save as an 8-bit indexed PNG",
            info.color_type
        ));
    }
    if info.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "expected an 8-bit indexed PNG; got {:?}",
            info.bit_depth
        ));
    }
    let palette = info
        .palette
        .as_deref()
        .ok_or_else(|| "indexed PNG has no PLTE chunk".to_owned())?;
    if palette.len() % 3 != 0 {
        return Err(format!(
            "indexed PNG palette is {} bytes, which is not a whole number of RGB triples",
            palette.len()
        ));
    }
    let palette: Vec<[u8; 3]> = palette
        .chunks_exact(3)
        .map(|color| [color[0], color[1], color[2]])
        .collect();

    // The header was checked against the template above, so these are the
    // template's own dimensions and the buffer below is bounded by them.
    let (width, height) = expected;

    let mut buffer = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| "PNG dimensions overflow".to_owned())?
    ];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|error| format!("could not read PNG pixels: {error}"))?;
    buffer.truncate(frame.buffer_size());

    // The PNG spec makes an index past the end of PLTE an error, and it matters
    // here beyond conformance: the import keeps the *template's* CMAP, so an
    // index the PNG's own palette never described would silently pick up a
    // colour from the template that the editor never saw.
    if let Some(index) = buffer
        .iter()
        .find(|index| usize::from(**index) >= palette.len())
    {
        return Err(format!(
            "indexed PNG uses palette index {index} but its PLTE has only {} entries",
            palette.len()
        ));
    }

    Ok(IndexedPng {
        width,
        height,
        palette,
        indices: buffer,
    })
}

pub fn write_rgba_png<W: Write>(
    writer: W,
    width: u16,
    height: u16,
    rgba: &[u8],
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("cannot export an empty RGBA image".to_owned());
    }
    let expected_bytes = usize::from(width)
        .checked_mul(usize::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "RGBA image dimensions overflow".to_owned())?;
    if rgba.len() != expected_bytes {
        return Err(format!(
            "RGBA image has {} bytes; expected {expected_bytes}",
            rgba.len()
        ));
    }

    let mut encoder = png::Encoder::new(writer, u32::from(width), u32::from(height));
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png_writer = encoder
        .write_header()
        .map_err(|error| format!("could not write PNG header: {error}"))?;
    png_writer
        .write_image_data(rgba)
        .map_err(|error| format!("could not write PNG pixels: {error}"))
}

fn write_indexed_png<W: Write>(
    writer: W,
    width: u16,
    height: u16,
    palette: &[[u8; 3]],
    palette_indices: &[u8],
    transparent_index: Option<u8>,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("cannot export an empty indexed image".to_owned());
    }
    if palette.is_empty() || palette.len() > 256 {
        return Err(format!(
            "indexed PNG requires 1 to 256 palette entries; got {}",
            palette.len()
        ));
    }
    let expected_pixels = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| "indexed image dimensions overflow".to_owned())?;
    if palette_indices.len() != expected_pixels {
        return Err(format!(
            "indexed image has {} palette indices; expected {expected_pixels}",
            palette_indices.len()
        ));
    }
    if let Some(index) = palette_indices
        .iter()
        .find(|index| usize::from(**index) >= palette.len())
    {
        return Err(format!("indexed image uses missing palette index {index}"));
    }
    if let Some(index) = transparent_index
        && usize::from(index) >= palette.len()
    {
        return Err(format!(
            "transparent index {index} exceeds {} palette entries",
            palette.len()
        ));
    }

    let palette_rgb: Vec<u8> = palette.iter().flatten().copied().collect();
    let mut encoder = png::Encoder::new(writer, u32::from(width), u32::from(height));
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_palette(palette_rgb);
    if let Some(index) = transparent_index {
        // tRNS entries apply to palette indices in order, so mark every index up
        // to the transparent one opaque and the transparent one itself clear;
        // there is no need to describe indices past it.
        let mut transparency = vec![255_u8; usize::from(index) + 1];
        transparency[usize::from(index)] = 0;
        encoder.set_trns(transparency);
    }
    let mut png_writer = encoder
        .write_header()
        .map_err(|error| format!("could not write PNG header: {error}"))?;
    png_writer
        .write_image_data(palette_indices)
        .map_err(|error| format!("could not write PNG pixels: {error}"))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{read_indexed_png, write_indexed_png, write_pbm_png, write_rgba_png};
    use crate::pbm::PbmImage;

    /// Encodes an indexed PNG through the `png` crate directly, so a test can
    /// build files [`write_indexed_png`] would refuse -- such as one whose
    /// pixels reach past its own PLTE.
    fn raw_indexed_png(width: u32, height: u32, palette: &[u8], indices: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(palette.to_vec());
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(indices).unwrap();
        drop(writer);
        encoded
    }

    /// The PNG spec makes an index past the end of PLTE an error. It is worse
    /// than a conformance miss here: the import keeps the *template's* CMAP, so
    /// index 200 under a one-entry PLTE would be written as the template's
    /// colour 200 -- a colour the PNG never described and the editor never saw.
    #[test]
    fn refuses_an_index_beyond_the_pngs_own_palette() {
        let encoded = raw_indexed_png(2, 1, &[7, 8, 9], &[0, 200]);

        let error = read_indexed_png(Cursor::new(encoded), (2, 1)).unwrap_err();

        assert!(
            error.contains("palette index 200") && error.contains("only 1"),
            "{error}"
        );
    }

    /// The boundary the off-by-one lives on: with two entries, index 2 is one
    /// past the end. A `>` in place of `>=` would wave this through.
    #[test]
    fn refuses_the_index_one_past_the_last_palette_entry() {
        let encoded = raw_indexed_png(2, 1, &[7, 8, 9, 1, 2, 3], &[1, 2]);

        let error = read_indexed_png(Cursor::new(encoded), (2, 1)).unwrap_err();

        assert!(
            error.contains("palette index 2") && error.contains("only 2"),
            "{error}"
        );
    }

    #[test]
    fn accepts_an_index_at_the_last_palette_entry() {
        let encoded = raw_indexed_png(2, 1, &[7, 8, 9, 1, 2, 3], &[1, 0]);

        let png = read_indexed_png(Cursor::new(encoded), (2, 1)).unwrap();

        assert_eq!(png.indices, [1, 0], "index 1 of a 2-entry PLTE is legal");
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xffff_ffff_u32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    fn png_chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
        chunk.extend_from_slice(id);
        chunk.extend_from_slice(data);
        let mut crc_input = id.to_vec();
        crc_input.extend_from_slice(data);
        chunk.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        chunk
    }

    /// The decode buffer is sized from the PNG *header*, so a file that declares
    /// 65535x65535 and then carries no pixels at all asks for ~4.29 GB before
    /// the missing IDAT can be reported. Checking the header against the
    /// template first bounds the allocation by a file the caller already holds.
    ///
    /// The fixture declares that size and then carries a two-byte IDAT: enough
    /// for the header parse to complete, nowhere near enough pixels. Any code
    /// path that reaches the allocation has already lost.
    #[test]
    fn refuses_a_header_that_does_not_match_the_template_before_decoding() {
        let mut header = Vec::new();
        header.extend_from_slice(&65535_u32.to_be_bytes());
        header.extend_from_slice(&65535_u32.to_be_bytes());
        // 8-bit, colour type 3 (indexed), deflate, no filter, no interlace.
        header.extend_from_slice(&[8, 3, 0, 0, 0]);
        let mut encoded = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        encoded.extend_from_slice(&png_chunk(b"IHDR", &header));
        encoded.extend_from_slice(&png_chunk(b"PLTE", &[1, 2, 3]));
        // A zlib header and nothing after it: `read_info` stops at the IDAT
        // boundary, so the stream only has to exist, not to decode.
        encoded.extend_from_slice(&png_chunk(b"IDAT", &[0x78, 0x01]));
        encoded.extend_from_slice(&png_chunk(b"IEND", &[]));

        let error = read_indexed_png(Cursor::new(encoded), (4, 2)).unwrap_err();

        assert!(
            error.contains("PNG is 65535x65535 but the template is 4x2"),
            "{error}"
        );
    }

    #[test]
    fn marks_a_nonzero_colour_key_transparent() {
        let mut palette = vec![[0, 0, 0]; 256];
        palette[0] = [0, 255, 0];
        palette[188] = [12, 34, 56];
        let indices = [188, 7, 188, 7, 188, 7];
        let mut encoded = Vec::new();

        write_indexed_png(&mut encoded, 3, 2, &palette, &indices, Some(188)).unwrap();

        let decoder = png::Decoder::new(Cursor::new(encoded));
        let reader = decoder.read_info().unwrap();
        let transparency = reader.info().trns.as_deref().expect("tRNS chunk");
        assert_eq!(transparency.len(), 189);
        assert_eq!(transparency[188], 0, "the colour key must be transparent");
        assert!(
            transparency[..188].iter().all(|alpha| *alpha == 255),
            "every index below the colour key stays opaque"
        );
    }

    #[test]
    fn exports_indexed_pixels_and_palette_losslessly() {
        let mut palette = vec![[0, 0, 0]; 256];
        palette[0] = [0, 255, 0];
        palette[1] = [255, 0, 0];
        palette[42] = [1, 2, 3];
        let indices = [0, 1, 42, 1, 0, 42];
        let mut encoded = Vec::new();

        write_indexed_png(&mut encoded, 3, 2, &palette, &indices, Some(0)).unwrap();

        let decoder = png::Decoder::new(Cursor::new(encoded));
        let mut reader = decoder.read_info().unwrap();
        let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut decoded).unwrap();
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(info.color_type, png::ColorType::Indexed);
        assert_eq!(
            reader.info().trns.as_deref(),
            Some([0_u8].as_slice()),
            "indexed export must mark the colour-key index transparent"
        );
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        assert_eq!(&decoded[..info.buffer_size()], indices);
        assert_eq!(
            reader.info().palette.as_deref(),
            Some(&palette_bytes(&palette)[..])
        );
    }

    #[test]
    fn exports_rgba_pixels_losslessly() {
        let rgba = [1, 2, 3, 255, 10, 20, 30, 40];
        let mut encoded = Vec::new();

        write_rgba_png(&mut encoded, 2, 1, &rgba).unwrap();

        let decoder = png::Decoder::new(Cursor::new(encoded));
        let mut reader = decoder.read_info().unwrap();
        let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut decoded).unwrap();
        assert_eq!((info.width, info.height), (2, 1));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(&decoded[..info.buffer_size()], rgba);
    }

    fn pbm_image(masking: u8, transparent_color: u8) -> PbmImage {
        let mut palette = vec![[0, 0, 0]; 256];
        palette[0] = [0, 255, 0];
        palette[1] = [255, 0, 0];
        palette[200] = [1, 2, 3];
        let indices = vec![0, 1, 200, 1, 0, 200];
        PbmImage {
            width: 3,
            height: 2,
            rgba: Vec::new(),
            indices,
            palette,
            palette_entries: 256,
            compression: 0,
            masking,
            transparent_color,
        }
    }

    #[test]
    fn pbm_export_preserves_indices_and_palette_byte_exactly() {
        let image = pbm_image(0, 0);
        let mut encoded = Vec::new();

        write_pbm_png(&mut encoded, &image).unwrap();

        let decoder = png::Decoder::new(Cursor::new(encoded));
        let mut reader = decoder.read_info().unwrap();
        let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut decoded).unwrap();
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(info.color_type, png::ColorType::Indexed);
        assert_eq!(&decoded[..info.buffer_size()], image.indices.as_slice());
        assert_eq!(
            reader.info().palette.as_deref(),
            Some(&palette_bytes(&image.palette)[..])
        );
    }

    #[test]
    fn pbm_masking_two_marks_the_declared_index_transparent() {
        // Use a nonzero transparent index so a hardcoded index-0 assumption
        // (the bug already fixed for IMP frames) would be caught here too.
        let image = pbm_image(2, 200);
        let mut encoded = Vec::new();

        write_pbm_png(&mut encoded, &image).unwrap();

        let decoder = png::Decoder::new(Cursor::new(encoded));
        let reader = decoder.read_info().unwrap();
        let transparency = reader.info().trns.as_deref().expect("tRNS chunk");
        assert_eq!(transparency.len(), 201);
        assert_eq!(
            transparency[200], 0,
            "the declared transparent index must be transparent"
        );
        assert!(
            transparency[..200].iter().all(|alpha| *alpha == 255),
            "every index below the transparent one stays opaque"
        );
    }

    #[test]
    fn pbm_without_masking_has_no_trns_chunk() {
        let image = pbm_image(0, 200);
        let mut encoded = Vec::new();

        write_pbm_png(&mut encoded, &image).unwrap();

        let decoder = png::Decoder::new(Cursor::new(encoded));
        let reader = decoder.read_info().unwrap();
        assert_eq!(
            reader.info().trns,
            None,
            "an unmasked PBM must not declare any transparent index"
        );
    }

    fn palette_bytes(palette: &[[u8; 3]]) -> Vec<u8> {
        palette.iter().flatten().copied().collect()
    }
}
