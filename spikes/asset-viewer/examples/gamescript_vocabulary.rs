//! Partition a profile's GameScript vocabulary into what each name actually is.
//!
//! The scanner's "likely hardcoded engine name" list is a heuristic: a name the corpus calls, never
//! defines, and which also appears as an ASCII string somewhere in `lomse.exe`. That last clause
//! is a coincidence filter, not a proof, and it admits roughly two thousand names per profile.
//!
//! With the engine's operator table recovered, the question stops being a guess. Every name falls
//! into exactly one class, in this order, which is the order the interpreter itself resolves in:
//!
//! 1. `script-definition` -- the corpus defines it, so a dictionary lookup finds it first.
//! 2. `language-primitive` -- the VM in `gamescript_vm` implements it as language, not game state.
//! 3. `native-host-call` -- `lomse.exe` registers it. Running dependent script needs the engine.
//! 4. `constant-or-data` -- SCREAMING_CASE and absent from the operator table.
//! 5. `heuristic-false-positive` -- none of the above: the name is called, nothing defines it, the
//!    engine does not implement it, and it is not constant-shaped.
//!
//! Usage:
//!
//! ```text
//! cargo run --example gamescript_vocabulary -- \
//!     --profile LABEL --gs PATH/gs.mpq --exe PATH/lomse.exe [more profiles] [--out DIR]
//! ```
//!
//! Only counts, names and classes are emitted. No `.gs` contents leave the archive.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use lom_asset_viewer::gamescript::GameScriptDocument;
use lom_asset_viewer::gamescript_vm::is_primitive;
use lom_asset_viewer::mpq::Archive;
use lom_asset_viewer::native_table::{NameClass, OperatorIndex, is_screaming_case};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    ScriptDefinition,
    LanguagePrimitive,
    NativeHostCall,
    ConstantOrData,
    HeuristicFalsePositive,
}

impl Class {
    const ALL: [Self; 5] = [
        Self::ScriptDefinition,
        Self::LanguagePrimitive,
        Self::NativeHostCall,
        Self::ConstantOrData,
        Self::HeuristicFalsePositive,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::ScriptDefinition => "script-definition",
            Self::LanguagePrimitive => "language-primitive",
            Self::NativeHostCall => "native-host-call",
            Self::ConstantOrData => "constant-or-data",
            Self::HeuristicFalsePositive => "heuristic-false-positive",
        }
    }
}

struct Profile {
    label: String,
    archive: PathBuf,
    executable: PathBuf,
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mut profiles: Vec<Profile> = Vec::new();
    let mut output: Option<PathBuf> = None;
    let mut index = 0;
    while index < arguments.len() {
        let value = |index: usize| -> String {
            arguments.get(index + 1).cloned().unwrap_or_else(|| {
                eprintln!("{} needs a value", arguments[index]);
                std::process::exit(2);
            })
        };
        match arguments[index].as_str() {
            "--profile" => profiles.push(Profile {
                label: value(index),
                archive: PathBuf::new(),
                executable: PathBuf::new(),
            }),
            "--gs" => match profiles.last_mut() {
                Some(profile) => profile.archive = PathBuf::from(value(index)),
                None => {
                    eprintln!("--gs must follow a --profile");
                    std::process::exit(2);
                }
            },
            "--exe" => match profiles.last_mut() {
                Some(profile) => profile.executable = PathBuf::from(value(index)),
                None => {
                    eprintln!("--exe must follow a --profile");
                    std::process::exit(2);
                }
            },
            "--out" => output = Some(PathBuf::from(value(index))),
            other => {
                eprintln!("unexpected argument {other}");
                std::process::exit(2);
            }
        }
        index += 2;
    }
    if profiles.is_empty() {
        eprintln!("usage: --profile LABEL --gs PATH/gs.mpq --exe PATH/lomse.exe [...] [--out DIR]");
        std::process::exit(2);
    }

    for profile in &profiles {
        if let Err(message) = classify(profile, output.as_deref()) {
            eprintln!("{}: {message}", profile.label);
            std::process::exit(1);
        }
    }
}

fn classify(profile: &Profile, output: Option<&Path>) -> Result<(), String> {
    let image = std::fs::read(&profile.executable)
        .map_err(|error| format!("could not read {}: {error}", profile.executable.display()))?;
    let operators = OperatorIndex::from_image(&image)
        .map_err(|error| format!("could not read the operator table: {error}"))?;
    let binary_strings = ascii_strings(&image);

    let archive = Archive::open(&profile.archive)
        .map_err(|error| format!("could not open the archive: {error}"))?;
    let entries = archive
        .entries()
        .map_err(|error| format!("could not list the archive: {error}"))?;

    let mut executable_names: BTreeMap<String, usize> = BTreeMap::new();
    let mut definition_names: BTreeSet<String> = BTreeSet::new();
    let mut members = 0_usize;
    let mut failures = 0_usize;

    for entry in entries
        .iter()
        .filter(|entry| entry.name.to_ascii_lowercase().ends_with(".gs"))
    {
        let Ok(bytes) = archive.read(&entry.name) else {
            failures += 1;
            continue;
        };
        let Ok(document) = GameScriptDocument::parse(&bytes) else {
            failures += 1;
            continue;
        };
        members += 1;
        let analysis = document.analyze();
        for (name, count) in &analysis.executable_names {
            *executable_names.entry(name.clone()).or_default() += count;
        }
        for name in analysis.definition_names.keys() {
            definition_names.insert(name.to_ascii_lowercase());
        }
    }

    let mut rows: Vec<(String, usize, Class, bool, Option<u32>)> = Vec::new();
    for (name, uses) in &executable_names {
        let lower = name.to_ascii_lowercase();
        let operator_class = operators.classify(name);
        let entry_point = match operator_class {
            NameClass::Operator { entry_point } => Some(entry_point),
            _ => None,
        };
        let class = if definition_names.contains(&lower) {
            Class::ScriptDefinition
        } else if is_primitive(name) {
            Class::LanguagePrimitive
        } else if entry_point.is_some() {
            Class::NativeHostCall
        } else if is_screaming_case(name) {
            Class::ConstantOrData
        } else {
            Class::HeuristicFalsePositive
        };
        // The scanner's existing broad heuristic, reproduced so the two can be compared directly.
        let candidate = !definition_names.contains(&lower) && binary_strings.contains(&lower);
        rows.push((name.clone(), *uses, class, candidate, entry_point));
    }

    let mut totals: BTreeMap<Class, usize> = BTreeMap::new();
    let mut candidate_totals: BTreeMap<Class, usize> = BTreeMap::new();
    for (_, _, class, candidate, _) in &rows {
        *totals.entry(*class).or_default() += 1;
        if *candidate {
            *candidate_totals.entry(*class).or_default() += 1;
        }
    }

    // Names the corpus defines that the engine ALSO implements. The dictionary wins at run time,
    // so these are script overrides of engine behaviour and worth naming explicitly.
    let shadowed: Vec<&String> = rows
        .iter()
        .filter(|(_, _, class, _, entry_point)| {
            *class == Class::ScriptDefinition && entry_point.is_some()
        })
        .map(|(name, _, _, _, _)| name)
        .collect();
    let shadowed_primitives: Vec<&String> = rows
        .iter()
        .filter(|(name, _, class, _, _)| *class == Class::ScriptDefinition && is_primitive(name))
        .map(|(name, _, _, _, _)| name)
        .collect();

    println!("profile\t{}", profile.label);
    println!("parsed-members\t{members}");
    println!("parse-failures\t{failures}");
    println!("operator-table-entries\t{}", operators.len());
    println!("distinct-executable-names\t{}", rows.len());
    println!(
        "broad-candidates\t{}",
        rows.iter().filter(|row| row.3).count()
    );
    for class in Class::ALL {
        println!(
            "class\t{}\t{}\t{}",
            class.label(),
            totals.get(&class).copied().unwrap_or(0),
            candidate_totals.get(&class).copied().unwrap_or(0)
        );
    }
    println!(
        "script-definitions-shadowing-an-operator\t{}",
        shadowed.len()
    );
    for name in &shadowed {
        println!("shadowed-operator\t{name}");
    }
    println!(
        "script-definitions-shadowing-a-primitive\t{}",
        shadowed_primitives.len()
    );
    for name in &shadowed_primitives {
        println!("shadowed-primitive\t{name}");
    }

    if let Some(directory) = output {
        std::fs::create_dir_all(directory)
            .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
        let path = directory.join(format!("vocabulary-{}.tsv", profile.label));
        let mut text = String::from("name\tuses\tclass\tbroad-candidate\toperator-entry-point\n");
        for (name, uses, class, candidate, entry_point) in &rows {
            let entry_point = entry_point
                .map(|value| format!("{value:#010x}"))
                .unwrap_or_else(|| "-".to_owned());
            text.push_str(&format!(
                "{name}\t{uses}\t{}\t{}\t{entry_point}\n",
                class.label(),
                if *candidate { "yes" } else { "no" }
            ));
        }
        std::fs::write(&path, text)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        println!("wrote\t{}", path.display());
    }

    Ok(())
}

/// Printable ASCII runs in the executable, lowercased. The scanner's coincidence filter.
fn ascii_strings(source: &[u8]) -> BTreeSet<String> {
    source
        .split(|byte| !(0x20..=0x7e).contains(byte))
        .filter(|bytes| bytes.len() >= 2)
        .map(|bytes| String::from_utf8_lossy(bytes).to_ascii_lowercase())
        .collect()
}
