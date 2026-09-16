//! Throwaway: compare how the origin field and each hotspot type relate to frame size.
use lom_asset_viewer::imp::ImpSprite;
use lom_asset_viewer::mpq::Archive;
use std::collections::BTreeMap;

fn stats(label: &str, rows: &[(f64, f64, f64)]) {
    // rows: (x, y, height)
    if rows.is_empty() {
        return;
    }
    let n = rows.len() as f64;
    let mean = |f: &dyn Fn(&(f64, f64, f64)) -> f64| rows.iter().map(f).sum::<f64>() / n;
    let mx = mean(&|r| r.0);
    let my = mean(&|r| r.1);
    let mh = mean(&|r| r.2);
    let var_h: f64 = rows.iter().map(|r| (r.2 - mh).powi(2)).sum::<f64>() / n;
    let cov: f64 = rows.iter().map(|r| (r.2 - mh) * (r.1 - my)).sum::<f64>() / n;
    let slope = if var_h > 0.0 { cov / var_h } else { 0.0 };
    let intercept = my - slope * mh;
    let resid: f64 = (rows
        .iter()
        .map(|r| (r.1 - (slope * r.2 + intercept)).powi(2))
        .sum::<f64>()
        / n)
        .sqrt();
    let raw: f64 = (rows.iter().map(|r| (r.1 - my).powi(2)).sum::<f64>() / n).sqrt();
    let mut xs: Vec<f64> = rows.iter().map(|r| r.0).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med_x = xs[xs.len() / 2];
    println!(
        "{label:<22} n={:<6} median_x={med_x:>5.1} mean_x={mx:>6.2} | y ~ {slope:.3}*h + {intercept:.2}  resid={resid:.2} of raw {raw:.2}",
        rows.len()
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let archive_path = args.next().expect("archive path");
    let listfile = args.next().expect("listfile");
    let archive = Archive::open(std::path::Path::new(&archive_path)).expect("open archive");
    let names: Vec<String> = std::fs::read_to_string(listfile)
        .expect("read listfile")
        .lines()
        .map(str::to_owned)
        .collect();

    let mut origin_rows = Vec::new();
    let mut by_id: BTreeMap<u16, Vec<(f64, f64, f64)>> = BTreeMap::new();
    for name in &names {
        if !name.to_ascii_lowercase().ends_with(".imp") {
            continue;
        }
        let Ok(bytes) = archive.read(name) else { continue };
        let Ok(sprite) = ImpSprite::parse(&bytes) else { continue };
        for frame in &sprite.frames {
            if frame.width == 0 || frame.height == 0 {
                continue;
            }
            let h = f64::from(frame.height);
            if !frame.hotspots.is_empty() {
                for spot in &frame.hotspots {
                    by_id
                        .entry(spot.id)
                        .or_default()
                        .push((f64::from(spot.x), f64::from(spot.y), h));
                }
            } else if let (Some(x), Some(y)) = (frame.origin_x, frame.origin_y) {
                origin_rows.push((f64::from(x), f64::from(y), h));
            }
        }
    }
    stats("origin field", &origin_rows);
    for (id, rows) in &by_id {
        if rows.len() >= 500 {
            stats(&format!("hotspot id {id}"), rows);
        }
    }
    // The decisive comparison: does -y/h cluster near 0.5 (centre-anchored)?
    let ratio = |rows: &[(f64, f64, f64)]| {
        let mut v: Vec<f64> = rows.iter().map(|r| -r.1 / r.2).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (v[v.len() / 4], v[v.len() / 2], v[3 * v.len() / 4])
    };
    // Refuted 2026-09-16: hotspot type 0 is NOT simply -(height >> 1). Two unit sprites matched
    // exactly, which looked like a rule; corpus-wide it holds for 1.7% of frames. Kept here so the
    // same wrong idea is cheap to re-test rather than re-derive.
    let exact = |rows: &[(f64, f64, f64)]| {
        let hits = rows
            .iter()
            .filter(|r| (r.1 + (r.2 as i64 >> 1) as f64).abs() < f64::EPSILON)
            .count();
        100.0 * hits as f64 / rows.len() as f64
    };
    if let Some(rows) = by_id.get(&0) {
        println!("\nhotspot id 0 with y == -(height >> 1): {:.1}% (refuted as a rule)", exact(rows));
    }

    println!("\n-y/height quartiles (0.50 == centred on the anchor)");
    println!("  origin field : {:?}", ratio(&origin_rows));
    for (id, rows) in &by_id {
        if rows.len() >= 500 {
            println!("  hotspot id {id:<3}: {:?}", ratio(rows));
        }
    }
}
