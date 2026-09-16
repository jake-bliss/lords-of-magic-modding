use std::io::Write;

use crate::imp::ImpSprite;

pub fn write_imp_frame_png<W: Write>(
    writer: W,
    sprite: &ImpSprite,
    frame_index: usize,
) -> Result<(), String> {
    let frame = sprite
        .resolved_frame(frame_index)
        .map_err(|error| error.to_string())?;
    write_indexed_png(
        writer,
        frame.width,
        frame.height,
        &sprite.palette,
        &frame.palette_indices,
        sprite.color_key,
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
    palette: &[[u8; 4]],
    palette_indices: &[u8],
    color_key: u8,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("cannot export an empty IMP frame".to_owned());
    }
    if palette.is_empty() || palette.len() > 256 {
        return Err(format!(
            "indexed PNG requires 1 to 256 palette entries; got {}",
            palette.len()
        ));
    }
    let expected_pixels = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| "IMP frame dimensions overflow".to_owned())?;
    if palette_indices.len() != expected_pixels {
        return Err(format!(
            "IMP frame has {} palette indices; expected {expected_pixels}",
            palette_indices.len()
        ));
    }
    if let Some(index) = palette_indices
        .iter()
        .find(|index| usize::from(**index) >= palette.len())
    {
        return Err(format!("IMP frame uses missing palette index {index}"));
    }

    let palette_rgb: Vec<u8> = palette
        .iter()
        .flat_map(|rgba| [rgba[0], rgba[1], rgba[2]])
        .collect();
    let mut encoder = png::Encoder::new(writer, u32::from(width), u32::from(height));
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_palette(palette_rgb);
    // The header's colour-key index is transparent and slot 1 is the shadow silhouette.
    // Without a tRNS chunk the exported frame is fully opaque and the key is lost, even
    // though the interactive viewer honours it. tRNS entries apply to palette indices in
    // order, so mark every index up to the key opaque and the key itself transparent.
    let mut transparency = vec![255_u8; usize::from(color_key) + 1];
    transparency[usize::from(color_key)] = 0;
    encoder.set_trns(transparency);
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

    use super::{write_indexed_png, write_rgba_png};

    #[test]
    fn marks_a_nonzero_colour_key_transparent() {
        let mut palette = vec![[0, 0, 0, 255]; 256];
        palette[0] = [0, 255, 0, 255];
        palette[188] = [12, 34, 56, 255];
        let indices = [188, 7, 188, 7, 188, 7];
        let mut encoded = Vec::new();

        write_indexed_png(&mut encoded, 3, 2, &palette, &indices, 188).unwrap();

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
        let mut palette = vec![[0, 0, 0, 255]; 256];
        palette[0] = [0, 255, 0, 255];
        palette[1] = [255, 0, 0, 255];
        palette[42] = [1, 2, 3, 255];
        let indices = [0, 1, 42, 1, 0, 42];
        let mut encoded = Vec::new();

        write_indexed_png(&mut encoded, 3, 2, &palette, &indices, 0).unwrap();

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

    fn palette_bytes(palette: &[[u8; 4]]) -> Vec<u8> {
        palette
            .iter()
            .flat_map(|rgba| [rgba[0], rgba[1], rgba[2]])
            .collect()
    }
}
