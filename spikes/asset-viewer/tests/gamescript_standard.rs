//! The `gs\standard.gs` exercise battery, under `cargo test`.
//!
//! **Why these are `#[ignore]`d.** The battery executes the *shipped* utility module, and neither
//! `gs.mpq` nor its members are in Git. So these tests cannot run unattended, and they are marked
//! ignored so that `cargo test` lists them as `ignored` rather than leaving them silently absent --
//! which is the state a cross-model review found this work in: 32 exercises lived in an example
//! with zero `#[test]` attributes and no `tests/` directory, so no suite ever executed one.
//!
//! Run them against a local installation:
//!
//! ```text
//! LOM_GS_MPQ='/path/to/English/gs.mpq' cargo test -- --ignored
//! ```
//!
//! Optionally set `LOM_GS_PROFILE` to `vanilla`, `patch302` or `gs5r3` to say which lineage the
//! archive is, so the expected per-profile disagreements are asserted rather than tolerated. The
//! default is `patch302`, which is the only profile that disagrees on nothing.

use std::collections::BTreeSet;
use std::path::PathBuf;

use lom_asset_viewer::gamescript_standard::{
    EXERCISES, GS5R3_EXPECTED_DISAGREEMENTS, MEMBER, VANILLA_EXPECTED_DISAGREEMENTS, load_module,
    run_exercises,
};
use lom_asset_viewer::mpq::Archive;

/// Read `gs\standard.gs` out of the archive named by `LOM_GS_MPQ`.
fn standard_gs() -> Vec<u8> {
    let path = PathBuf::from(std::env::var("LOM_GS_MPQ").expect(
        "set LOM_GS_MPQ to a local English/gs.mpq; these tests execute shipped script that is not in Git",
    ));
    let archive = Archive::open(&path).expect("gs.mpq opens");
    let entries = archive.entries().expect("gs.mpq lists");
    let entry = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(MEMBER))
        .unwrap_or_else(|| panic!("{} has no member {MEMBER}", path.display()));
    archive.read(&entry.name).expect("member reads")
}

/// The exercises this profile is expected to disagree on, by name.
fn expected_disagreements() -> BTreeSet<&'static str> {
    let profile = std::env::var("LOM_GS_PROFILE").unwrap_or_else(|_| "patch302".to_owned());
    let names: &[&str] = match profile.as_str() {
        "patch302" => &[],
        "vanilla" => VANILLA_EXPECTED_DISAGREEMENTS,
        "gs5r3" => GS5R3_EXPECTED_DISAGREEMENTS,
        other => panic!("unknown LOM_GS_PROFILE {other}; use vanilla, patch302 or gs5r3"),
    };
    names.iter().copied().collect()
}

#[test]
#[ignore = "executes shipped script; set LOM_GS_MPQ"]
fn the_standard_module_loads_and_defines_its_utilities() {
    let (vm, _) = load_module(&standard_gs()).expect("standard.gs loads");

    // A module load that left operands behind would mean the VM mis-modelled one of its
    // definitions; the corpus's own idiom is that a definition consumes everything it pushes.
    assert!(
        vm.operand_stack().is_empty(),
        "loading left {} operands behind: {:?}",
        vm.operand_stack().len(),
        vm.operand_stack()
    );
    // Named, not counted: a count says nothing about which definition went missing.
    for name in [
        "min",
        "max",
        "between",
        "interpolate",
        "stack",
        "get_if_known",
    ] {
        assert!(
            vm.defined_names().iter().any(|defined| defined == name),
            "standard.gs should define {name}"
        );
    }
}

/// Every exercise must do what its expectation says, allowing only the disagreements this profile
/// is *declared* to have. An undeclared disagreement fails, and so does a declared one that stops
/// happening -- the second direction is what catches an expectation quietly rotting into agreement.
#[test]
#[ignore = "executes shipped script; set LOM_GS_MPQ"]
fn every_exercise_agrees_with_its_expectation() {
    let (mut vm, _) = load_module(&standard_gs()).expect("standard.gs loads");
    let outcomes = run_exercises(&mut vm);
    assert_eq!(outcomes.len(), EXERCISES.len());

    let expected = expected_disagreements();
    let actual: BTreeSet<&str> = outcomes
        .iter()
        .filter(|outcome| outcome.disagreement.is_some())
        .map(|outcome| outcome.exercise)
        .collect();

    let detail: Vec<String> = outcomes
        .iter()
        .filter_map(|outcome| {
            outcome
                .disagreement
                .as_ref()
                .map(|why| format!("{}: {why}", outcome.exercise))
        })
        .collect();
    assert_eq!(
        actual,
        expected,
        "disagreements were:\n{}",
        detail.join("\n")
    );
}

/// The property issue #5 is actually about: a name the engine owns must stop, never become a
/// value. `run_exercise` reports an `INVENTED`-shaped disagreement if one does, but this states it
/// directly over the whole battery so it cannot be lost in a profile's declared exceptions.
#[test]
#[ignore = "executes shipped script; set LOM_GS_MPQ"]
fn no_exercise_that_should_stop_produces_a_value_instead() {
    use lom_asset_viewer::gamescript_standard::Expected;

    let (mut vm, _) = load_module(&standard_gs()).expect("standard.gs loads");
    for exercise in EXERCISES {
        if let Expected::StopsOn(name) = exercise.expected {
            let outcome = lom_asset_viewer::gamescript_standard::run_exercise(&mut vm, exercise);
            let trace = outcome.unknown_name.as_ref();
            assert!(
                trace.is_some_and(|trace| trace.name == name),
                "{} had to stop on {name}; instead: {:?}",
                exercise.name,
                outcome.disagreement
            );
        }
    }
}
