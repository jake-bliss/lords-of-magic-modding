//! Throwaway: write RAW palette bytes into an IMP, and report which frame pixels use them.
//!
//! Writing raw bytes rather than going through the decoder is the point: comparing the bytes we
//! wrote against the pixels the engine renders settles the channel order without assuming ours.
use lom_asset_viewer::imp::ImpSprite;
use std::collections::BTreeMap;

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("input imp");
    let output = args.next().expect("output imp");
    let mut bytes = std::fs::read(&input).expect("read");
    let sprite = ImpSprite::parse(&bytes).expect("parse");
    let frame = &sprite.frames[0];

    let mut used: BTreeMap<u8, usize> = BTreeMap::new();
    for index in &frame.palette_indices {
        *used.entry(*index).or_default() += 1;
    }
    let mut ranked: Vec<(u8, usize)> = used.into_iter().filter(|(i, _)| *i > 1).collect();
    ranked.sort_by_key(|(_, n)| std::cmp::Reverse(*n));

    let palette_offset = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    println!("frame0 {}x{}  palette@0x{palette_offset:x}  key={}", frame.width, frame.height, sprite.color_key);

    // Author four unmistakable entries as RAW bytes, plus index 1 (the shadow candidate).
    let targets: Vec<(u8, [u8; 4])> = vec![
        (ranked[0].0, [0xff, 0x00, 0x00, 0x00]),
        (ranked[1].0, [0x00, 0xff, 0x00, 0x00]),
        (ranked[2].0, [0x00, 0x00, 0xff, 0x00]),
        (ranked[3].0, [0xff, 0xff, 0xff, 0x00]),
        (1,           [0xff, 0x00, 0xff, 0x00]),
    ];
    for (index, raw) in &targets {
        let at = palette_offset + usize::from(*index) * 4;
        let before: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        bytes[at..at + 4].copy_from_slice(raw);
        let count = frame.palette_indices.iter().filter(|i| *i == index).count();
        println!("  index {index:3}: raw {before:02x?} -> {raw:02x?}   {count} pixels");
    }
    std::fs::write(&output, &bytes).expect("write");

    // Where each authored index sits inside the frame, so the capture can be read without guessing.
    println!("\nframe-local positions (col,row) of the first pixel of each authored index:");
    for (index, raw) in &targets {
        if let Some(pos) = frame.palette_indices.iter().position(|i| i == index) {
            println!("  index {index:3} raw {raw:02x?} first at ({}, {})",
                     pos % usize::from(frame.width), pos / usize::from(frame.width));
        }
    }
    // Emit a compact map so the analysis step can sample many pixels per index.
    let mut map = String::new();
    for (index, _) in &targets {
        let positions: Vec<String> = frame
            .palette_indices
            .iter()
            .enumerate()
            .filter(|(_, i)| *i == index)
            .take(400)
            .map(|(p, _)| format!("{},{}", p % usize::from(frame.width), p / usize::from(frame.width)))
            .collect();
        map.push_str(&format!("{index} {}\n", positions.join(" ")));
    }
    std::fs::write(format!("{output}.map"), map).expect("write map");
    println!("\nwrote {output} and {output}.map");
}
