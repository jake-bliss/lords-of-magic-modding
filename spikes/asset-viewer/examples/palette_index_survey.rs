//! Throwaway: how often is palette index 1 (the community's "shadow" index) actually used?
use lom_asset_viewer::imp::ImpSprite;
use lom_asset_viewer::mpq::Archive;

fn main() {
    let mut args = std::env::args().skip(1);
    let archive = Archive::open(std::path::Path::new(&args.next().expect("archive"))).expect("open");
    let listfile = args.next().expect("listfile");
    let (mut files, mut files_with_1, mut frames, mut frames_with_1) = (0, 0, 0u64, 0u64);
    let (mut px_total, mut px_1, mut px_key) = (0u64, 0u64, 0u64);
    let mut examples = Vec::new();
    for name in std::fs::read_to_string(listfile).expect("read").lines() {
        if !name.to_ascii_lowercase().ends_with(".imp") { continue }
        let Ok(bytes) = archive.read(name) else { continue };
        let Ok(sprite) = ImpSprite::parse(&bytes) else { continue };
        files += 1;
        let mut file_has = false;
        for frame in &sprite.frames {
            if frame.palette_indices.is_empty() { continue }
            frames += 1;
            let ones = frame.palette_indices.iter().filter(|i| **i == 1).count() as u64;
            px_total += frame.palette_indices.len() as u64;
            px_1 += ones;
            px_key += frame.palette_indices.iter().filter(|i| **i == sprite.color_key).count() as u64;
            if ones > 0 { frames_with_1 += 1; file_has = true; }
        }
        if file_has {
            files_with_1 += 1;
            if examples.len() < 5 {
                let p = sprite.palette[1];
                examples.push(format!("{name} palette[1]={:?} key={}", &p[..3], sprite.color_key));
            }
        }
    }
    println!("files {files} ({files_with_1} use index 1)   frames {frames} ({frames_with_1} use index 1)");
    println!("pixels {px_total}: index1 {px_1} ({:.3}%), colour-key {px_key} ({:.1}%)",
             100.0 * px_1 as f64 / px_total as f64, 100.0 * px_key as f64 / px_total as f64);
    for e in &examples { println!("  {e}") }
}
