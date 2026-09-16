//! Throwaway: read an archive member by name, bypassing the listfile.
use lom_asset_viewer::imp::ImpSprite;
use lom_asset_viewer::mpq::Archive;

fn main() {
    let mut args = std::env::args().skip(1);
    let archive = Archive::open(std::path::Path::new(&args.next().expect("archive"))).expect("open");
    let name = args.next().expect("member");
    match archive.read(&name) {
        Err(error) => println!("{name}: READ FAILED: {error}"),
        Ok(bytes) => {
            print!("{name}: {} bytes", bytes.len());
            match ImpSprite::parse(&bytes) {
                Ok(sprite) => {
                    let f = &sprite.frames[0];
                    let spots: Vec<(u16, i16, i16)> =
                        f.hotspots.iter().map(|s| (s.id, s.x, s.y)).collect();
                    println!("  frame0 {}x{} hotspots {spots:?}", f.width, f.height);
                }
                Err(e) => println!("  (not a parseable IMP: {e})"),
            }
        }
    }
}
