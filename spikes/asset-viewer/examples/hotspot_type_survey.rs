//! Throwaway: raw hotspot bytes for the two out-of-vocabulary files, beside a normal one.
use lom_asset_viewer::imp::ImpSprite;
use lom_asset_viewer::mpq::Archive;

fn main() {
    let mut args = std::env::args().skip(1);
    let archive = Archive::open(std::path::Path::new(&args.next().expect("archive"))).expect("open");
    for name in ["units\\imp\\aicr2a.imp", "units\\imp\\eacr5a.imp", "units\\imp\\aiwm1b.imp"] {
        let Ok(bytes) = archive.read(name) else { continue };
        let Ok(sprite) = ImpSprite::parse(&bytes) else { continue };
        println!("\n=== {name}  file_flags=0x{:02x} variant={} bpp={}",
                 sprite.file_flags, sprite.record_variant, sprite.bits_per_pixel);
        for index in 0..3.min(sprite.frames.len()) {
            let frame = &sprite.frames[index];
            let Some(base) = frame.hotspot_offset else { continue };
            let count = frame.hotspots.len();
            println!("  frame {index}: record@0x{:x} count={count} array@0x{base:x}",
                     frame.record_offset);
            print!("    record bytes : ");
            for b in &bytes[frame.record_offset..frame.record_offset + 16] {
                print!("{b:02x} ");
            }
            println!();
            print!("    array  bytes : ");
            let span = (count * 6 + 7) & !7;
            for b in &bytes[base..(base + span).min(bytes.len())] {
                print!("{b:02x} ");
            }
            println!();
            for spot in &frame.hotspots {
                println!("      id={:<4} ({:>5}, {:>5})  raw={:02x?}", spot.id, spot.x, spot.y, spot.raw);
            }
        }
    }
}
