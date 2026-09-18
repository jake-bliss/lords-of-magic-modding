//! Execute the engine-light subset of `gs\standard.gs` and trace what the engine still owes us.
//!
//! `standard.gs` is the corpus's utility module: stacks, clamps, flag helpers, string/number
//! conversion, interpolation. Most of it is pure language and runs here in full. The rest calls the
//! engine, and every one of those calls **stops** with a structured trace rather than returning an
//! invented value.
//!
//! Usage:
//!
//! ```text
//! cargo run --example gamescript_standard -- --gs PATH/gs.mpq [--exe PATH/lomse.exe]
//! ```
//!
//! The exercises below state the stack they must produce, worked out from the shipped bodies. They
//! are not recordings of this VM's output: several were wrong the first time and the run said so.
//! Exit status is non-zero if any exercise disagrees with its expectation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use lom_asset_viewer::gamescript::GameScriptDocument;
use lom_asset_viewer::gamescript_vm::{GameScriptVm, GameScriptVmError, Value};
use lom_asset_viewer::mpq::Archive;
use lom_asset_viewer::native_table::{NameClass, OperatorIndex};

/// What an exercise must do.
enum Expected {
    /// Run to completion and leave exactly this rendered operand stack.
    Stack(&'static str),
    /// Stop on this engine name. The VM must not invent a value for it.
    StopsOn(&'static str),
}

struct Exercise {
    name: &'static str,
    source: &'static str,
    expected: Expected,
    note: &'static str,
}

const MEMBER: &str = "gs\\standard.gs";
const STEP_LIMIT: usize = 2_000_000;

/// The engine-light battery.
///
/// Every `source` calls only procedures `standard.gs` defines; the expectations were derived by
/// reading the shipped bodies, so an expectation that disagrees with the run is a finding either
/// way round.
const EXERCISES: &[Exercise] = &[
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

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mut archive_path: Option<PathBuf> = None;
    let mut executable_path: Option<PathBuf> = None;
    let mut survey = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--gs" => {
                index += 1;
                archive_path = arguments.get(index).map(PathBuf::from);
            }
            "--exe" => {
                index += 1;
                executable_path = arguments.get(index).map(PathBuf::from);
            }
            "--survey" => survey = true,
            other => {
                eprintln!("unexpected argument {other}");
                eprintln!("usage: --gs PATH/gs.mpq [--exe PATH/lomse.exe] [--survey]");
                std::process::exit(2);
            }
        }
        index += 1;
    }
    let Some(archive_path) = archive_path else {
        eprintln!("usage: --gs PATH/gs.mpq [--exe PATH/lomse.exe] [--survey]");
        std::process::exit(2);
    };

    match run(&archive_path, executable_path.as_deref(), survey) {
        Ok(0) => {}
        Ok(failures) => {
            eprintln!("{failures} exercises did not match their expectation");
            std::process::exit(1);
        }
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}

fn run(archive_path: &Path, executable_path: Option<&Path>, survey: bool) -> Result<usize, String> {
    let operators = executable_path
        .map(|path| {
            let bytes = std::fs::read(path)
                .map_err(|error| format!("could not read {}: {error}", path.display()))?;
            OperatorIndex::from_image(&bytes)
                .map_err(|error| format!("could not read the operator table: {error}"))
        })
        .transpose()?;

    let archive = Archive::open(archive_path)
        .map_err(|error| format!("could not open the archive: {error}"))?;
    let entries = archive
        .entries()
        .map_err(|error| format!("could not list the archive: {error}"))?;
    let entry = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(MEMBER))
        .ok_or_else(|| format!("the archive has no member {MEMBER}"))?;
    let bytes = archive
        .read(&entry.name)
        .map_err(|error| format!("could not read {MEMBER}: {error}"))?;

    let document = GameScriptDocument::parse(&bytes)
        .map_err(|error| format!("could not parse {MEMBER}: {error}"))?;
    let analysis = document.analyze();

    println!("member\t{MEMBER}");
    println!("source-bytes\t{}", bytes.len());
    println!("tokens\t{}", analysis.token_count);
    println!("comments\t{}", analysis.comment_count);
    println!("procedure-anomalies\t{}", analysis.procedure_anomaly_count);

    let mut vm = GameScriptVm::new(STEP_LIMIT);
    vm.execute_document(&document)
        .map_err(|error| format!("{MEMBER} did not load: {error}"))?;
    let defined = vm.defined_names();
    println!("module-load\tcomplete");
    println!("module-steps\t{}", vm.steps());
    println!("module-defined-names\t{}", defined.len());
    for name in &defined {
        println!("module-defined-name\t{name}");
    }

    let mut failures = 0;
    let mut unknown_names: BTreeMap<String, usize> = BTreeMap::new();
    let mut unknown_frames: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for exercise in EXERCISES {
        vm.reset_stacks();
        let outcome = evaluate(&mut vm, exercise.source);
        if let Err(error) = &outcome
            && let Some(trace) = error.unknown_name()
        {
            *unknown_names.entry(trace.name.clone()).or_default() += 1;
            unknown_frames
                .entry(trace.name.clone())
                .or_insert_with(|| trace.call_stack.clone());
        }

        let verdict = match (&exercise.expected, &outcome) {
            (Expected::Stack(expected), Ok(actual)) if actual == expected => "ok".to_owned(),
            (Expected::Stack(expected), Ok(actual)) => {
                failures += 1;
                format!("MISMATCH\texpected [{expected}]\tgot [{actual}]")
            }
            (Expected::Stack(expected), Err(error)) => {
                failures += 1;
                format!("STOPPED\texpected [{expected}]\t{error}")
            }
            (Expected::StopsOn(expected), Err(error)) => match error.unknown_name() {
                Some(trace) if trace.name == *expected => format!("ok\tstopped on {expected}"),
                Some(trace) => {
                    failures += 1;
                    format!(
                        "MISMATCH\texpected a stop on {expected}\tstopped on {}",
                        trace.name
                    )
                }
                None => {
                    failures += 1;
                    format!("MISMATCH\texpected a stop on {expected}\tfailed instead: {error}")
                }
            },
            (Expected::StopsOn(expected), Ok(actual)) => {
                failures += 1;
                format!("INVENTED\texpected a stop on {expected}\tbut it produced [{actual}]")
            }
        };
        println!("exercise\t{}\t{verdict}", exercise.name);
        println!("exercise-note\t{}\t{}", exercise.name, exercise.note);
    }

    println!("exercises\t{}", EXERCISES.len());
    println!("exercise-failures\t{failures}");
    println!("distinct-unknown-native-names\t{}", unknown_names.len());
    for (name, hits) in &unknown_names {
        let class = match operators.as_ref().map(|index| index.classify(name)) {
            Some(NameClass::Operator { entry_point }) => format!("operator\t{entry_point:#010x}"),
            Some(NameClass::EngineConstant) => "engine-constant\t".to_owned(),
            Some(NameClass::Unresolved) => "unresolved\t".to_owned(),
            None => "unclassified\t".to_owned(),
        };
        let frames = unknown_frames
            .get(name)
            .map(|stack| stack.join(" -> "))
            .unwrap_or_default();
        println!("unknown-native\t{name}\t{hits}\t{class}\t{frames}");
    }

    // A name the module calls but never defines, and which this VM does not implement, is the
    // module's static debt to the engine. It is a superset of what the battery reached.
    //
    // "Defines" has to include the names bound inside a procedure's private dictionary --
    // `/terraintype exch def` and the like -- which never reach the top-level dictionary. Judging
    // by the top-level dictionary alone files every loop variable in the module as engine debt.
    let mut defined: BTreeSet<String> = defined.into_iter().collect();
    defined.extend(analysis.definition_names.keys().cloned());
    let static_debt: BTreeSet<&String> = analysis
        .executable_names
        .keys()
        .filter(|name| {
            !defined.contains(*name) && !lom_asset_viewer::gamescript_vm::is_primitive(name)
        })
        .collect();
    println!("static-engine-debt\t{}", static_debt.len());
    for name in &static_debt {
        let class = match operators.as_ref().map(|index| index.classify(name)) {
            Some(NameClass::Operator { .. }) => "operator",
            Some(NameClass::EngineConstant) => "engine-constant",
            Some(NameClass::Unresolved) => "unresolved",
            None => "unclassified",
        };
        println!("static-engine-debt-name\t{name}\t{class}");
    }

    if survey {
        survey_module_loads(&archive, &entries, operators.as_ref());
    }

    Ok(failures)
}

/// Load every `.gs` member on its own and record what stopped it.
///
/// This is the size of the module-loading question: how much of the corpus is pure language, and
/// which engine names stand between the VM and the rest. Each member gets a fresh machine, so a
/// member that stops cannot corrupt the next one's result.
fn survey_module_loads(
    archive: &Archive,
    entries: &[lom_asset_viewer::mpq::Entry],
    operators: Option<&OperatorIndex>,
) {
    let mut loaded = 0_usize;
    let mut stopped_on_name = 0_usize;
    let mut other_failure = 0_usize;
    let mut unparsable = 0_usize;
    let mut blockers: BTreeMap<String, usize> = BTreeMap::new();
    let mut other_messages: BTreeMap<String, usize> = BTreeMap::new();
    let mut corpus_definitions: BTreeSet<String> = BTreeSet::new();
    let mut blocked_by: BTreeMap<&'static str, usize> = BTreeMap::new();

    for entry in entries
        .iter()
        .filter(|entry| entry.name.to_ascii_lowercase().ends_with(".gs"))
    {
        let Ok(bytes) = archive.read(&entry.name) else {
            continue;
        };
        let Ok(document) = GameScriptDocument::parse(&bytes) else {
            continue;
        };
        for name in document.analyze().definition_names.keys() {
            corpus_definitions.insert(name.to_ascii_lowercase());
        }
    }

    for entry in entries
        .iter()
        .filter(|entry| entry.name.to_ascii_lowercase().ends_with(".gs"))
    {
        let Ok(bytes) = archive.read(&entry.name) else {
            unparsable += 1;
            continue;
        };
        let Ok(document) = GameScriptDocument::parse(&bytes) else {
            unparsable += 1;
            continue;
        };
        let mut vm = GameScriptVm::new(STEP_LIMIT);
        match vm.execute_document(&document) {
            Ok(()) => loaded += 1,
            Err(error) => match error.unknown_name() {
                Some(trace) => {
                    stopped_on_name += 1;
                    *blocked_by
                        .entry(blocker_class(&trace.name, &corpus_definitions, operators))
                        .or_default() += 1;
                    *blockers.entry(trace.name).or_default() += 1;
                }
                None => {
                    other_failure += 1;
                    *other_messages.entry(error.message).or_default() += 1;
                }
            },
        }
    }

    println!("survey-members-loaded\t{loaded}");
    println!("survey-members-stopped-on-a-name\t{stopped_on_name}");
    println!("survey-members-failed-otherwise\t{other_failure}");
    println!("survey-members-unreadable\t{unparsable}");
    println!("survey-distinct-blocking-names\t{}", blockers.len());
    for (class, count) in &blocked_by {
        println!("survey-first-blocker-class\t{class}\t{count}");
    }

    let mut ranked: Vec<(&String, &usize)> = blockers.iter().collect();
    ranked.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
    for (name, count) in ranked.iter().take(30) {
        let class = blocker_class(name, &corpus_definitions, operators);
        println!("survey-blocker\t{name}\t{count}\t{class}");
    }

    let mut ranked: Vec<(&String, &usize)> = other_messages.iter().collect();
    ranked.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
    for (message, count) in ranked.iter().take(10) {
        println!("survey-other-failure\t{message}\t{count}");
    }
}

/// Run one expression against the loaded module and render whatever it leaves.
fn evaluate(vm: &mut GameScriptVm, source: &str) -> Result<String, GameScriptVmError> {
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

/// What kind of thing a member's first unresolved name is, which is what decides whether module
/// loading would unblock it.
///
/// `defined-in-another-member` comes first deliberately: a name some other `.gs` defines is one a
/// loader resolves for free, whatever the engine also happens to call it.
fn blocker_class(
    name: &str,
    corpus_definitions: &BTreeSet<String>,
    operators: Option<&OperatorIndex>,
) -> &'static str {
    if corpus_definitions.contains(&name.to_ascii_lowercase()) {
        return "defined-in-another-member";
    }
    match operators.map(|index| index.classify(name)) {
        Some(NameClass::Operator { .. }) => "engine-operator",
        Some(NameClass::EngineConstant) => "engine-constant",
        Some(NameClass::Unresolved) => "unresolved",
        None => "unclassified",
    }
}
