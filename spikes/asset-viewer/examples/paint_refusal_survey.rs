//! How often a plain rectangular paint is refused on a real map, and why.
//!
//! The map editor's UI makes this the first thing a user meets: they drag a rectangle over a
//! shipped world map and the tool either paints it or says no. The CLI never made the *rate*
//! visible, because it paints once per process. This is the instrument behind the figure in
//! [map format](../../../docs/map-format.md#painting-a-shipped-world-map-is-refused-about-a-third-of-the-time),
//! so that number can be re-measured rather than believed.
//!
//! **No map or tileset is committed** -- both are proprietary -- so this takes the map and its
//! `.til` as arguments. Extract the tileset from `pic.mpq` first:
//!
//! ```sh
//! lom-asset-viewer --extract "$PIC_MPQ" 'til\tilesb01.til' /tmp/tilesb01.til --listfile "$LISTFILE"
//! cargo run --release --example paint_refusal_survey -- /path/to/map/URAK.scn /tmp/tilesb01.til 3
//! ```
//!
//! It plans, never applies and never writes: nothing here can reach a game directory.

use std::collections::BTreeMap;
use std::process::ExitCode;

use lom_asset_viewer::map::{MapAsset, PaintRefusal};
use lom_asset_viewer::tile::{TileSelector, TileSetDefinition};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.len() != 3 {
        eprintln!("usage: paint_refusal_survey MAP TILESET.til SIDE");
        return ExitCode::from(2);
    }
    let Ok(side) = arguments[2].parse::<u32>() else {
        eprintln!("SIDE must be a positive integer");
        return ExitCode::from(2);
    };
    let map = match std::fs::read(&arguments[0]).map_err(|error| error.to_string()).and_then(
        |bytes| MapAsset::parse(&bytes).map_err(|error| error.to_string()),
    ) {
        Ok(map) => map,
        Err(error) => {
            eprintln!("could not read {}: {error}", arguments[0]);
            return ExitCode::from(2);
        }
    };
    let tile_set = match std::fs::read(&arguments[1]).map_err(|error| error.to_string()).and_then(
        |bytes| TileSetDefinition::parse(&bytes).map_err(|error| error.to_string()),
    ) {
        Ok(tile_set) => tile_set,
        Err(error) => {
            eprintln!("could not read {}: {error}", arguments[1]);
            return ExitCode::from(2);
        }
    };

    // Every terrain the tileset actually draws, not the eleven-name world table: ids are
    // tileset-local.
    let mut terrains: Vec<u32> = tile_set
        .tiles
        .values()
        .map(|tile| tile.terrain_type)
        .collect();
    terrains.sort_unstable();
    terrains.dedup();

    println!("terrain\tattempted\tpainted\trefused\tdrawn-cells\tfirst-refusal");
    let mut totals = (0_usize, 0_usize, 0_usize, 0_usize);
    let mut refusal_kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    for terrain_type in terrains {
        let (mut attempted, mut painted, mut refused, mut drawn) = (0, 0, 0, 0);
        let mut first = String::new();
        // Every position the rectangle fits, on a stride of its own side, so the whole map is
        // covered once with no overlap.
        for y in (0..map.height.saturating_sub(side - 1)).step_by(side as usize) {
            for x in (0..map.width.saturating_sub(side - 1)).step_by(side as usize) {
                attempted += 1;
                match map.plan_terrain_paint(
                    (x, y, x + side - 1, y + side - 1),
                    terrain_type,
                    &tile_set,
                    TileSelector::LowestSlot,
                ) {
                    Ok(plan) => {
                        painted += 1;
                        drawn += plan.drawn_cells();
                    }
                    Err(refusal) => {
                        refused += 1;
                        *refusal_kinds.entry(kind(&refusal)).or_default() += 1;
                        if first.is_empty() {
                            first = refusal.to_string();
                        }
                    }
                }
            }
        }
        println!(
            "{terrain_type}\t{attempted}\t{painted}\t{refused}\t{drawn}\t{}",
            first.chars().take(90).collect::<String>()
        );
        totals.0 += attempted;
        totals.1 += painted;
        totals.2 += refused;
        totals.3 += drawn;
    }
    println!(
        "total\t{}\t{}\t{}\t{}",
        totals.0, totals.1, totals.2, totals.3
    );
    for (kind, count) in refusal_kinds {
        println!("refusal-kind\t{kind}\t{count}");
    }
    ExitCode::SUCCESS
}

fn kind(refusal: &PaintRefusal) -> &'static str {
    match refusal {
        PaintRefusal::NotARectangle { .. } => "not-a-rectangle",
        PaintRefusal::OutsideMap { .. } => "outside-map",
        PaintRefusal::BackgroundTileUnrecognised { .. } => "background-tile-unrecognised",
        PaintRefusal::NoMatchingTile { .. } => "no-matching-tile",
        PaintRefusal::TileSetUnknown => "tileset-unknown",
        PaintRefusal::TerrainTypeNotInTileSet { .. } => "terrain-type-not-in-tileset",
        PaintRefusal::RingTileOutsideTagField { .. } => "ring-tile-outside-tag-field",
    }
}
