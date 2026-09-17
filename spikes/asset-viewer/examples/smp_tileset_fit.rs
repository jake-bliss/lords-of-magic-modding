//! Score the installed map corpus against the tileset-resolution rule.
//!
//! **This is the instrument the class-based rule needed and did not have.** The previous version of
//! this project resolved every `.smp` to `tilesa01.til` from a hardcoded constant. Every test
//! passed, four independent mutations all killed tests, two reviewers reproduced every published
//! percentage -- and the rule was wrong for the large majority of the corpus. Nothing in the suite
//! could fail on *the rule being wrong*, only on the code disagreeing with the constant. This
//! program is what closes that: it reads the real maps and the real tilesets and reports whether
//! the resolution actually explains them.
//!
//! It **exits non-zero** when it scores nothing, when a map class is missing entirely, or when the
//! measured satisfaction falls below a floor. An instrument that reports success on no data is the
//! failure mode this repository has already been bitten by -- an empty result is a failed
//! measurement, not a clean one.
//!
//! No map and no tileset is committed -- both are proprietary -- so both directories are arguments.
//!
//! ```sh
//! cargo run --release --example smp_tileset_fit -- /path/to/English/map /path/to/extracted/til
//! ```
//!
//! Result on the GS5R3 corpus, 2026-09-17: see `docs/map-format.md`. Headlines are that the
//! gamescript-declared tileset satisfies **95.09%** of the cells of the 169 bound combat maps
//! against **8.43%** for `tilesa01.til`, and that 168 combat maps have no binding and are reported
//! as unresolved rather than scored against a guess.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::ExitCode;

use lom_asset_viewer::map::{MapAsset, MapCell};
use lom_asset_viewer::tile::{
    MapClass, Neighbourhood, TileSetDefinition, TileSetResolution, resolve_tileset,
};

/// The satisfaction floor each scored group must clear.
///
/// Set well below the measured 95.09% and 93.13%, so ordinary corpus variation does not trip it,
/// but far above the 8.43% a wrong resolution produces. **This is the assertion that would have
/// caught the class-based rule**, and the number is chosen to make that concrete rather than to be
/// tight.
const SATISFACTION_FLOOR: f64 = 60.0;

#[derive(Default)]
struct Fit {
    maps: usize,
    cells: usize,
    undeclared: usize,
    /// Satisfied with an off-map neighbour read as the cell's own terrain.
    satisfied: usize,
    /// Satisfied with an off-map neighbour satisfying every constraint.
    ///
    /// Both readings are reported because this project's published `.scn` control -- 5.9% of cells
    /// violating, so 94.11% satisfying -- is the **open** one, and a program that printed only the
    /// closed figure would leave that control unreproducible from the committed instrument.
    satisfied_open: usize,
    scored: usize,
}

impl Fit {
    fn percent(&self) -> f64 {
        Self::ratio(self.satisfied, self.scored)
    }

    fn percent_open(&self) -> f64 {
        Self::ratio(self.satisfied_open, self.scored)
    }

    fn ratio(part: usize, whole: usize) -> f64 {
        if whole == 0 {
            0.0
        } else {
            part as f64 * 100.0 / whole as f64
        }
    }
}

/// Score one map against one tileset, reading a cell's terrain from the tileset's `self` column.
///
/// Off-map neighbours are read as the cell's own terrain -- the closed-edge reading
/// [`Neighbourhood::closed_with`] uses. The open reading differs by well under a point and neither
/// changes any conclusion here.
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
            let open = Neighbourhood::from_lookup(|dx, dy| {
                terrain_at(i64::from(x) + i64::from(dx), i64::from(y) + i64::from(dy))
            });
            if tile.accepts(&open.closed_with(tile.terrain_type)) {
                fit.satisfied += 1;
            }
            if tile.accepts(&open) {
                fit.satisfied_open += 1;
            }
        }
    }
}

fn load_tile_sets(dir: &str) -> Result<BTreeMap<String, TileSetDefinition>, String> {
    let entries = std::fs::read_dir(dir).map_err(|error| format!("{dir}: {error}"))?;
    let mut tile_sets = BTreeMap::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("til"))
        {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let tile_set = TileSetDefinition::parse(&bytes)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        tile_sets.insert(name, tile_set);
    }
    if tile_sets.is_empty() {
        return Err(format!("{dir} contains no .til files"));
    }
    Ok(tile_sets)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(map_dir), Some(til_dir)) = (args.next(), args.next()) else {
        eprintln!("usage: smp_tileset_fit MAP_DIRECTORY TIL_DIRECTORY");
        return ExitCode::from(2);
    };

    let tile_sets = match load_tile_sets(&til_dir) {
        Ok(tile_sets) => tile_sets,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };

    let entries = match std::fs::read_dir(&map_dir) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("error: {map_dir}: {error}");
            return ExitCode::from(2);
        }
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && MapClass::from_path(path).is_some())
        .collect();
    paths.sort();

    // Per **extension**, not per class, so the `.scn`-only control this project's "not a broken
    // scorer" argument rests on is printed rather than asserted in prose. Pooling `.scn` with
    // `.lgd` was how that control went missing.
    let mut by_extension: BTreeMap<String, Fit> = BTreeMap::new();
    let mut bound_combat = Fit::default();
    let mut generated_combat = Fit::default();
    let mut ambiguous_spread: Vec<(String, f64, f64)> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    let mut missing_tilesets: BTreeSet<String> = BTreeSet::new();
    let mut unreadable: Vec<String> = Vec::new();
    let mut combat_slots: BTreeSet<u32> = BTreeSet::new();
    let mut atlas_buckets: BTreeMap<u32, usize> = BTreeMap::new();
    // Does the filename predict the gamescript's own pairing? This is the measurement that decides
    // whether "the name picks the tileset" is a mechanism or a coincidence, and it is committed
    // here rather than asserted in prose because it was asserted in prose once and was wrong.
    let (mut faith_prefixed, mut faith_name_agrees, mut faith_declares_all) = (0_usize, 0, 0);

    for path in &paths {
        let Some(class) = MapClass::from_path(path) else {
            continue;
        };
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let Ok(bytes) = std::fs::read(path) else {
            unreadable.push(name);
            continue;
        };
        let Ok(map) = MapAsset::parse(&bytes) else {
            unreadable.push(name);
            continue;
        };

        let resolution = resolve_tileset(path).unwrap_or(TileSetResolution::CombatUnresolved);

        if class == MapClass::Combat {
            combat_slots.extend(map.cells.iter().map(MapCell::tile_index));
            let highest = map.cells.iter().map(MapCell::tile_index).max().unwrap_or(0);
            let bucket = [64_u32, 128, 256, 624]
                .into_iter()
                .find(|capacity| highest < *capacity)
                .unwrap_or(624);
            *atlas_buckets.entry(bucket).or_default() += 1;
        }

        let candidates = resolution.candidates();
        if candidates.is_empty() {
            unresolved.push(name.clone());
        }

        if class == MapClass::Combat {
            let lower = name.to_ascii_lowercase();
            const FAITHS: [&str; 8] = ["ai", "ch", "de", "ea", "fi", "li", "or", "wa"];
            if let Some(faith) = FAITHS.iter().find(|faith| lower.starts_with(**faith)) {
                let guess = format!("{faith}bldg01.til");
                faith_prefixed += 1;
                if candidates.len() == 1 && candidates[0] == guess {
                    faith_name_agrees += 1;
                }
                // The weaker claim that made the name rule look right: does it merely *declare*
                // every slot the map uses, whether or not the scripts pair them?
                if let Some(tile_set) = tile_sets.get(&guess)
                    && map
                        .cells
                        .iter()
                        .all(|cell| tile_set.tiles.contains_key(&cell.tile_index()))
                {
                    faith_declares_all += 1;
                }
            }
        }

        // Score every candidate; the best is what the group figure uses, and for an ambiguous map
        // the spread between best and worst is itself reported.
        let mut scores: Vec<(f64, &str, Fit)> = Vec::new();
        for candidate in candidates {
            let Some(tile_set) = tile_sets.get(*candidate) else {
                missing_tilesets.insert((*candidate).to_owned());
                continue;
            };
            let mut fit = Fit::default();
            score(&map, tile_set, &mut fit);
            scores.push((fit.percent(), candidate, fit));
        }
        if scores.is_empty() {
            continue;
        }
        scores.sort_by(|left, right| {
            right
                .0
                .partial_cmp(&left.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if scores.len() > 1 {
            let worst = scores.last().expect("scores is non-empty").0;
            ambiguous_spread.push((name.clone(), scores[0].0, worst));
        }
        let best = &scores[0].2;
        let entry = by_extension.entry(extension).or_default();
        entry.maps += 1;
        entry.cells += best.cells;
        entry.undeclared += best.undeclared;
        entry.satisfied += best.satisfied;
        entry.satisfied_open += best.satisfied_open;
        entry.scored += best.scored;

        if class == MapClass::Combat {
            bound_combat.maps += 1;
            bound_combat.cells += best.cells;
            bound_combat.undeclared += best.undeclared;
            bound_combat.satisfied += best.satisfied;
            bound_combat.satisfied_open += best.satisfied_open;
            bound_combat.scored += best.scored;
            // The control: the same map through the tileset the retracted rule recommended.
            if let Some(tile_set) = tile_sets.get("tilesa01.til") {
                score(&map, tile_set, &mut generated_combat);
            }
        }
    }

    println!("maps-found\t{}", paths.len());
    for (extension, fit) in &by_extension {
        println!(
            ".{extension}\tmaps:{}\tcells:{}\tundeclared:{} ({:.2}%)\tsatisfied:{}/{} ({:.2}%)",
            fit.maps,
            fit.cells,
            fit.undeclared,
            if fit.cells == 0 {
                0.0
            } else {
                fit.undeclared as f64 * 100.0 / fit.cells as f64
            },
            fit.satisfied,
            fit.scored,
            fit.percent(),
        );
        println!(
            ".{extension}-open-edge\tsatisfied:{}/{} ({:.2}%)",
            fit.satisfied_open,
            fit.scored,
            fit.percent_open(),
        );
    }
    println!(
        "combat-bound\tmaps:{}\tsatisfied:{}/{} ({:.2}%)",
        bound_combat.maps,
        bound_combat.satisfied,
        bound_combat.scored,
        bound_combat.percent(),
    );
    println!(
        "combat-through-tilesa01\tmaps:{}\tsatisfied:{}/{} ({:.2}%)\t(the retracted rule, for \
         comparison)",
        generated_combat.maps,
        generated_combat.satisfied,
        generated_combat.scored,
        generated_combat.percent(),
    );
    println!(
        "combat-faith-prefixed\t{faith_prefixed}\tname-rule-matches-the-script-pairing:\
         {faith_name_agrees}\tname-rule-merely-declares-every-slot:{faith_declares_all}"
    );
    println!("combat-unresolved\t{}", unresolved.len());
    println!("combat-ambiguous\t{}", ambiguous_spread.len());
    for (name, best, worst) in &ambiguous_spread {
        println!("  ambiguous\t{name}\tbest:{best:.2}%\tworst:{worst:.2}%");
    }
    if let (Some(lowest), Some(highest)) = (combat_slots.first(), combat_slots.last()) {
        println!(
            "combat-slot-range\t{lowest}..={highest}\tdistinct:{}",
            combat_slots.len()
        );
    }
    for (capacity, count) in &atlas_buckets {
        println!("combat-atlas-bucket\thighest-slot-under-{capacity}\t{count}");
    }
    if !unreadable.is_empty() {
        println!("unreadable\t{}\t{}", unreadable.len(), unreadable.join(" "));
    }
    if !missing_tilesets.is_empty() {
        println!(
            "tilesets-referenced-but-absent\t{}",
            missing_tilesets.iter().cloned().collect::<Vec<_>>().join(" ")
        );
    }

    // --- the checks that make this an instrument rather than a report ----------------------

    let mut failures: Vec<String> = Vec::new();
    if by_extension.is_empty() {
        failures.push(format!(
            "scored no maps at all in {map_dir}; an empty result is a failed measurement, not a \
             clean one"
        ));
    }
    for required in ["smp", "scn"] {
        if !by_extension.contains_key(required) {
            failures.push(format!(
                "no .{required} map was scored; this corpus is missing a map class entirely, so \
                 the comparison this program exists to make cannot be made"
            ));
        }
    }
    for (extension, fit) in &by_extension {
        if fit.scored == 0 {
            failures.push(format!(".{extension} contributed no scored cell"));
        } else if fit.percent() < SATISFACTION_FLOOR {
            failures.push(format!(
                ".{extension} satisfies only {:.2}% of its cells against the resolved tileset, \
                 below the {SATISFACTION_FLOOR:.0}% floor -- the resolution rule does not explain \
                 this corpus",
                fit.percent(),
            ));
        }
    }
    if !missing_tilesets.is_empty() {
        eprintln!(
            "note: the gamescript names {} tileset(s) this directory does not contain, and maps \
             bound only to those were skipped",
            missing_tilesets.len()
        );
    }

    if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        for failure in &failures {
            eprintln!("FAIL: {failure}");
        }
        ExitCode::FAILURE
    }
}
