//! Regression anchors for the operator-body analysis.
//!
//! Three kinds of test live here and they are deliberately different.
//!
//! **Relations read off the committed table.** That the two map writers share a writer, that the
//! operators which act on the map name one common object, that the operand-stack primitives touch
//! no engine state. Each was established outside the binary — by writing map files and watching the
//! engine load them — and each fails if the analyser starts attributing calls or globals to the
//! wrong operator. They run without the proprietary binary.
//!
//! **Counts checked against the shipped script corpus.** The `.gs` members are the engine's own
//! callers, so an operand count recovered from a disassembler is a falsifiable prediction about
//! them. Five are pinned here with the member and offset that settles each, recovered with
//! `tools/gs_callsites.py`. This is the only independent authority available: everything else in
//! the file is a second look at the same bytes.
//!
//! **Checks against the binary, which do not skip silently.** These are `#[ignore]`d and run with
//! `cargo test --release -- --ignored`, so a machine without the executable reports them as
//! ignored rather than as passed. The previous version returned early when `LOM_EXE` was unset,
//! and `.ok()?` made a mistyped path indistinguishable from no path — so with helper discovery
//! fully collapsed the suite reported "8 passed" and printed nothing. An unrunnable check is not a
//! check.

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
    let text =
        std::fs::read_to_string(artifact_path()).expect("the classification table is committed");
    parse(&text)
}

fn parse(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut lines = text.lines();
    let columns: Vec<&str> = lines
        .next()
        .expect("the table has a header")
        .split('\t')
        .collect();
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

// ---------------------------------------------------------------------------------------------
// Relations established outside the binary
// ---------------------------------------------------------------------------------------------

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
        assert!(
            !globals.is_empty(),
            "{name} references no engine state at all"
        );
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
///
/// Asserts the **class**, not just the absence of globals. Asserting only the globals let the four
/// of them sit in `unknown` for calling the script error raiser, while the class comment claimed
/// they had been moved out of it.
#[test]
fn the_operand_stack_primitives_are_classified_as_operand_only() {
    let table = table();
    for name in ["dup", "exch", "pop", "roll"] {
        let row = &table[name];
        assert_eq!(
            row["global_addresses"], "-",
            "{name} is an operand-stack primitive but references engine state"
        );
        assert_eq!(
            row["behaviour"], "operand-only",
            "{name} is an operand-stack primitive but is classified {}",
            row["behaviour"]
        );
    }
}

/// The arithmetic operators read a shared float pool in **read-only** data, which is not engine
/// state. Counting it as state put nineteen of them in `reads-state` and left the class that
/// describes them with no members at all.
#[test]
fn the_arithmetic_operators_touch_constants_and_not_state() {
    let table = table();
    for name in ["abs", "atan", "cos", "sin", "sqrt", "tan"] {
        let row = &table[name];
        assert_eq!(
            row["global_addresses"], "-",
            "{name} should touch no engine state"
        );
        assert_ne!(
            row["constant_addresses"], "-",
            "{name} should read the shared float pool"
        );
        assert_eq!(row["behaviour"], "floating-point", "{name} misclassified");
    }
}

/// Twelve named operators share one entry point that is a single `ret`. They are registered names
/// with nothing behind them, and publishing them as anything else invents an implementation.
#[test]
fn the_unimplemented_operators_share_one_entry_point_and_are_labelled_as_stubs() {
    let table = table();
    let stubs: BTreeMap<&String, &String> = table
        .iter()
        .filter(|(_, row)| row["behaviour"] == "stub")
        .map(|(name, row)| (name, &row["entry_point"]))
        .collect();
    assert!(
        stubs.len() >= 12,
        "expected the shared no-op entry point to gather at least a dozen names, found {stubs:?}"
    );
    let mut by_entry: BTreeMap<&String, usize> = BTreeMap::new();
    for entry in stubs.values() {
        *by_entry.entry(entry).or_default() += 1;
    }
    let largest = by_entry.values().copied().max().unwrap_or(0);
    assert!(
        largest >= 12,
        "the stubs should share one entry point; the largest group is {largest}"
    );
    for name in ["savegridflags", "sunlight", "makelighttables", "setplane"] {
        assert_eq!(table[name]["behaviour"], "stub", "{name} should be a stub");
        assert_eq!(table[name]["instructions"], "1");
    }
}

/// `netlockgame` is the one body the walk cannot finish, and it must still be described rather than
/// dropped: a nullary tail call through a vtable slot on the network-session object.
#[test]
fn the_one_incomplete_body_still_names_its_dispatch_slot() {
    let table = table();
    let row = &table["netlockgame"];
    assert_eq!(row["boundary"], "unresolved-indirect-jump");
    assert_eq!(row["behaviour"], "unknown");
    assert_ne!(
        row["virtual_call_slots"], "-",
        "an unfollowable dispatch should still be named by object and slot"
    );
    let slots = list(row, "virtual_call_slots");
    assert_eq!(slots.len(), 1, "one dispatch, one slot: {slots:?}");
    let global = list(row, "global_addresses");
    assert_eq!(global.len(), 1);
    assert!(
        slots
            .iter()
            .next()
            .expect("one slot")
            .starts_with(global.iter().next().expect("one global")),
        "the slot should be attributed to the object the body loaded"
    );
}

// ---------------------------------------------------------------------------------------------
// Against the shipped script corpus
// ---------------------------------------------------------------------------------------------

/// Operand counts checked against the engine's own callers.
///
/// Each count below was read off a shipped `.gs` call site with `tools/gs_callsites.py`, which
/// resolves every name in the operand window against the corpus's own definitions so a procedure
/// that pushes one value is not counted as one token per word. The corpus is proprietary and is not
/// in this repository, so the member and offset that settles each one is recorded here instead.
///
/// These are the *only* independent authority in this file. Every other test is a second look at
/// the same bytes the analyser read.
#[test]
fn the_recovered_counts_agree_with_the_shipped_call_sites() {
    // (operator, operands at the call site, where it was read)
    const CALL_SITES: [(&str, usize, &str); 5] = [
        // `/full_xywh 0 0 109 32 xywh` -- four literals after a name boundary.
        ("xywh", 4, "PANELS5.gs:51:497"),
        // `sprite_id spelldef_id caster_level 1000 0 1 0 0 0 addcitymod`
        ("addcitymod", 9, "gs/spells/fireworks.gs:1:1129"),
        // `experience_bground_dd p_exp_dd -99 15 0 1 1 health_divider_dd bargraph`
        ("bargraph", 8, "selarmy2.gs:228:79"),
        // `0 KEEP 0 0 0 0 leader_type 0 setbuildingrequirements`
        ("setbuildingrequirements", 8, "building.gs:549:31"),
        // `myowner 1 50 15 relative2actual 50 50 relative2actual 3 0
        // getplayergroupintoformation` -- `relative2actual` takes two and returns two, which this
        // same table says, so the site passes 1+1+2+2+1+1.
        ("getplayergroupintoformation", 8, "getinfrm.gs:1:1079"),
    ];

    let table = table();
    let mut wrong = Vec::new();
    for (name, operands, source) in CALL_SITES {
        let row = &table[name];
        if row["nominal_arity"] != operands.to_string() {
            wrong.push(format!(
                "{name}: table says {}, the call site at {source} passes {operands}",
                row["nominal_arity"]
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// The old site count is refuted by the corpus for the same four operators, which is what makes
/// the disagreement a finding rather than a difference of method.
#[test]
fn the_call_sites_refute_the_previously_recorded_counts() {
    let table = table();
    for (name, recorded) in [
        ("addcitymod", "2"),
        ("bargraph", "2"),
        ("setbuildingrequirements", "1"),
    ] {
        assert_eq!(
            table[name]["declared_arity"], recorded,
            "the recorded count for {name} changed; re-read the call site before trusting this"
        );
        assert_ne!(
            table[name]["nominal_arity"], recorded,
            "{name} should disagree with the recorded count"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Table-wide invariants
// ---------------------------------------------------------------------------------------------

/// The count recovered by walking a body must be at least the count already recorded, because the
/// older measurement is a site count that misses operands fetched through a helper — a weakness its
/// own module documents. Anything *below* it is this analysis losing a path.
///
/// Three operators break the rule and they are named here rather than excused: `button`,
/// `setunitdata` and `nsetunitdata` pop a different number of operands on different branches, where
/// a site count is the higher number by construction.
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

/// Coverage, stated as a rate rather than a per-row verdict.
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

/// `mutates-state` must mean the body itself stores. It used to be granted on a direct callee's
/// store too, which made it false for 154 rows — including predicates like `armystrength`.
#[test]
fn the_mutating_class_is_only_ever_a_store_in_the_body_itself() {
    let table = table();
    let mut without_a_store = Vec::new();
    for (name, row) in &table {
        if row["behaviour"] != "mutates-state" {
            continue;
        }
        // A store is either to an absolute address or through a pointer the body read out of a
        // global; both are the body's own store.
        let writes: usize = row["globals_written"].parse().unwrap_or(0);
        let through_pointer = row["pointer_write_addresses"] != "-";
        if writes == 0 && !through_pointer {
            without_a_store.push(name.clone());
        }
    }
    assert!(
        without_a_store.is_empty(),
        "classified as mutating with no store of their own: {:?}",
        &without_a_store[..without_a_store.len().min(20)]
    );
}

/// A variadic operator is one with a fetch inside a **loop**, not one with many candidate counts.
///
/// The two were reported under a single flag, which cost `launchmissile` its count: it has
/// twenty-one candidate counts because twenty-one early exits converge, and no loop at all.
#[test]
fn a_long_chain_of_early_exits_is_not_a_variadic_operator() {
    let table = table();
    let launch = &table["launchmissile"];
    assert_eq!(
        launch["loop_carried_operands"], "no",
        "launchmissile fetches down one chain, not around a loop"
    );
    assert_eq!(launch["nominal_arity"], "21");
    assert!(
        list(launch, "arity_candidates").len() > 15,
        "the point of the case is that many counts reach its returns"
    );

    // A loop-carried count is never published as a nominal arity.
    let mut loops = 0;
    for (name, row) in &table {
        if row["loop_carried_operands"] == "yes" {
            loops += 1;
            assert_eq!(
                row["nominal_arity"], "-",
                "{name} consumes operands in a loop and cannot have a single count"
            );
        }
    }
    assert!(loops > 0, "the table should contain variadic operators");

    // The two conditions must be independently observable, or they are still one flag.
    let cap_without_loop = table
        .values()
        .filter(|row| {
            row["operand_state_cap_hit"] == "yes" && row["loop_carried_operands"] == "no"
        })
        .count();
    assert!(
        cap_without_loop > 0,
        "no operator hits the state cap without a loop, so the split is untested"
    );
}

/// The network operators dispatch through one object's vtable, and the recovered slots are what a
/// reader chasing the multiplayer code needs. Distinct operators must get distinct slots — one slot
/// for several operators would mean the taint is being attributed to the wrong load.
#[test]
fn the_network_operators_resolve_to_distinct_slots_on_one_object() {
    let table = table();
    let mut slots: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    let mut object: Option<String> = None;
    for name in [
        "createnetworkgame",
        "joinnetworkgame",
        "enumnetworkgames",
        "enumproviders",
        "netlockgame",
        "selectprovider",
    ] {
        let recovered = list(&table[name], "virtual_call_slots");
        assert_eq!(
            recovered.len(),
            1,
            "{name} should dispatch through exactly one slot, found {recovered:?}"
        );
        let slot = recovered.into_iter().next().expect("one slot");
        let (source, _) = slot.split_once('+').expect("slots are object+offset");
        match &object {
            None => object = Some(source.to_owned()),
            Some(previous) => assert_eq!(
                previous, source,
                "{name} dispatches on a different object than the rest of the family"
            ),
        }
        slots.entry(slot).or_default().push(&table[name]["name"]);
    }
    for (slot, operators) in &slots {
        assert_eq!(
            operators.len(),
            1,
            "slot {slot} is claimed by {operators:?}; the taint is following the wrong load"
        );
    }
}

/// An operand count must have somewhere to have come from: an inline pop, a recognised shared
/// helper, or a call to something whose own body pops. Twenty-one operators take the third route
/// through a per-subsystem wrapper too rarely called to be recognised as a shared helper.
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
// Against the binary. `cargo test --release -- --ignored` with LOM_EXE set.
// ---------------------------------------------------------------------------------------------

/// Read the executable, or fail loudly. Never returns `None`: a check that quietly passes when it
/// could not run is worse than no check.
fn executable() -> Vec<u8> {
    let path = std::env::var_os("LOM_EXE").expect(
        "set LOM_EXE to a copy of lomse.exe. This test is #[ignore]d precisely so that a machine \
         without the binary reports it as ignored instead of as passed.",
    );
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("LOM_EXE is set to {path:?} but cannot be read: {error}"))
}

fn regenerate(bytes: &[u8]) -> (String, usize) {
    use lom_asset_viewer::native_table::{self, PeImage};
    use lom_asset_viewer::operator_bodies;

    let image = PeImage::parse(bytes).expect("the executable parses");
    let runs = native_table::extract(bytes).expect("the operator tables are recoverable");
    let mut seen = BTreeSet::new();
    let mut operators = Vec::new();
    for entry in runs.iter().flat_map(|run| run.entries.iter()) {
        let name = entry.name.to_ascii_lowercase();
        if seen.insert(name.clone()) {
            operators.push((name, entry.entry_point));
        }
    }
    let analysis = operator_bodies::analyse(&image, &operators).expect("every entry point walks");
    let count = analysis.reports.len();
    (operator_bodies::operator_table(&analysis.reports), count)
}

/// The committed table still says what the binary says — **every column of it**.
///
/// Compares the regenerated text, not a handful of fields. The previous version checked three of
/// twenty-eight columns, and the two the offline anchors read were not among them.
#[test]
#[ignore = "needs LOM_EXE; run with --ignored"]
fn the_committed_table_matches_the_binary_in_every_column() {
    let bytes = executable();
    let (regenerated, count) = regenerate(&bytes);
    let committed =
        std::fs::read_to_string(artifact_path()).expect("the classification table is committed");
    if regenerated == committed {
        return;
    }
    // Name the rows that moved rather than dumping 400 KB of diff.
    let new = parse(&regenerated);
    let old = parse(&committed);
    assert_eq!(new.len(), count);
    let mut differences = Vec::new();
    for (name, row) in &new {
        match old.get(name) {
            None => differences.push(format!("{name}: missing from the committed table")),
            Some(previous) => {
                for (column, value) in row {
                    if previous.get(column) != Some(value) {
                        differences.push(format!("{name}.{column}"));
                    }
                }
            }
        }
    }
    for name in old.keys() {
        if !new.contains_key(name) {
            differences.push(format!("{name}: no longer in the table"));
        }
    }
    panic!(
        "the committed table is stale; regenerate it with the operator_bodies example. {} fields differ, first: {:?}",
        differences.len(),
        &differences[..differences.len().min(20)]
    );
}

/// The shared operand helpers are found by shape, so finding them is a claim about the binary.
///
/// What the set is load-bearing for is **measured**, not asserted: collapsing the search changes
/// `helper_pops` for 318 rows and `behaviour` for 44, and `nominal_arity` for 3. The arity result
/// rests on the generic callee-pop folding, not on this search. The test guards the two columns the
/// search does own.
#[test]
#[ignore = "needs LOM_EXE; run with --ignored"]
fn the_operand_helpers_are_discovered_by_shape() {
    use lom_asset_viewer::native_table::{self, PeImage};
    use lom_asset_viewer::operator_bodies::ProgramIndex;

    let bytes = executable();
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
         search collapsed back to the single-helper assumption",
        index.pop_helpers()
    );
    assert!(
        !index.push_helpers().is_empty(),
        "the engine has a shared result-push helper and the search should find it"
    );
}

/// The `.rdata`/`.data` distinction is a property of the binary's section headers, so it is checked
/// against them and not against the table.
#[test]
#[ignore = "needs LOM_EXE; run with --ignored"]
fn read_only_data_is_distinguished_from_engine_state_by_the_section_header() {
    use lom_asset_viewer::native_table::PeImage;

    let bytes = executable();
    let image = PeImage::parse(&bytes).expect("the executable parses");
    let mut writable = 0_usize;
    let mut constant = 0_usize;
    // Walk the mapped data range and count both kinds. Both must be non-empty, or the flag is
    // being read from the wrong header field and every constant looks like state.
    for address in (image.image_base()..image.image_base() + 0x0200_0000).step_by(0x1000) {
        if !image.is_data_address(address) {
            continue;
        }
        if image.is_writable_data_address(address) {
            writable += 1;
        } else {
            constant += 1;
        }
    }
    assert!(
        writable > 0 && constant > 0,
        "the image has both writable and read-only data sections; found {writable} writable and \
         {constant} read-only pages"
    );
}
