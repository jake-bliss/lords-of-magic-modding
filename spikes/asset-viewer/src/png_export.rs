use std::io::Write;

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

    use super::{write_indexed_png, write_pbm_png, write_rgba_png};
    use crate::pbm::PbmImage;

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
