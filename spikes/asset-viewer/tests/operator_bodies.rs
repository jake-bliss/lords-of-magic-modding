//! Regression anchors for the operator-body analysis.
//!
//! Two kinds of test live here, and they are deliberately different.
//!
//! The first kind reads the **committed table** and asserts *relations* between operators that were
//! established separately, by writing map files and watching the engine read them: that the two map
//! writers share a writer, that the operators which act on the map name one common object, that the
//! operators which only shuffle the operand stack touch no engine state. None of these restates a
//! number the analyser produced; each would fail if the analyser started attributing calls or
//! globals to the wrong operator, and each runs without the proprietary binary.
//!
//! The second kind runs only when `LOM_EXE` points at a copy of `lomse.exe`. It re-derives the
//! table and requires the committed artifact to still be what the binary says, so a change to the
//! analyser cannot quietly leave stale findings in the repository.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the crate sits two directories below the repository root")
}

fn artifact_path() -> PathBuf {
    repository_root().join("reports/natives/operator-bodies.tsv")
}

/// The committed table, as a row per operator keyed by name.
fn table() -> BTreeMap<String, BTreeMap<String, String>> {
    let text = std::fs::read_to_string(artifact_path()).expect("the classification table is committed");
    let mut lines = text.lines();
    let columns: Vec<&str> = lines.next().expect("the table has a header").split('\t').collect();
    let mut rows = BTreeMap::new();
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            fields.len(),
            columns.len(),
            "row {:?} has {} fields against {} columns",
            fields.first(),
            fields.len(),
            columns.len()
        );
        let row: BTreeMap<String, String> = columns
            .iter()
            .zip(fields.iter())
            .map(|(column, field)| ((*column).to_owned(), (*field).to_owned()))
            .collect();
        rows.insert(row["name"].clone(), row);
    }
    rows
}

fn list(row: &BTreeMap<String, String>, column: &str) -> BTreeSet<String> {
    let value = &row[column];
    if value == "-" {
        return BTreeSet::new();
    }
    value.split(',').map(str::to_owned).collect()
}

/// `savescenariomap` and `savespecialmap` write byte-identical files — established by writing both
/// and comparing them, not by reading the binary. If that is true they cannot each have their own
/// serialiser, and the call graph should show them sharing one.
#[test]
fn the_two_map_writers_share_a_writer() {
    let table = table();
    let scenario = list(&table["savescenariomap"], "call_targets");
    let special = list(&table["savespecialmap"], "call_targets");
    let shared: Vec<&String> = scenario.intersection(&special).collect();
    assert!(
        !shared.is_empty(),
        "the two map writers produce identical bytes but share no callee: {scenario:?} against {special:?}"
    );
}

/// Every operator that acts on the map should reach the same engine object.
///
/// The membership of this list comes from outside the binary: these are the operators whose effect
/// on a map file has been observed. The test is that the analysis puts them on one object — it does
/// not say which, so it still fails if the analyser starts mixing operators' references together
/// but passes if the engine is rebuilt at different addresses.
#[test]
fn the_map_operators_name_one_common_object() {
    let table = table();
    let map_operators = [
        "anythingat",
        "armyat",
        "buildingat",
        "cityat",
        "resetvisibility",
        "setterrain",
        "terrainspriteat",
    ];
    let mut common: Option<BTreeSet<String>> = None;
    for name in map_operators {
        let globals = list(&table[name], "global_addresses");
        assert!(!globals.is_empty(), "{name} references no engine state at all");
        common = Some(match common {
            None => globals,
            Some(previous) => previous.intersection(&globals).cloned().collect(),
        });
    }
    assert_eq!(
        common.expect("the list is not empty").len(),
        1,
        "the map operators should agree on exactly one object"
    );
}

/// The operand-stack primitives do their work on the interpreter context and nothing else.
#[test]
fn the_stack_primitives_touch_no_engine_state() {
    let table = table();
    for name in ["dup", "exch", "pop", "roll", "index"] {
        let Some(row) = table.get(name) else {
            continue;
        };
        assert_eq!(
            row["global_addresses"], "-",
            "{name} is a stack primitive but references engine state"
        );
    }
}

/// The count recovered by walking a body must be at least the count already recorded, because the
/// older measurement is a site count that misses operands fetched through a helper — a weakness its
/// own module documents. Anything *below* it is this analysis losing a path.
///
/// Three operators break the rule and they are named here rather than excused: `button`,
/// `setunitdata` and `nsetunitdata` are the operators whose bodies pop a different number of
/// operands on different branches, where a site count is the higher number by construction.
#[test]
fn the_body_walk_never_undercounts_the_site_count() {
    let table = table();
    let known_branching: BTreeSet<&str> = ["button", "setunitdata", "nsetunitdata"].into();
    let mut undercounts = Vec::new();
    for (name, row) in &table {
        let (Ok(nominal), Ok(declared)) = (
            row["nominal_arity"].parse::<usize>(),
            row["declared_arity"].parse::<usize>(),
        ) else {
            continue;
        };
        if nominal < declared && !known_branching.contains(name.as_str()) {
            undercounts.push(format!("{name}: {nominal} against {declared}"));
        }
    }
    assert!(
        undercounts.is_empty(),
        "the body walk lost operands for: {undercounts:?}"
    );
}

/// Coverage, stated as a rate rather than a per-row verdict: the walk must finish for nearly every
/// operator, or nothing else in the table can be trusted.
#[test]
fn the_boundary_walk_completes_for_nearly_every_operator() {
    let table = table();
    let complete = table
        .values()
        .filter(|row| row["boundary"] == "complete")
        .count();
    assert!(
        complete * 100 >= table.len() * 99,
        "only {complete} of {} bodies walked to completion",
        table.len()
    );
}

/// An operand count must have somewhere to have come from.
///
/// An operator credited with operands must either pop them itself, call one of the recognised
/// shared helpers, or call *something* whose own body pops. Twenty-one operators take the third
/// route through a per-subsystem wrapper that is too rarely called to be recognised as a shared
/// helper, which is why the test asks for a mechanism rather than for a helper: requiring a helper
/// declared those twenty-one broken when they are merely fetching through their own wrapper.
#[test]
fn an_operand_count_has_a_mechanism_behind_it() {
    let table = table();
    let mut invented = Vec::new();
    for (name, row) in &table {
        let inline: usize = row["inline_pops"].parse().unwrap_or(0);
        let helper: usize = row["helper_pops"].parse().unwrap_or(0);
        let calls: usize = row["calls"].parse().unwrap_or(0);
        let nominal: usize = row["nominal_arity"].parse().unwrap_or(0);
        if nominal > 0 && inline + helper + calls == 0 {
            invented.push(name.clone());
        }
    }
    assert!(
        invented.is_empty(),
        "operands appear from nowhere in: {invented:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Against the binary, when there is one.
// ---------------------------------------------------------------------------------------------

fn executable() -> Option<Vec<u8>> {
    let path = std::env::var_os("LOM_EXE")?;
    std::fs::read(path).ok()
}

/// The committed table still says what the binary says.
#[test]
fn the_committed_table_matches_the_binary() {
    let Some(bytes) = executable() else {
        eprintln!("skipped: set LOM_EXE to a copy of lomse.exe to check the artifact is current");
        return;
    };
    use lom_asset_viewer::native_table::{self, PeImage};
    use lom_asset_viewer::operator_bodies;

    let image = PeImage::parse(&bytes).expect("the executable parses");
    let runs = native_table::extract(&bytes).expect("the operator tables are recoverable");
    let mut seen = BTreeSet::new();
    let mut operators = Vec::new();
    for entry in runs.iter().flat_map(|run| run.entries.iter()) {
        let name = entry.name.to_ascii_lowercase();
        if seen.insert(name.clone()) {
            operators.push((name, entry.entry_point));
        }
    }
    let analysis = operator_bodies::analyse(&image, &operators).expect("every entry point walks");
    let table = table();
    assert_eq!(
        analysis.reports.len(),
        table.len(),
        "the committed table has a different number of operators than the binary does"
    );
    let mut differences = Vec::new();
    for report in &analysis.reports {
        let row = &table[&report.name];
        let expected = format!("{:#010x}", report.entry_point);
        if row["entry_point"] != expected {
            differences.push(format!("{}: entry point", report.name));
        }
        if row["behaviour"] != report.behaviour.label() {
            differences.push(format!("{}: behaviour", report.name));
        }
        let nominal = report
            .body
            .nominal_arity()
            .map_or_else(|| "-".to_owned(), |value| value.to_string());
        if row["nominal_arity"] != nominal {
            differences.push(format!("{}: arity", report.name));
        }
    }
    assert!(
        differences.is_empty(),
        "the committed table is stale; regenerate it with the operator_bodies example: {:?}",
        &differences[..differences.len().min(20)]
    );
}

/// The shared operand-fetch helpers are found by shape, so finding them is itself a claim about the
/// binary: the engine has more than one, and together they account for most operand traffic.
#[test]
fn the_operand_helpers_are_discovered_by_shape() {
    let Some(bytes) = executable() else {
        eprintln!("skipped: set LOM_EXE to a copy of lomse.exe");
        return;
    };
    use lom_asset_viewer::native_table::{self, PeImage};
    use lom_asset_viewer::operator_bodies::ProgramIndex;

    let image = PeImage::parse(&bytes).expect("the executable parses");
    let runs = native_table::extract(&bytes).expect("the operator tables are recoverable");
    let entry_points: Vec<u32> = runs
        .iter()
        .flat_map(|run| run.entries.iter())
        .map(|entry| entry.entry_point)
        .collect();
    let index = ProgramIndex::build(&image, &entry_points).expect("the index builds");
    assert!(
        index.pop_helpers().len() > 1,
        "the engine fetches operands through more than one helper; finding only {:?} means the \
         search collapsed back to the single-helper assumption that made a third of the table look \
         nullary",
        index.pop_helpers()
    );
}
