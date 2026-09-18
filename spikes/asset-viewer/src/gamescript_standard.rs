//! The `gs\\standard.gs` exercise battery, as library code so that `cargo test` can run it.
//!
//! This table used to live in `examples/gamescript_standard.rs`, which meant it ran only when a
//! person invoked the example by hand. A cross-model review measured the consequence: the example
//! carried 32 exercises and **zero** `#[test]` attributes, and the crate had no `tests/` directory,
//! so every "225 passed" figure quoted about this work was a suite that never executed a single
//! exercise. Changing `sin` to call `cos` would have left the suite green. The expectations being
//! written before the run mattered much less than nothing re-checking them afterwards.
//!
//! The battery needs the shipped `gs\\standard.gs`, which is not in Git, so
//! `tests/gamescript_standard.rs` is `#[ignore]`d and reports as `ignored` rather than not
//! existing. Run it with:
//!
//! ```text
//! LOM_GS_MPQ=/path/to/English/gs.mpq cargo test -- --ignored
//! ```
//!
//! What a *default* `cargo test` covers instead is the language the battery is built on:
//! `gamescript_vm`'s own unit tests, including `trigonometry_is_not_self_consistent_under_a_swap`,
//! which is the specific mutation this structure exists to catch.

use crate::gamescript::GameScriptDocument;
use crate::gamescript_vm::{GameScriptVm, GameScriptVmError, UnknownNameTrace, Value};

/// What an exercise must do.
pub enum Expected {
    /// Run to completion and leave exactly this rendered operand stack.
    Stack(&'static str),
    /// Stop on this engine name. The VM must not invent a value for it.
    StopsOn(&'static str),
}

pub struct Exercise {
    pub name: &'static str,
    pub source: &'static str,
    pub expected: Expected,
    pub note: &'static str,
}

pub const MEMBER: &str = "gs\\standard.gs";
pub const STEP_LIMIT: usize = 2_000_000;

/// The engine-light battery.
///
/// Every `source` calls only procedures `standard.gs` defines; the expectations were derived by
/// reading the shipped bodies, so an expectation that disagrees with the run is a finding either
/// way round.
pub const EXERCISES: &[Exercise] = &[
    Exercise {
        name: "min",
        source: "3 7 min 7 3 min",
        expected: Expected::Stack("3 3"),
        note: "clamp helper, both operand orders",
    },
    Exercise {
        name: "max",
        source: "3 7 max 7 3 max",
        expected: Expected::Stack("7 7"),
        note: "clamp helper, both operand orders",
    },
    Exercise {
        name: "between",
        source: "3 1 5 between 9 1 5 between 1 1 5 between",
        expected: Expected::Stack("true false true"),
        note: "value low high; inclusive at the ends",
    },
    Exercise {
        name: "script-defined index shadows the primitive",
        source: "10 20 30 1 index",
        expected: Expected::Stack("10 20 30 20"),
        note: "standard.gs redefines /index using roll; it must agree with the primitive",
    },
    Exercise {
        name: "getflagvalue",
        source: "5 0 getflagvalue 5 1 getflagvalue 5 2 getflagvalue",
        expected: Expected::Stack("true false true"),
        note: "bit test; its `and` yields an integer that `ifelse` consumes as a condition",
    },
    Exercise {
        name: "setflagvalue",
        source: "4 0 true setflagvalue 5 0 false setflagvalue",
        expected: Expected::Stack("5 4"),
        note: "set and clear bit zero",
    },
    Exercise {
        name: "dump_flags is a no-op on its operand",
        source: "5 dump_flags 4 dump_flags",
        expected: Expected::Stack("5 4"),
        note: "after the first iteration it tests the loop counter, not the flags, then drops it",
    },
    Exercise {
        name: "radians",
        source: "180 radians",
        expected: Expected::Stack("3.141596"),
        note: "the module's own degree-to-radian conversion, applied before every sin/cos",
    },
    Exercise {
        name: "polar",
        source: "0 0 10 0 polar",
        expected: Expected::Stack("10 0"),
        note: "x y distance angle; zero degrees is +x",
    },
    Exercise {
        name: "stack: push, pop, LIFO order",
        source: "/s 4 stack def s 7 pushonstack s 9 pushonstack s popoffstack s popoffstack",
        expected: Expected::Stack("9 7"),
        note: "the module's array-backed stack; every entry point consumes the array reference",
    },
    Exercise {
        name: "stack: layout after two pushes",
        source: "/s 4 stack def s 7 pushonstack s 9 pushonstack s",
        expected: Expected::Stack("[2 7 9 0 0]"),
        note: "count in slot zero, values from slot one",
    },
    Exercise {
        name: "onstack?",
        source: "/s 4 stack def s 7 pushonstack s 9 pushonstack s 9 onstack? s 8 onstack?",
        expected: Expected::Stack("true false"),
        note: "linear search with an early exit",
    },
    Exercise {
        name: "popoffstack underflows to /null",
        source: "/s 4 stack def s popoffstack",
        expected: Expected::Stack("/null"),
        note: "an empty stack answers with a name, not an error",
    },
    Exercise {
        name: "retrievefromstack",
        source: "/s 4 stack def s 7 pushonstack s 9 pushonstack s 1 retrievefromstack",
        expected: Expected::Stack("7"),
        note: "extract by index and close the gap",
    },
    Exercise {
        name: "dumpstack drains onto the operand stack",
        source: "/s 4 stack def s 7 pushonstack s dumpstack",
        expected: Expected::Stack("7"),
        note: "it pops until /null and leaves every popped value behind",
    },
    Exercise {
        name: "get_if_known",
        source: "<< /a 1 >> /a 99 get_if_known << /a 1 >> /b 99 get_if_known",
        expected: Expected::Stack("1 99"),
        note: "operands are dictionary, key, default -- the reverse of the header comment",
    },
    Exercise {
        name: "exec_if_known",
        source: "<< /a {41 1 add} >> /a exec_if_known << /a {1} >> /b exec_if_known",
        expected: Expected::Stack("42"),
        note: "runs the value when the key is present and leaves nothing when it is not",
    },
    Exercise {
        name: "interpolate, between two keys",
        source: "<< 0 0 10 100 >> 5 interpolate",
        expected: Expected::Stack("50"),
        note: "numeric dictionary keys walked with forall, then linear interpolation",
    },
    Exercise {
        name: "interpolate, exact key",
        source: "<< 0 0 10 100 >> 10 interpolate",
        expected: Expected::Stack("100"),
        note: "an exact hit returns the stored value without interpolating",
    },
    Exercise {
        name: "string_cvi",
        source: "\"1234\" string_cvi \"-42\" string_cvi",
        expected: Expected::Stack("1234 -42"),
        note: "digit-by-digit conversion over the string's character codes",
    },
    Exercise {
        name: "char_cvs reads the procedure's attached array",
        source: "65 char_cvs 97 char_cvs",
        expected: Expected::Stack("\"A\" \"a\""),
        note: "/char_array is attached with `replace` and read as a literal name",
    },
    Exercise {
        name: "stackdump is empty",
        source: "stackdump",
        expected: Expected::Stack(""),
        note: "the shipped body is `{}`",
    },
    Exercise {
        name: "makeregion needs the terrain host",
        source: "0 0 5 1 makeregion",
        expected: Expected::StopsOn("rand"),
        note: "map painting; the first engine call is the random source",
    },
    Exercise {
        name: "writestring needs file output",
        source: "1 \"text\" writestring",
        expected: Expected::StopsOn("write"),
        note: "byte output to an engine file handle",
    },
    Exercise {
        name: "eval needs the temporary-file host",
        source: "\"1 1 add\" eval",
        expected: Expected::StopsOn("gettemppath"),
        note: "the module writes a scratch file and `run`s it",
    },
    Exercise {
        name: "free_stack_elements needs the allocator",
        source: "/s 4 stack def s 7 pushonstack s free_stack_elements",
        expected: Expected::StopsOn("free"),
        note: "releases engine-owned handles held in a stack",
    },
    Exercise {
        name: "closeifopen needs the dialog host",
        source: "1 closeifopen",
        expected: Expected::StopsOn("dialogisopen?"),
        note: "user interface state",
    },
    Exercise {
        name: "exitapplication needs the dialog host",
        source: "exitapplication",
        expected: Expected::StopsOn("sysdlg"),
        note: "user interface state",
    },
    Exercise {
        name: "geteasyunitdata needs the unit tables",
        source: "0 geteasyunitdata",
        expected: Expected::StopsOn("unitdictxref"),
        note: "game data the engine owns",
    },
    Exercise {
        name: "addrect needs the dialog host",
        source: "1 2 3 4 {} addrect",
        expected: Expected::StopsOn("additem"),
        note: "dialog layout",
    },
    Exercise {
        name: "retrievefromstack's error path needs the reporter",
        source: "/s 4 stack def s 0 retrievefromstack",
        expected: Expected::StopsOn("build_statement"),
        note: "the out-of-range branch formats a message with an engine helper",
    },
];

/// The verdict on one exercise. `Ok` means it did what its expectation said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub exercise: &'static str,
    /// `None` when the exercise matched its expectation.
    pub disagreement: Option<String>,
    /// The engine name this exercise stopped on, whether or not that was expected. Collected so a
    /// survey can count the distinct host calls the battery reaches.
    pub unknown_name: Option<UnknownNameTrace>,
}

/// Load `gs\standard.gs` from its bytes and return a machine holding its definitions.
pub fn load_module(source: &[u8]) -> Result<(GameScriptVm, GameScriptDocument), String> {
    let document = GameScriptDocument::parse(source)
        .map_err(|error| format!("could not parse {MEMBER}: {error}"))?;
    let mut vm = GameScriptVm::new(STEP_LIMIT);
    vm.execute_document(&document)
        .map_err(|error| format!("{MEMBER} did not load: {error}"))?;
    Ok((vm, document))
}

/// Run every exercise against a loaded module.
///
/// Each exercise starts from a clean operand and dictionary stack, so an exercise that stops on a
/// host call cannot leave debris that changes the next one's result.
pub fn run_exercises(vm: &mut GameScriptVm) -> Vec<Outcome> {
    EXERCISES
        .iter()
        .map(|exercise| run_exercise(vm, exercise))
        .collect()
}

pub fn run_exercise(vm: &mut GameScriptVm, exercise: &Exercise) -> Outcome {
    vm.reset_stacks();
    let outcome = evaluate(vm, exercise.source);
    let unknown_name = outcome
        .as_ref()
        .err()
        .and_then(GameScriptVmError::unknown_name);
    let disagreement = match (&exercise.expected, &outcome) {
        (Expected::Stack(expected), Ok(actual)) if actual == expected => None,
        (Expected::Stack(expected), Ok(actual)) => {
            Some(format!("expected [{expected}], got [{actual}]"))
        }
        (Expected::Stack(expected), Err(error)) => {
            Some(format!("expected [{expected}], stopped: {error}"))
        }
        (Expected::StopsOn(expected), Err(error)) => match error.unknown_name() {
            Some(trace) if trace.name == *expected => None,
            Some(trace) => Some(format!(
                "expected a stop on {expected}, stopped on {}",
                trace.name
            )),
            None => Some(format!(
                "expected a stop on {expected}, failed instead: {error}"
            )),
        },
        // The arm that matters most: a host call must never turn into a value.
        (Expected::StopsOn(expected), Ok(actual)) => Some(format!(
            "expected a stop on {expected}, but it produced [{actual}]"
        )),
    };
    Outcome {
        exercise: exercise.name,
        disagreement,
        unknown_name,
    }
}

/// Run one expression against the loaded module and render whatever it leaves.
pub fn evaluate(vm: &mut GameScriptVm, source: &str) -> Result<String, GameScriptVmError> {
    let document =
        GameScriptDocument::parse(source.as_bytes()).map_err(|error| GameScriptVmError {
            message: format!("exercise did not parse: {error}"),
            step: 0,
            call_stack: Vec::new(),
        })?;
    vm.execute_document(&document)?;
    Ok(vm
        .operand_stack()
        .iter()
        .map(Value::render)
        .collect::<Vec<_>>()
        .join(" "))
}

/// The exercises GS5R3 is expected to disagree on, and why.
///
/// Two because it ships `min` and `max` with their bodies exchanged, and two because it does not
/// ship `string_cvi` or `char_cvs` at all -- those are 3.02 additions. Naming them is what turns
/// "the GS5R3 run has four failures" from a footnote into an assertion.
pub const GS5R3_EXPECTED_DISAGREEMENTS: &[&str] = &[
    "char_cvs reads the procedure's attached array",
    "max",
    "min",
    "string_cvi",
];

/// The exercises vanilla is expected to disagree on: it has neither 3.02 string helper.
pub const VANILLA_EXPECTED_DISAGREEMENTS: &[&str] = &[
    "char_cvs reads the procedure's attached array",
    "string_cvi",
];
