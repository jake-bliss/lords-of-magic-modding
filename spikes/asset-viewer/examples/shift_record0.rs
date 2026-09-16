//! Throwaway: shift hotspot record 0 of EVERY frame in an IMP by a fixed delta.
//!
//! Shifting every frame makes the experiment frame-independent: whichever frame the engine draws,
//! it moves by the same amount.
use lom_asset_viewer::imp::{self, ImpSprite};

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("input imp");
    let output = args.next().expect("output imp");
    let dx: i16 = args.next().expect("dx").parse().expect("dx");
    let dy: i16 = args.next().expect("dy").parse().expect("dy");

    let mut bytes = std::fs::read(&input).expect("read input");
    let sprite = ImpSprite::parse(&bytes).expect("parse");

    // Distinct records only: aliased frames share bytes, and shifting twice would double the delta.
    let mut seen = std::collections::BTreeSet::new();
    let mut patched = 0;
    for index in 0..sprite.frames.len() {
        let frame = &sprite.frames[index];
        let Some(spot) = frame.hotspots.iter().find(|spot| spot.id == 0) else { continue };
        if !seen.insert(frame.record_offset) {
            continue;
        }
        let (x, y) = (spot.x.saturating_add(dx), spot.y.saturating_add(dy));
        bytes = imp::write_frame_hotspot(&bytes, index, 0, x, y).expect("write");
        patched += 1;
    }

    // Read back and confirm every record 0 moved by exactly the delta.
    let after = ImpSprite::parse(&bytes).expect("reparse");
    let mut checked = 0;
    for (before, now) in sprite.frames.iter().zip(&after.frames) {
        let (Some(b), Some(a)) = (
            before.hotspots.iter().find(|s| s.id == 0),
            now.hotspots.iter().find(|s| s.id == 0),
        ) else { continue };
        assert_eq!((a.x - b.x, a.y - b.y), (dx, dy), "frame moved by the wrong delta");
        checked += 1;
    }
    std::fs::write(&output, &bytes).expect("write output");
    println!("patched {patched} distinct records, verified {checked} frames, delta ({dx}, {dy})");
}
