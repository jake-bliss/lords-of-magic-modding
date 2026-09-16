//! Throwaway: find a small terrain-placeable sprite that uses palette index 1 heavily.
use lom_asset_viewer::imp::ImpSprite;
use lom_asset_viewer::mpq::Archive;

fn main() {
    let mut args = std::env::args().skip(1);
    let archive = Archive::open(std::path::Path::new(&args.next().expect("archive"))).expect("open");
    let listfile = args.next().expect("listfile");
    let mut rows = Vec::new();
    for name in std::fs::read_to_string(listfile).expect("read").lines() {
        if !name.to_ascii_lowercase().ends_with(".imp") { continue }
        let Ok(bytes) = archive.read(name) else { continue };
        let Ok(sprite) = ImpSprite::parse(&bytes) else { continue };
        // want: exactly one frame (no animation), modest size, uses index 1
        if sprite.frames.len() != 1 { continue }
        let f = &sprite.frames[0];
        if f.palette_indices.is_empty() { continue }
        let ones = f.palette_indices.iter().filter(|i| **i == 1).count();
        if ones < 100 { continue }
        let distinct = f.palette_indices.iter().collect::<std::collections::BTreeSet<_>>().len();
        if f.width > 90 || f.height > 90 { continue }
        rows.push((ones, name.to_owned(), f.width, f.height, distinct, sprite.color_key,
                   sprite.palette[1], f.hotspots.len()));
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    println!("single-frame sprites using index 1 (top 12):");
    for (ones, name, w, h, distinct, key, p1, hs) in rows.iter().take(12) {
        println!("  {name:<34} {w}x{h}  index1={ones:<5} distinct={distinct:<4} key={key} palette[1]={:?} hotspots={hs}", &p1[..3]);
    }
}
