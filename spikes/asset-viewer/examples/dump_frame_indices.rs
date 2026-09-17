//! Throwaway: dump a frame's palette indices and raw palette bytes as TSV.
//!
//! Pairing "the raw bytes in the file" with "the pixel the engine rendered" over every index in a
//! frame is what settles the palette channel order without assuming our decoder's own ordering.
use lom_asset_viewer::imp::ImpSprite;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("imp path");
    let bytes = std::fs::read(&path).expect("read");
    let sprite = ImpSprite::parse(&bytes).expect("parse");
    let frame = &sprite.frames[0];
    let palette_offset = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    println!("size\t{}\t{}", frame.width, frame.height);
    for index in 0..256usize {
        let at = palette_offset + index * 4;
        println!(
            "palette\t{index}\t{}\t{}\t{}\t{}",
            bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]
        );
    }
    for (position, index) in frame.palette_indices.iter().enumerate() {
        println!(
            "pixel\t{}\t{}\t{index}",
            position % usize::from(frame.width),
            position / usize::from(frame.width)
        );
    }
}
