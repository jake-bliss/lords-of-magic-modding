//! Throwaway: is hotspot record 0 always type 0, and how many frames carry only it?
use lom_asset_viewer::imp::ImpSprite;
use lom_asset_viewer::mpq::Archive;
use std::collections::BTreeMap;

fn main() {
    let mut args = std::env::args().skip(1);
    let archive_path = args.next().expect("archive path");
    let listfile = args.next().expect("listfile");
    let archive = Archive::open(std::path::Path::new(&archive_path)).expect("open archive");
    let (mut total, mut slot0_is_type0, mut only_slot0) = (0_usize, 0_usize, 0_usize);
    let mut slot0_types: BTreeMap<u16, usize> = BTreeMap::new();
    let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
    let mut offenders: Vec<String> = Vec::new();
    let mut dup_type0 = 0_usize;
    for name in std::fs::read_to_string(listfile).expect("read listfile").lines() {
        if !name.to_ascii_lowercase().ends_with(".imp") {
            continue;
        }
        let Ok(bytes) = archive.read(name) else { continue };
        let Ok(sprite) = ImpSprite::parse(&bytes) else { continue };
        for frame in &sprite.frames {
            if frame.hotspots.is_empty() {
                continue;
            }
            total += 1;
            *counts.entry(frame.hotspots.len()).or_default() += 1;
            let first = frame.hotspots[0].id;
            *slot0_types.entry(first).or_default() += 1;
            if first == 0 {
                slot0_is_type0 += 1;
            } else if offenders.len() < 6 {
                offenders.push(format!("{name} type {first}"));
            }
            if frame.hotspots.len() == 1 {
                only_slot0 += 1;
            }
            if frame.hotspots[1..].iter().any(|spot| spot.id == 0) {
                dup_type0 += 1;
            }
        }
    }
    println!("frames with hotspot records: {total}");
    println!("  slot 0 has type 0        : {slot0_is_type0} ({:.2}%)", 100.0 * slot0_is_type0 as f64 / total as f64);
    println!("  slot 0 types seen        : {slot0_types:?}");
    println!("  frames with ONLY slot 0  : {only_slot0}");
    println!("  type 0 also in slots 1.. : {dup_type0}");
    println!("  record count histogram   : {counts:?}");
    if !offenders.is_empty() {
        println!("  slot-0 exceptions        : {offenders:?}");
    }
}
