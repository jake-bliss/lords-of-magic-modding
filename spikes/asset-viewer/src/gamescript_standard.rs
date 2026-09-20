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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
pub fn run_exercises(vm: &mut GameScriptVm, lineage: Lineage) -> Vec<Outcome> {
    EXERCISES
        .iter()
        .map(|exercise| run_exercise(vm, exercise, lineage))
        .collect()
}

pub fn run_exercise(vm: &mut GameScriptVm, exercise: &Exercise, lineage: Lineage) -> Outcome {
    vm.reset_stacks();
    let expected = lineage.expected_for(exercise);
    let outcome = evaluate(vm, exercise.source);
    let unknown_name = outcome
        .as_ref()
        .err()
        .and_then(GameScriptVmError::unknown_name);
    let disagreement = match (&expected, &outcome) {
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

/// Which lineage a `gs\standard.gs` belongs to, **derived from the file itself**.
///
/// The previous design took this from a `LOM_GS_PROFILE` environment variable and defaulted to
/// `patch302`, so three of the four installs on this machine failed unless the operator declared
/// the lineage by hand -- and a *wrong* declaration silently selected the wrong expectations
/// instead of being caught. A profile asserted by the operator is not evidence about the artifact.
/// This reads the artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lineage {
    /// The stock retail scripts. On this machine: the Steam build and `Lords of Magic Development`,
    /// whose `gs.mpq` files are byte-identical.
    Vanilla,
    /// The 3.02 patch, which adds the two string helpers.
    Patch302,
    /// ManTerA's GS5R3, which reverses `/min` and `/max`.
    Gs5r3,
}

/// The two signals `Lineage::derive` reads, kept separate so a refusal can name which one was odd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineageEvidence {
    /// Whether `standard.gs` defines `string_cvi` and `char_cvs`.
    ///
    /// **Observed in the corpus, 2026-09-19**, in all three distinct `gs.mpq` files on this
    /// machine: defined **only** in 3.02 (`standard.gs` md5 `881a31838368e9a4e4bd74b48f69f852`),
    /// along with the `/char_array` the second one reads. Absent from the stock archive
    /// (`4786977e4ecf8b73028df2a3aa71fe3a`) and from GS5R3 (`34e4525f63bbd4832c38aa1153612bba`).
    pub defines_string_helpers: bool,
    /// The comparison inside `/min`: `gt` in the ordinary reading, `lt` in GS5R3.
    pub minimum_comparison: &'static str,
}

/// The comparison token inside a `/NAME{2 copy CMP{exch pop}{pop}ifelse}` definition.
///
/// Matched on the **whitespace-free** text because the archives disagree about layout: 3.02 writes
/// `/min {\n  2 copy gt {...`, GS5R3 writes `/min{2 copy lt{...`, and the stock archive puts the
/// whole library on one line. Comments are stripped per line first, since `;` runs to a line end
/// and no further, and bare CR is a line ending in this format.
fn definition_comparison(compact: &str, name: &str) -> Option<&'static str> {
    let head = format!("/{name}{{2copy");
    let rest = &compact[compact.find(&head)? + head.len()..];
    if rest.starts_with("gt") {
        Some("gt")
    } else if rest.starts_with("lt") {
        Some("lt")
    } else {
        None
    }
}

/// Strip comments and all whitespace, so one match works on every layout the archives use.
fn compact_source(source: &[u8]) -> String {
    String::from_utf8_lossy(source)
        .split(['\r', '\n'])
        .flat_map(|line| line.split(';').next().unwrap_or("").chars())
        .filter(|c| !c.is_whitespace())
        .collect()
}

impl Lineage {
    /// Read the lineage out of a `gs\standard.gs`.
    ///
    /// Two independent signals, so neither alone decides: whether the 3.02 string helpers are
    /// defined, and which comparison `/min` is built on. An unmet combination is refused by name
    /// rather than guessed at.
    pub fn derive(standard_source: &[u8]) -> Result<(Self, LineageEvidence), String> {
        let compact = compact_source(standard_source);
        let defines_string_helpers = ["string_cvi", "char_cvs"]
            .iter()
            .all(|name| compact.contains(&format!("/{name}{{")));
        let minimum_comparison = definition_comparison(&compact, "min").ok_or_else(|| {
            format!("{MEMBER} does not define /min as a two-operand comparison")
        })?;
        let maximum_comparison = definition_comparison(&compact, "max").ok_or_else(|| {
            format!("{MEMBER} does not define /max as a two-operand comparison")
        })?;
        if minimum_comparison == maximum_comparison {
            return Err(format!(
                "{MEMBER} defines /min and /max with the same comparison ({minimum_comparison})"
            ));
        }
        let evidence = LineageEvidence {
            defines_string_helpers,
            minimum_comparison,
        };
        let lineage = match (defines_string_helpers, minimum_comparison) {
            (true, "gt") => Self::Patch302,
            (false, "gt") => Self::Vanilla,
            (false, "lt") => Self::Gs5r3,
            // Never seen on this machine. Refuse rather than fold it into a neighbour: an archive
            // with 3.02's helpers *and* GS5R3's reversal is a combination nothing here has
            // measured, and its min/max expectations would be a guess.
            (true, _) => {
                return Err(format!(
                    "{MEMBER} defines the 3.02 string helpers but builds /min on \
                     {minimum_comparison}; no measured lineage has that combination"
                ));
            }
            _ => unreachable!("definition_comparison yields only gt or lt"),
        };
        Ok((lineage, evidence))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Patch302 => "patch302",
            Self::Gs5r3 => "gs5r3",
        }
    }

    /// What this exercise must do **in this lineage**.
    ///
    /// The `expected` on each [`Exercise`] is the 3.02 reading, which is where the battery was
    /// written. Where an archive genuinely behaves differently, that difference is asserted here
    /// rather than tolerated as a "declared disagreement": every exercise gets a real expectation
    /// on every profile, so a GS5R3 run that started agreeing with 3.02's `min` would fail.
    ///
    /// **Read carefully what the `min`/`max` rows do and do not claim.** They do **not** assert
    /// that `min` returns the smaller operand -- in GS5R3 it does not, and saying so would be
    /// false. They assert that the token computes what its *own definition* computes: GS5R3's
    /// `/min` is built on `lt`, which yields the larger operand, so `3 7 min 7 3 min` leaves
    /// `7 7` there and `3 3` everywhere else. The invariant that holds in every lineage is that
    /// **the battery agrees with the definition the archive ships**, not that a name means what it
    /// is spelled.
    pub fn expected_for(self, exercise: &Exercise) -> Expected {
        match (self, exercise.name) {
            // GS5R3 reverses both definitions, under its own comment
            // `WILL WORK TO REVERSE THE TWO ABOVE BY USING THE TWO BELOW`.
            (Self::Gs5r3, "min") => Expected::Stack("7 7"),
            (Self::Gs5r3, "max") => Expected::Stack("3 3"),
            // Neither the stock archive nor GS5R3 defines the 3.02 string helpers. The VM must
            // **stop** on the undefined name rather than invent a value for it, which is the
            // property `no_exercise_that_should_stop_produces_a_value_instead` states over the
            // whole battery. This is not a VM gap: they are script procedures, not primitives, and
            // an archive that does not define one has no such name to call.
            (Self::Vanilla | Self::Gs5r3, "string_cvi") => Expected::StopsOn("string_cvi"),
            (
                Self::Vanilla | Self::Gs5r3,
                "char_cvs reads the procedure's attached array",
            ) => Expected::StopsOn("char_cvs"),
            _ => exercise.expected,
        }
    }
}
