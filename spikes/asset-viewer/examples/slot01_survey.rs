//! Throwaway: are palette slots 0 and 1 pure green and pure red across the whole corpus?
//!
//! Boaster's claim (impz 2086) is about "the palettes", plural. It had been checked against one
//! file, which is not the same statement.
use lom_asset_viewer::mpq::Archive;

fn main() {
    let mut args = std::env::args().skip(1);
    let archive = Archive::open(std::path::Path::new(&args.next().expect("archive"))).expect("open");
    let listfile = args.next().expect("listfile");
    let (mut files, mut green0_red1, mut other) = (0u64, 0u64, 0u64);
    let mut shapes: std::collections::BTreeMap<String, u64> = Default::default();
    for name in std::fs::read_to_string(listfile).expect("read").lines() {
        if !name.to_ascii_lowercase().ends_with(".imp") {
            continue;
        }
        let Ok(bytes) = archive.read(name) else { continue };
        if bytes.len() < 12 {
            continue;
        }
        let at = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        if at + 8 > bytes.len() {
            continue;
        }
        files += 1;
        // Stored blue, green, red, pad (research log, 2026-09-23).
        let slot = |i: usize| {
            let b = at + i * 4;
            (bytes[b + 2], bytes[b + 1], bytes[b]) // (r, g, b)
        };
        let (zero, one) = (slot(0), slot(1));
        if zero == (0, 255, 0) && one == (255, 0, 0) {
            green0_red1 += 1;
        } else {
            other += 1;
            *shapes.entry(format!("slot0 rgb {zero:?}  slot1 rgb {one:?}")).or_default() += 1;
        }
    }
    println!("files with a readable palette: {files}");
    println!("  slot 0 pure green AND slot 1 pure red: {green0_red1}");
    println!("  anything else:                         {other}");
    let mut ranked: Vec<_> = shapes.into_iter().collect();
    ranked.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (shape, count) in ranked.into_iter().take(6) {
        println!("    {count:>5}  {shape}");
    }
}
