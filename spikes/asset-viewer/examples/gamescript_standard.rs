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
//! cargo run --example gamescript_standard -- --gs PATH/gs.mpq [--exe PATH/lomse.exe] \
//!     [--survey] [--preload MEMBER]...
//! ```
//!
//! Exit status is non-zero if any exercise disagrees with its expectation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use lom_asset_viewer::gamescript::GameScriptDocument;
use lom_asset_viewer::gamescript_standard::{
    EXERCISES, Expected, Lineage, MEMBER, STEP_LIMIT, load_module, run_exercises,
};
use lom_asset_viewer::gamescript_vm::{
    GameScriptVm, ModuleSource, NameResolution, normalize_module_path,
};
use lom_asset_viewer::mpq::Archive;
use lom_asset_viewer::native_table::{NameClass, OperatorIndex};

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mut archive_path: Option<PathBuf> = None;
    let mut executable_path: Option<PathBuf> = None;
    let mut survey = false;
    let mut preload: Vec<String> = Vec::new();
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
            // A declared input, exactly as `--stub` is: these members are executed into each
            // survey machine before the member under test, standing in for the load order the
            // engine performs from `START.GS`. Nothing is preloaded unless it is named here.
            "--preload" => {
                index += 1;
                match arguments.get(index) {
                    Some(member) => preload.push(member.clone()),
                    None => {
                        eprintln!("--preload needs a member name");
                        std::process::exit(2);
                    }
                }
            }
            other => {
                eprintln!("unexpected argument {other}");
                eprintln!("usage: --gs PATH/gs.mpq [--exe PATH/lomse.exe] [--survey] [--preload MEMBER]...");
                std::process::exit(2);
            }
        }
        index += 1;
    }
    let Some(archive_path) = archive_path else {
        eprintln!("usage: --gs PATH/gs.mpq [--exe PATH/lomse.exe] [--survey] [--preload MEMBER]...");
        std::process::exit(2);
    };

    match run(&archive_path, executable_path.as_deref(), survey, &preload) {
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

fn run(
    archive_path: &Path,
    executable_path: Option<&Path>,
    survey: bool,
    preload: &[String],
) -> Result<usize, String> {
    let operators = executable_path
        .map(|path| {
            let bytes = std::fs::read(path)
                .map_err(|error| format!("could not read {}: {error}", path.display()))?;
            OperatorIndex::from_image(&bytes)
                .map_err(|error| format!("could not read the operator table: {error}"))
        })
        .transpose()?;

    let archive = Rc::new(
        Archive::open(archive_path)
            .map_err(|error| format!("could not open the archive: {error}"))?,
    );
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

    let (lineage, evidence) = Lineage::derive(&bytes)?;
    println!("lineage\t{}", lineage.label());
    println!("lineage-evidence\t{evidence:?}");
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

    for (exercise, outcome) in EXERCISES.iter().zip(run_exercises(&mut vm, lineage)) {
        if let Some(trace) = &outcome.unknown_name {
            *unknown_names.entry(trace.name.clone()).or_default() += 1;
            unknown_frames
                .entry(trace.name.clone())
                .or_insert_with(|| trace.call_stack.clone());
        }
        // The *resolved* expectation is printed alongside the verdict. Without it an `ok` on an
        // archive that does not ship `string_cvi` is indistinguishable from an `ok` on one that
        // does -- the first stopped on an undefined name and the second computed `1234 -42`, and
        // a transcript that records both as bare `ok` has lost the thing it exists to witness.
        let expected = match lineage.expected_for(exercise) {
            Expected::Stack(stack) => format!("leaves [{stack}]"),
            Expected::StopsOn(name) => format!("stops on {name}"),
        };
        match &outcome.disagreement {
            None => println!("exercise\t{}\tok\t{expected}", exercise.name),
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
        survey_module_loads(&archive, &entries, operators.as_ref(), preload);
    }

    Ok(failures)
}

/// Read modules for `run` out of the archive, read-only.
///
/// The archive is the only place this looks, and it never writes. Members are named with `\` and
/// `run` targets are written with `/`, and the archive matches case-insensitively, so the index is
/// built on the folded form and the archive's own spelling is what gets read.
struct ArchiveModules {
    archive: Rc<Archive>,
    members: BTreeMap<String, String>,
}

impl ArchiveModules {
    fn new(archive: Rc<Archive>, entries: &[lom_asset_viewer::mpq::Entry]) -> Self {
        let members = entries
            .iter()
            .map(|entry| (normalize_module_path(&entry.name), entry.name.clone()))
            .collect();
        Self { archive, members }
    }
}

impl ModuleSource for ArchiveModules {
    fn load(&self, path: &str) -> Result<Vec<u8>, String> {
        let name = self
            .members
            .get(&normalize_module_path(path))
            .ok_or_else(|| format!("the archive has no member {path}"))?;
        self.archive.read(name).map_err(|error| error.to_string())
    }
}

/// The corpus-wide execution census: how much of the shipped script set this VM can run.
///
/// Each member gets a fresh machine, so a member that stops cannot corrupt the next one's result.
/// Three declared inputs change what "can run" means, and each is reported beside the numbers it
/// produced rather than folded into them:
///
/// - a **module source**, so `"gs/standard.gs" run` resolves inside the VM;
/// - the **preload list**, executed into each machine before the member under test;
/// - the **opaque constant set**, taken from the engine's own operator tables (SCREAMING_CASE and
///   absent from them) and available only when `--exe` was supplied. Without it no name becomes a
///   constant and the run is the stricter one.
fn survey_module_loads(
    archive: &Rc<Archive>,
    entries: &[lom_asset_viewer::mpq::Entry],
    operators: Option<&OperatorIndex>,
    preload: &[String],
) {
    let mut loaded = 0_usize;
    let mut stopped_on_name = 0_usize;
    let mut other_failure = 0_usize;
    let mut unparsable = 0_usize;
    let mut blockers: BTreeMap<String, usize> = BTreeMap::new();
    let mut other_messages: BTreeMap<String, usize> = BTreeMap::new();
    let mut line_endings = LineEndingCensus::default();
    let mut corpus_definitions: BTreeSet<String> = BTreeSet::new();
    // Which members define each name, so "another member defines it" can be turned into "and
    // *that* member runs" -- the question that decides whether module loading actually pays.
    let mut definition_sites: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut members_that_load: BTreeSet<String> = BTreeSet::new();
    // Every executable name the corpus mentions anywhere, whether or not execution ever reached
    // it. This is what keeps the "never reached" figure honest: a survey that stops each member at
    // its first unresolved name understates dynamic reach, so dynamic reach alone would overstate
    // how much engine surface the corpus does not use.
    let mut static_names: BTreeSet<String> = BTreeSet::new();
    let mut constant_names: BTreeSet<String> = BTreeSet::new();
    let mut reached: BTreeMap<String, BTreeSet<NameResolution>> = BTreeMap::new();

    let members: Vec<&lom_asset_viewer::mpq::Entry> = entries
        .iter()
        .filter(|entry| entry.name.to_ascii_lowercase().ends_with(".gs"))
        .collect();

    for entry in &members {
        let Ok(bytes) = archive.read(&entry.name) else {
            continue;
        };
        let Ok(document) = GameScriptDocument::parse(&bytes) else {
            continue;
        };
        line_endings.record(&bytes);
        let analysis = document.analyze();
        for name in analysis.definition_names.keys() {
            // Case-sensitively. The VM resolves names case-sensitively -- `lookup` builds a
            // `DictKey::Name` from the name as written -- so folding here claimed that a loader
            // would resolve `GOLD` because `gs\barter.gs` defines `/gold`. It would not, and that
            // one fold put 54 constant names and 75 members in the wrong class.
            corpus_definitions.insert(name.clone());
            definition_sites
                .entry(name.clone())
                .or_default()
                .push(entry.name.clone());
        }
        for name in analysis.executable_names.keys() {
            static_names.insert(name.clone());
        }
    }

    // A constant is declared only where the engine's own tables say the name is not an operator.
    // Without `--exe` there is no such evidence, so nothing is declared and the survey runs
    // stricter -- never looser -- than it would with the binary in hand.
    if let Some(operators) = operators {
        for name in &static_names {
            if !corpus_definitions.contains(name)
                && matches!(operators.classify(name), NameClass::EngineConstant)
            {
                constant_names.insert(name.clone());
            }
        }
    }

    let modules: Rc<dyn ModuleSource> =
        Rc::new(ArchiveModules::new(Rc::clone(archive), entries));

    let mut preloaded: Vec<(String, Vec<u8>)> = Vec::new();
    for member in preload {
        match modules.load(member) {
            Ok(bytes) => preloaded.push((member.clone(), bytes)),
            Err(error) => eprintln!("preload {member} unreadable: {error}"),
        }
    }

    for entry in &members {
        let Ok(bytes) = archive.read(&entry.name) else {
            unparsable += 1;
            continue;
        };
        let Ok(document) = GameScriptDocument::parse(&bytes) else {
            unparsable += 1;
            continue;
        };
        let mut vm = GameScriptVm::new(STEP_LIMIT);
        vm.set_module_source(Rc::clone(&modules));
        vm.declare_opaque_constants(constant_names.iter().cloned());
        for (_, prelude) in &preloaded {
            if let Ok(prelude) = GameScriptDocument::parse(prelude) {
                // A prelude that stops part-way still leaves behind every definition it
                // reached, which is the point of preloading. Operands are dropped so its debris
                // cannot be mistaken for the member's own result; the dictionary stack is left
                // alone, because a module that ends inside an unmatched `begin` is how the corpus
                // publishes a dictionary — see `GameScriptVm::discard_operands`.
                let _ = vm.execute_document(&prelude);
                vm.discard_operands();
            }
        }
        let outcome = vm.execute_document(&document);
        for (name, resolution) in vm.reaches().keys() {
            reached
                .entry(name.clone())
                .or_default()
                .insert(*resolution);
        }
        match outcome {
            Ok(()) => {
                loaded += 1;
                members_that_load.insert(entry.name.clone());
            }
            Err(error) => match error.unknown_name() {
                Some(trace) => {
                    stopped_on_name += 1;
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
    println!("survey-members\t{}", members.len());
    println!("survey-members-loaded\t{loaded}");
    println!("survey-members-stopped-on-a-name\t{stopped_on_name}");
    println!("survey-members-failed-otherwise\t{other_failure}");
    println!("survey-members-unreadable\t{unparsable}");
    println!("survey-preloaded-members\t{}", preloaded.len());
    for (member, bytes) in &preloaded {
        println!("survey-preloaded-member\t{member}\t{}", bytes.len());
    }
    println!("survey-declared-opaque-constants\t{}", constant_names.len());
    println!("survey-distinct-blocking-names\t{}", blockers.len());

    // Classified at report time, not as each member stops: the most useful class --
    // "another member defines it *and that member runs*" -- is not knowable until every member
    // has been attempted. The class is a function of the name alone, so deriving it from the
    // name/count table loses nothing.
    let class_of = |name: &str| -> &'static str {
        let class = blocker_class(name, &corpus_definitions, operators);
        if class != "defined-in-another-member" {
            return class;
        }
        match definition_sites.get(name) {
            Some(sites) if sites.iter().any(|site| members_that_load.contains(site)) => {
                "defined-in-a-member-that-runs"
            }
            _ => "defined-in-a-member-that-does-not-run-either",
        }
    };
    let mut blocked_by: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (name, count) in &blockers {
        *blocked_by.entry(class_of(name)).or_default() += count;
    }
    for (class, count) in &blocked_by {
        println!("survey-first-blocker-class\t{class}\t{count}");
    }

    let mut ranked: Vec<(&String, &usize)> = blockers.iter().collect();
    ranked.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
    for (name, count) in ranked.iter().take(30) {
        println!("survey-blocker\t{name}\t{count}\t{}", class_of(name));
    }

    let mut ranked: Vec<(&String, &usize)> = other_messages.iter().collect();
    ranked.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
    for (message, count) in ranked.iter().take(10) {
        println!("survey-other-failure\t{message}\t{count}");
    }

    report_reach_census(&reached, &static_names, operators);
}

/// How much of the engine's operator surface the corpus reaches, and what happened when it did.
///
/// The three denominators are deliberately separate. An operator no script names is not a gap in
/// this VM; an operator scripts name but execution never reaches is a gap this survey cannot see
/// past; an operator execution did reach is the only one whose status is measured.
fn report_reach_census(
    reached: &BTreeMap<String, BTreeSet<NameResolution>>,
    static_names: &BTreeSet<String>,
    operators: Option<&OperatorIndex>,
) {
    let mut by_resolution: BTreeMap<&'static str, usize> = BTreeMap::new();
    for resolutions in reached.values() {
        for resolution in resolutions {
            *by_resolution.entry(resolution.label()).or_default() += 1;
        }
    }
    println!("reach-distinct-names\t{}", reached.len());
    for (label, count) in &by_resolution {
        println!("reach-names-by-resolution\t{label}\t{count}");
    }

    let Some(operators) = operators else {
        return;
    };
    let table: BTreeSet<String> = operators.names().map(str::to_owned).collect();
    let fold = |names: &BTreeSet<String>| -> BTreeSet<String> {
        names.iter().map(|name| name.to_ascii_lowercase()).collect()
    };
    let static_folded = fold(static_names);
    let reached_folded: BTreeSet<String> = reached
        .keys()
        .map(|name| name.to_ascii_lowercase())
        .collect();

    let named_statically: BTreeSet<&String> = table.intersection(&static_folded).collect();
    let reached_dynamically: BTreeSet<&String> = table.intersection(&reached_folded).collect();

    println!("reach-operators-in-table\t{}", table.len());
    println!(
        "reach-operators-never-named-by-any-script\t{}",
        table.len() - named_statically.len()
    );
    println!(
        "reach-operators-named-by-a-script\t{}",
        named_statically.len()
    );
    println!(
        "reach-operators-reached-by-execution\t{}",
        reached_dynamically.len()
    );

    // Of the operators execution actually reached, what resolved them. An operator is `blocked`
    // only when nothing resolved it on any path; a script that defines a name the engine also
    // registers shadows it, and the dictionary wins at run time.
    let mut implemented = 0_usize;
    let mut shadowed = 0_usize;
    let mut stubbed = 0_usize;
    let mut blocked = 0_usize;
    for name in &reached_dynamically {
        let Some(resolutions) = reached
            .iter()
            .find(|(candidate, _)| candidate.to_ascii_lowercase() == ***name)
            .map(|(_, resolutions)| resolutions)
        else {
            continue;
        };
        if resolutions.contains(&NameResolution::Primitive) {
            implemented += 1;
        } else if resolutions.contains(&NameResolution::Stub) {
            stubbed += 1;
        } else if resolutions.contains(&NameResolution::Definition)
            || resolutions.contains(&NameResolution::ProcedureLocal)
        {
            shadowed += 1;
        } else {
            blocked += 1;
        }
    }
    println!("reach-operators-implemented\t{implemented}");
    println!("reach-operators-stubbed\t{stubbed}");
    println!("reach-operators-shadowed-by-a-script\t{shadowed}");
    println!("reach-operators-blocked\t{blocked}");
}

/// What kind of thing a member's first unresolved name is, which is what decides whether module
/// loading would unblock it.
///
/// `defined-in-another-member` comes first deliberately: a name some other `.gs` defines is one a
/// loader resolves for free, whatever the engine also happens to call it.
///
/// **"For free" turned out to be wrong**, which is why the caller splits this class again by
/// whether the defining member itself runs. `gs\tree.gs` defines `terrainsprites` and stops at its
/// own **step 2** on `maxterrainspritetypes`; `units\easyunit.gs` defines `begin_unit_definition`
/// and stops at step 1 on `userdict`. Measured 2026-09-18.
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
