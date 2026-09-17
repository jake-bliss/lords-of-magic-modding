//! Measure how the installed maps fit the tileset the engine reads them through.
//!
//! **Which tileset a `.smp` uses is not decided by this program.** It is decided by `START.GS`,
//! which sets `combattileset` to `til/tilesa01.til` once at startup and never again -- see
//! `docs/map-format.md`. This exists so the *consequences* of that fact stay re-runnable: that
//! `tilesa01.til` declares every slot the combat maps use, that its neighbour constraints
//! nonetheless describe almost none of them, and that the world maps measured by the same code
//! come out at the figure this repository published independently.
//!
//! That last number is the control, and it is the reason to trust the first two. A scorer that
//! reported 8% on `.smp` and *also* 8% on `.scn` would be a broken scorer; one that reproduces the
//! 5.9% `.scn` violation rate already recorded here is measuring the corpus.
//!
//! No map and no tileset is committed -- both are proprietary -- so both directories are
//! arguments.
//!
//! ```sh
//! cargo run --release --example smp_tileset_fit -- /path/to/English/map /path/to/extracted/til
//! ```
//!
//! Result on the GS5R3 corpus, 2026-09-17:
//!
//! ```text
//! combat   337 maps  778,240 cells  tilesa01.til  undeclared 0 (0.00%)  satisfied 63,120 (8.11%)
//! world     29 maps  ...            tilesb01.til  undeclared 0 (0.00%)  satisfied ... (94.11% on .scn)
//! tilesa01 vs tilesb01 over the 308 slots .smp uses: 0 self, 0 constraint disagreements
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lom_asset_viewer::map::MapAsset;
use lom_asset_viewer::tile::{
    Direction, MapClass, Neighbourhood, TileSetDefinition, engine_tileset_member,
};

struct Fit {
    maps: usize,
    cells: usize,
    undeclared: usize,
    satisfied: usize,
    scored: usize,
}

impl Fit {
    const fn new() -> Self {
        Self {
            maps: 0,
            cells: 0,
            undeclared: 0,
            satisfied: 0,
            scored: 0,
        }
    }
}

/// Score one map against one tileset, reading a tile's terrain from the tileset's `self` column
/// and an off-map neighbour as satisfying every constraint -- the same open-edge reading
/// `NeighbourConstraint::accepts` uses.
fn score(map: &MapAsset, tile_set: &TileSetDefinition, fit: &mut Fit) {
    fit.maps += 1;
    fit.cells += map.cells.len();
    let terrain_at = |x: i64, y: i64| -> Option<u32> {
        if x < 0 || y < 0 || x >= i64::from(map.width) || y >= i64::from(map.height) {
            return None;
        }
        let cell = map.cell(x as u32, y as u32)?;
        tile_set.terrain_type_of_tile(cell.tile_index())
    };
    for y in 0..map.height {
        for x in 0..map.width {
            let Some(cell) = map.cell(x, y) else { continue };
            let Some(tile) = tile_set.tiles.get(&cell.tile_index()) else {
                fit.undeclared += 1;
                continue;
            };
            if !tile.constraints_are_complete() {
                continue;
            }
            fit.scored += 1;
            let neighbours = Neighbourhood::from_lookup(|dx, dy| {
                terrain_at(i64::from(x) + i64::from(dx), i64::from(y) + i64::from(dy))
            });
            if tile.accepts(&neighbours) {
                fit.satisfied += 1;
            }
        }
    }
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 * 100.0 / whole as f64
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(map_dir), Some(til_dir)) = (args.next(), args.next()) else {
        eprintln!("usage: smp_tileset_fit MAP_DIRECTORY TIL_DIRECTORY");
        return ExitCode::from(2);
    };

    let mut tile_sets: BTreeMap<String, TileSetDefinition> = BTreeMap::new();
    for class in [MapClass::Combat, MapClass::World] {
        let member = engine_tileset_member(class);
        let path = Path::new(&til_dir).join(member);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("could not read {}: {error}", path.display());
                return ExitCode::from(2);
            }
        };
        match TileSetDefinition::parse(&bytes) {
            Ok(tile_set) => {
                tile_sets.insert(member.to_owned(), tile_set);
            }
            Err(error) => {
                eprintln!("could not parse {member}: {error}");
                return ExitCode::from(2);
            }
        }
    }

    let Ok(entries) = std::fs::read_dir(&map_dir) else {
        eprintln!("could not read {map_dir}");
        return ExitCode::from(2);
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && MapClass::from_path(path).is_some())
        .collect();
    paths.sort();

    let mut fits: BTreeMap<MapClass, Fit> = BTreeMap::new();
    // Every slot the combat maps actually use, which is the range over which the two world
    // tilesets have to be told apart.
    let mut combat_slots: BTreeSet<u32> = BTreeSet::new();
    let mut unreadable = 0_usize;

    for path in &paths {
        let Some(class) = MapClass::from_path(path) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(path) else {
            unreadable += 1;
            continue;
        };
        let Ok(map) = MapAsset::parse(&bytes) else {
            unreadable += 1;
            continue;
        };
        if class == MapClass::Combat {
            combat_slots.extend(map.cells.iter().map(|cell| cell.tile_index()));
        }
        let tile_set = &tile_sets[engine_tileset_member(class)];
        score(&map, tile_set, fits.entry(class).or_insert_with(Fit::new));
    }

    println!("maps\t{}\tunreadable\t{unreadable}", paths.len());
    for (class, fit) in &fits {
        println!(
            "{}\tmaps:{}\tcells:{}\ttileset:{}\tundeclared:{} ({:.2}%)\tsatisfied:{}/{} ({:.2}%)",
            class.description().replace(' ', "-"),
            fit.maps,
            fit.cells,
            engine_tileset_member(*class),
            fit.undeclared,
            percent(fit.undeclared, fit.cells),
            fit.satisfied,
            fit.scored,
            percent(fit.satisfied, fit.scored),
        );
    }

    // The two 624-slot tilesets, compared only over the slots the combat maps reach. If they agree
    // everywhere there, no measurement over `.smp` cells can prefer one, and the gamescript is the
    // only thing that can.
    let combat = &tile_sets["tilesa01.til"];
    let world = &tile_sets["tilesb01.til"];
    let (mut self_diff, mut constraint_diff, mut only_one_declares) = (0_usize, 0_usize, 0_usize);
    for slot in &combat_slots {
        match (combat.tiles.get(slot), world.tiles.get(slot)) {
            (Some(left), Some(right)) => {
                if left.terrain_type != right.terrain_type {
                    self_diff += 1;
                }
                if Direction::ALL
                    .iter()
                    .any(|direction| left.neighbour(*direction) != right.neighbour(*direction))
                {
                    constraint_diff += 1;
                }
            }
            (None, None) => {}
            _ => only_one_declares += 1,
        }
    }
    println!(
        "tilesa01-vs-tilesb01\tslots-used-by-combat-maps:{}\tself-disagreements:{self_diff}\t\
         constraint-disagreements:{constraint_diff}\tdeclared-by-only-one:{only_one_declares}",
        combat_slots.len(),
    );
    if let (Some(lowest), Some(highest)) = (combat_slots.first(), combat_slots.last()) {
        println!("combat-slot-range\t{lowest}..={highest}");
    }

    ExitCode::SUCCESS
}
