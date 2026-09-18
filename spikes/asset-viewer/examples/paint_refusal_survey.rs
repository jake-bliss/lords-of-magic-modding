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
    if arguments.len() != 3 && arguments.len() != 4 {
        eprintln!("usage: paint_refusal_survey MAP TILESET.til SIDE [STRIDE]");
        return ExitCode::from(2);
    }
    // Zero is rejected here rather than later: `side - 1` underflows and `step_by(0)` panics, so
    // the promised validation error used to be a crash.
    let side = match arguments[2].parse::<u32>() {
        Ok(side) if side > 0 => side,
        _ => {
            eprintln!("SIDE must be a positive integer");
            return ExitCode::from(2);
        }
    };
    let stride = match arguments.get(3).map(|value| value.parse::<u32>()) {
        None => 1,
        Some(Ok(stride)) if stride > 0 => stride,
        Some(_) => {
            eprintln!("STRIDE must be a positive integer");
            return ExitCode::from(2);
        }
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

    // **Stride 1 by default: every position the rectangle legally fits.** An earlier version
    // stepped by the rectangle's own side and so tested a disjoint lattice -- 42x42 origins out of
    // the 126x126 a 3x3 actually fits on a 128-wide map -- while the figure it produced was
    // published as a rate over *all* paints. A sample reported as a population is the exact error
    // this repository has already paid for. A larger stride is still available as an argument, and
    // the final position is always flush with each far edge, because the map edge is where the
    // off-map-neighbour assumption applies and no saved artifact tests it.
    let positions = |extent: u32| -> Vec<u32> {
        if extent < side {
            return Vec::new();
        }
        let last = extent - side;
        let mut starts: Vec<u32> = (0..=last).step_by(stride as usize).collect();
        if starts.last() != Some(&last) {
            starts.push(last);
        }
        starts
    };
    let columns = positions(map.width);
    let rows = positions(map.height);
    if columns.is_empty() || rows.is_empty() {
        eprintln!(
            "a {side}x{side} rectangle does not fit on this {}x{} map",
            map.width, map.height
        );
        return ExitCode::from(2);
    }
    eprintln!(
        "sampling {} x {} = {} origins with stride {stride}; every position a {side}x{side} \
         rectangle fits is {} x {} = {}",
        columns.len(),
        rows.len(),
        columns.len() * rows.len(),
        map.width - side + 1,
        map.height - side + 1,
        (map.width - side + 1) as usize * (map.height - side + 1) as usize,
    );

    println!("terrain\tattempted\tpainted\trefused\tdrawn-cells\tdrawn-per-paint\tfirst-refusal");
    let mut totals = (0_usize, 0_usize, 0_usize, 0_usize);
    let mut refusal_kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    for terrain_type in terrains {
        let (mut attempted, mut painted, mut refused, mut drawn) = (0, 0, 0, 0);
        let mut first = String::new();
        for &y in &rows {
            for &x in &columns {
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
        // Drawn cells per **accepted** paint. The raw drawn count answers a different question --
        // a terrain that is refused everywhere accumulates almost none of them -- and dividing by
        // attempts would mix the two.
        let per_paint = if painted == 0 {
            "n/a".to_owned()
        } else {
            format!("{:.2}", drawn as f64 / painted as f64)
        };
        println!(
            "{terrain_type}\t{attempted}\t{painted}\t{refused}\t{drawn}\t{per_paint}\t{}",
            first.chars().take(90).collect::<String>()
        );
        totals.0 += attempted;
        totals.1 += painted;
        totals.2 += refused;
        totals.3 += drawn;
    }
    println!(
        "total\t{}\t{}\t{}\t{}\t{}",
        totals.0,
        totals.1,
        totals.2,
        totals.3,
        if totals.1 == 0 {
            "n/a".to_owned()
        } else {
            format!("{:.2}", totals.3 as f64 / totals.1 as f64)
        },
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
