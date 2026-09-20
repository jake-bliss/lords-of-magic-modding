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
//! **The lineage is derived from the archive, not declared.** An earlier revision took it from
//! `LOM_GS_PROFILE` and defaulted to `patch302`, so three of the four installs on this machine
//! were red unless the operator named the lineage by hand -- and naming it *wrongly* silently
//! selected the wrong expectations rather than being caught. `Lineage::derive` reads two
//! independent signals out of `standard.gs` itself. `LOM_GS_PROFILE` is still honoured, but only
//! as a claim that must **agree** with the file; a disagreement fails the test.
//!
//! Verified on all four installed profiles on 2026-09-19.

use std::path::PathBuf;

use lom_asset_viewer::gamescript_standard::{
    EXERCISES, Lineage, MEMBER, load_module, run_exercises,
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

/// The lineage of the archive under test, read from its own `standard.gs`.
///
/// If `LOM_GS_PROFILE` is set it is checked against the derivation rather than believed. That is
/// the whole point: a declaration the artifact contradicts is a finding, not an override.
fn lineage(source: &[u8]) -> Lineage {
    let (derived, evidence) = Lineage::derive(source).expect("standard.gs names a known lineage");
    if let Ok(declared) = std::env::var("LOM_GS_PROFILE") {
        assert_eq!(
            declared,
            derived.label(),
            "LOM_GS_PROFILE says {declared}, but {MEMBER} is {} ({evidence:?})",
            derived.label()
        );
    }
    derived
}

#[test]
#[ignore = "executes shipped script; set LOM_GS_MPQ"]
fn the_standard_module_loads_and_defines_its_utilities() {
    let source = standard_gs();
    let (vm, _) = load_module(&source).expect("standard.gs loads");

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

/// Every exercise must do what its expectation says **for this archive's lineage**.
///
/// The previous revision allowed a set of "declared disagreements" per profile, which meant four
/// exercises asserted nothing at all on GS5R3 and two asserted nothing on the stock archive. They
/// now carry real expectations everywhere: GS5R3's reversed `min`/`max` are asserted to be
/// reversed, and an archive that does not ship the 3.02 string helpers is asserted to **stop** on
/// the undefined name. So a GS5R3 build whose `min` started returning the smaller operand would
/// fail here, where before it would have been tolerated.
#[test]
#[ignore = "executes shipped script; set LOM_GS_MPQ"]
fn every_exercise_agrees_with_its_expectation() {
    let source = standard_gs();
    let lineage = lineage(&source);
    let (mut vm, _) = load_module(&source).expect("standard.gs loads");
    let outcomes = run_exercises(&mut vm, lineage);
    assert_eq!(outcomes.len(), EXERCISES.len());

    let detail: Vec<String> = outcomes
        .iter()
        .filter_map(|outcome| {
            outcome
                .disagreement
                .as_ref()
                .map(|why| format!("{}: {why}", outcome.exercise))
        })
        .collect();
    assert!(
        detail.is_empty(),
        "{} of {} exercises disagreed with the {} reading of {MEMBER}:\n{}",
        detail.len(),
        outcomes.len(),
        lineage.label(),
        detail.join("\n")
    );
}

/// The property issue #5 is actually about: a name the engine owns must stop, never become a
/// value. `run_exercise` reports an `INVENTED`-shaped disagreement if one does, but this states it
/// directly over the whole battery so it cannot be lost in a lineage's expectations.
///
/// It also covers the two 3.02 string helpers on archives that do not define them, which is why
/// the expectation is taken from the lineage rather than from the exercise: on the stock archive
/// and on GS5R3, `string_cvi` and `char_cvs` are names with no definition, and stopping on them is
/// the correct behaviour rather than a gap in the VM. They are script procedures, not primitives
/// -- **observed in the corpus, 2026-09-19**: `/string_cvi`, `/char_cvs` and the `/char_array`
/// that `char_cvs` reads appear only in 3.02's `standard.gs`.
#[test]
#[ignore = "executes shipped script; set LOM_GS_MPQ"]
fn no_exercise_that_should_stop_produces_a_value_instead() {
    use lom_asset_viewer::gamescript_standard::Expected;

    let source = standard_gs();
    let lineage = lineage(&source);
    let (mut vm, _) = load_module(&source).expect("standard.gs loads");
    let mut stopped = 0_usize;
    for exercise in EXERCISES {
        let Expected::StopsOn(name) = lineage.expected_for(exercise) else {
            continue;
        };
        stopped += 1;
        let outcome =
            lom_asset_viewer::gamescript_standard::run_exercise(&mut vm, exercise, lineage);
        let trace = outcome.unknown_name.as_ref();
        assert!(
            trace.is_some_and(|trace| trace.name == name),
            "{} had to stop on {name}; instead: {:?}",
            exercise.name,
            outcome.disagreement
        );
    }
    // A tripwire on the loop itself: if `expected_for` ever stopped yielding `StopsOn`, this test
    // would pass vacuously while checking nothing.
    assert!(
        stopped > 0,
        "no exercise in the {} reading expects a stop; this test checked nothing",
        lineage.label()
    );
}
