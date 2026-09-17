//! Parse every `.til` in a directory and report what each declares.
//!
//! Reading the eight neighbour-constraint columns is a capability the parser gained on 2026-09-17,
//! and a parser that starts reading columns it used to discard can start rejecting files it used to
//! accept. `--view-map` and `--export-map-preview` need only a tile's slot and its `self` column, so
//! that would be a regression for anyone with a modded tileset. This is the check that it is not.
//!
//! Run against the 26 members of `pic.mpq`'s `til\` directory, extracted somewhere outside the
//! repository. **No tileset is committed** -- they are proprietary -- so this takes the directory as
//! an argument rather than embedding a fixture.
//!
//! ```sh
//! cargo run --release --example parse_all_tilesets -- /path/to/extracted/til
//! ```
//!
//! Result on the GS5R3 set, 2026-09-17: **26 parsed, 0 failed, 0 incomplete rows.** Terrain ids run
//! to 42 in `cavecry2.til` where `tilesb01.til` stops at 10, which is the concrete evidence that
//! terrain ids are **tileset-local** and none may be hardcoded as a global.

use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: parse_all_tilesets DIRECTORY");
        return ExitCode::from(2);
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("could not read {dir}");
        return ExitCode::from(2);
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("til"))
        })
        .collect();
    paths.sort();

    let (mut parsed, mut failed, mut incomplete_total) = (0_usize, 0_usize, 0_usize);
    for path in &paths {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                failed += 1;
                println!("FAIL {name:<14} could not read: {error}");
                continue;
            }
        };
        match lom_asset_viewer::tile::TileSetDefinition::parse(&bytes) {
            Ok(tile_set) => {
                parsed += 1;
                let incomplete = tile_set
                    .tiles
                    .values()
                    .filter(|tile| !tile.constraints_are_complete())
                    .count();
                incomplete_total += incomplete;
                let highest = tile_set.terrain_types.keys().copied().max().unwrap_or(0);
                println!(
                    "OK   {name:<14} {}x{} atlas {:<4} tiles {:<4} terrains {:<3} highest-id {:<3} incomplete {incomplete}",
                    tile_set.columns,
                    tile_set.rows,
                    tile_set.atlas_capacity(),
                    tile_set.tiles.len(),
                    tile_set.terrain_types.len(),
                    highest,
                );
            }
            Err(error) => {
                failed += 1;
                println!("FAIL {name:<14} {error}");
            }
        }
    }
    println!("\nparsed\t{parsed}");
    println!("failed\t{failed}");
    println!("incomplete-rows\t{incomplete_total}");
    if paths.is_empty() {
        eprintln!("no .til files in {dir} -- an empty run is not a pass");
        return ExitCode::from(2);
    }
    if failed > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}
