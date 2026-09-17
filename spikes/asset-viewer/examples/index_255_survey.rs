//! Throwaway: does palette index 0xff ever appear in pixel data?
//!
//! snv's 2011 specification (impz thread 2012) lists it beside the transparency and shadow
//! indices: "index 0 used for transparency, index 1 is shadow index 0xff is RLE special value".
//! If it is reserved, it should never occur in decoded pixels.
use lom_asset_viewer::imp::ImpSprite;
use lom_asset_viewer::mpq::Archive;

fn main() {
    let mut args = std::env::args().skip(1);
    let archive = Archive::open(std::path::Path::new(&args.next().expect("archive"))).expect("open");
    let listfile = args.next().expect("listfile");
    let (mut files, mut files_with_255, mut frames, mut frames_with_255) = (0u64, 0u64, 0u64, 0u64);
    let (mut pixels, mut pixels_255) = (0u64, 0u64);
    let mut examples: Vec<String> = Vec::new();
    for name in std::fs::read_to_string(listfile).expect("read").lines() {
        if !name.to_ascii_lowercase().ends_with(".imp") {
            continue;
        }
        let Ok(bytes) = archive.read(name) else { continue };
        let Ok(sprite) = ImpSprite::parse(&bytes) else { continue };
        files += 1;
        let mut file_has = false;
        for frame in &sprite.frames {
            if frame.palette_indices.is_empty() {
                continue;
            }
            frames += 1;
            let count = frame.palette_indices.iter().filter(|i| **i == 0xff).count() as u64;
            pixels += frame.palette_indices.len() as u64;
            pixels_255 += count;
            if count > 0 {
                frames_with_255 += 1;
                if !file_has && examples.len() < 400 {
                    examples.push(format!("{name} ({count} px)"));
                }
                file_has = true;
            }
        }
        if file_has {
            files_with_255 += 1;
        }
    }
    println!("files {files}, with index 255: {files_with_255}");
    println!("frames {frames}, with index 255: {frames_with_255}");
    println!("pixels {pixels}, index 255: {pixels_255}");
    for example in examples {
        println!("  e.g. {example}");
    }
}
