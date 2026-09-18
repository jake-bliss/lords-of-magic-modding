//! Run the `gs\standard.gs` battery and trace what the engine still owes us.
//!
//! The exercise table itself lives in `lom_asset_viewer::gamescript_standard`, so that
//! `tests/gamescript_standard.rs` can run it under `cargo test`. It used to live here, which meant
//! it ran only when a person typed this command -- 32 exercises that no test suite touched. This
//! file is now a reporting driver over library code, and `--survey` is the part that has no test
//! equivalent because it is a census rather than an assertion.
//!
//! Usage:
//!
//! ```text
//! cargo run --example gamescript_standard -- --gs PATH/gs.mpq [--exe PATH/lomse.exe] [--survey]
//! ```
//!
//! Exit status is non-zero if any exercise disagrees with its expectation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use lom_asset_viewer::gamescript::GameScriptDocument;
use lom_asset_viewer::gamescript_standard::{
    EXERCISES, MEMBER, STEP_LIMIT, load_module, run_exercises,
};
use lom_asset_viewer::gamescript_vm::GameScriptVm;
use lom_asset_viewer::mpq::Archive;
use lom_asset_viewer::native_table::{NameClass, OperatorIndex};

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

    let (mut vm, document) = load_module(&bytes)?;
    let analysis = document.analyze();

    println!("member\t{MEMBER}");
    println!("source-bytes\t{}", bytes.len());
    println!("tokens\t{}", analysis.token_count);
    println!("comments\t{}", analysis.comment_count);
    println!("procedure-anomalies\t{}", analysis.procedure_anomaly_count);

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

    for (exercise, outcome) in EXERCISES.iter().zip(run_exercises(&mut vm)) {
        if let Some(trace) = &outcome.unknown_name {
            *unknown_names.entry(trace.name.clone()).or_default() += 1;
            unknown_frames
                .entry(trace.name.clone())
                .or_insert_with(|| trace.call_stack.clone());
        }
        match &outcome.disagreement {
            None => println!("exercise\t{}\tok", exercise.name),
            Some(disagreement) => {
                failures += 1;
                println!("exercise\t{}\tDISAGREES\t{disagreement}", exercise.name);
            }
        }
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
    let mut line_endings = LineEndingCensus::default();
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
        line_endings.record(&bytes);
        for name in document.analyze().definition_names.keys() {
            // Case-sensitively. The VM resolves names case-sensitively -- `lookup` builds a
            // `DictKey::Name` from the name as written -- so folding here claimed that a loader
            // would resolve `GOLD` because `gs\barter.gs` defines `/gold`. It would not, and that
            // one fold put 54 constant names and 75 members in the wrong class.
            corpus_definitions.insert(name.clone());
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

    line_endings.report();
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

/// What kind of thing a member's first unresolved name is, which is what decides whether module
/// loading would unblock it.
///
/// `defined-in-another-member` comes first deliberately: a name some other `.gs` defines is one a
/// loader resolves for free, whatever the engine also happens to call it.
///
/// The match is **case-sensitive**, because the VM's own name resolution is. It did not used to be,
/// and the difference is not cosmetic: nothing in 3.02 defines `GOLD`, `gs\barter.gs` defines
/// `/gold`, and the fold moved 54 engine-constant names -- 75 members by `GOLD` alone -- into
/// `defined-in-another-member`. That was the single number the continue recommendation rested on.
fn blocker_class(
    name: &str,
    corpus_definitions: &BTreeSet<String>,
    operators: Option<&OperatorIndex>,
) -> &'static str {
    if corpus_definitions.contains(name) {
        return "defined-in-another-member";
    }
    match operators.map(|index| index.classify(name)) {
        Some(NameClass::Operator { .. }) => "engine-operator",
        Some(NameClass::EngineConstant) => "engine-constant",
        Some(NameClass::Unresolved) => "unresolved",
        None => "unclassified",
    }
}

/// How the archive's members terminate their lines.
///
/// These are **overlapping** counts, not a partition: a member may hold CRLF and bare CR and bare
/// LF at once, and 193 of 3.02's bare-CR members also hold CRLF. Reporting them as if they
/// partitioned the archive is what made an earlier version of the documentation sum to more
/// members than exist. `bare-cr-only` is the subset for which the lexer's line counter is the
/// *only* thing standing between a reader and a wrong line number.
#[derive(Default)]
struct LineEndingCensus {
    members: usize,
    with_crlf: usize,
    with_bare_cr: usize,
    with_bare_lf: usize,
    bare_cr_only: usize,
    without_any: usize,
}

impl LineEndingCensus {
    fn record(&mut self, bytes: &[u8]) {
        self.members += 1;
        let mut crlf = false;
        let mut bare_cr = false;
        let mut bare_lf = false;
        for (index, byte) in bytes.iter().enumerate() {
            match byte {
                b'\r' if bytes.get(index + 1) == Some(&b'\n') => crlf = true,
                b'\r' => bare_cr = true,
                b'\n' if index == 0 || bytes[index - 1] != b'\r' => bare_lf = true,
                _ => {}
            }
        }
        self.with_crlf += usize::from(crlf);
        self.with_bare_cr += usize::from(bare_cr);
        self.with_bare_lf += usize::from(bare_lf);
        self.bare_cr_only += usize::from(bare_cr && !crlf && !bare_lf);
        self.without_any += usize::from(!crlf && !bare_cr && !bare_lf);
    }

    fn report(&self) {
        println!("line-endings-members\t{}", self.members);
        println!("line-endings-containing-crlf\t{}", self.with_crlf);
        println!("line-endings-containing-bare-cr\t{}", self.with_bare_cr);
        println!("line-endings-containing-bare-lf\t{}", self.with_bare_lf);
        println!("line-endings-bare-cr-only\t{}", self.bare_cr_only);
        println!("line-endings-without-any\t{}", self.without_any);
    }
}
