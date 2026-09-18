//! Recover the engine's in-memory state model from the operator bodies.
//!
//! ```text
//! cargo run --release --example operator_state -- <lomse.exe> [--out DIRECTORY] [--depth N]
//! cargo run --release --example operator_state -- <lomse.exe> --base 0x005aa12c
//! ```
//!
//! Without `--out` the summary goes to stdout and nothing is written. With it, five tables are
//! produced: the per-operator access map, the candidate structures, their fields, the
//! two-instrument agreement on static objects, and the subject-word convergence that is the only
//! material behind any *name* for a structure.
//!
//! Every address, offset, width and count is derived analysis. No bytes and no strings of the
//! binary are written.

use std::collections::BTreeSet;
use std::path::PathBuf;

use lom_asset_viewer::engine_state::{self, DEFAULT_DEPTH, StateModel, Structure};
use lom_asset_viewer::native_table::{self, PeImage};
use lom_asset_viewer::operator_bodies::{self, BaseKind};

/// Depths the summary prints the join's size at, so the chosen depth can be judged.
const DEPTH_SWEEP: [usize; 6] = [0, 1, 2, 3, 4, 5];

/// Structures below this many operators are noise in the summary; they stay in the full tables.
const REPORT_MINIMUM: usize = 8;

fn main() -> Result<(), String> {
    let mut arguments = std::env::args().skip(1);
    let executable = arguments
        .next()
        .ok_or("usage: operator_state <lomse.exe> [--out DIRECTORY] [--depth N] [--base ADDR]")?;
    let mut out: Option<PathBuf> = None;
    let mut depth = DEFAULT_DEPTH;
    let mut base: Option<u32> = None;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--out" => out = arguments.next().map(PathBuf::from),
            "--depth" => {
                depth = arguments
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or("--depth takes a call depth")?;
            }
            "--base" => {
                base = arguments.next().and_then(|value| {
                    u32::from_str_radix(value.trim_start_matches("0x"), 16).ok()
                });
            }
            other => return Err(format!("unrecognised argument {other}")),
        }
    }

    let bytes = std::fs::read(&executable).map_err(|error| format!("{executable}: {error}"))?;
    let image = PeImage::parse(&bytes).map_err(|error| error.to_string())?;
    let runs = native_table::extract(&bytes).map_err(|error| error.to_string())?;

    let mut seen = BTreeSet::new();
    let mut operators = Vec::new();
    for entry in runs.iter().flat_map(|run| run.entries.iter()) {
        let name = entry.name.to_ascii_lowercase();
        if seen.insert(name.clone()) {
            operators.push((name, entry.entry_point));
        }
    }

    let analysis = operator_bodies::analyse(&image, &operators).map_err(|error| error.to_string())?;
    let model = engine_state::build(&analysis, &analysis.bodies, depth);
    let structures = engine_state::structures(&model);

    if let Some(address) = base {
        print!("{}", describe(&model, &structures, address));
        return Ok(());
    }

    let curve = engine_state::reach_curve(&analysis, &analysis.bodies, &DEPTH_SWEEP);
    let agreement = engine_state::agreement(&model, &structures);
    let summary = summarise(&model, &structures, &curve, &agreement);
    match out {
        None => print!("{summary}"),
        Some(directory) => {
            std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
            write(
                &directory.join("operator-field-access.tsv"),
                &engine_state::access_table(&model),
            )?;
            write(
                &directory.join("structures.tsv"),
                &engine_state::structure_table(&structures),
            )?;
            write(
                &directory.join("structure-fields.tsv"),
                &engine_state::field_table(&structures),
            )?;
            write(
                &directory.join("static-object-agreement.tsv"),
                &engine_state::agreement_table(&agreement),
            )?;
            write(
                &directory.join("subject-convergence.tsv"),
                &engine_state::subject_convergence(&model, &structures, REPORT_MINIMUM),
            )?;
            write(&directory.join("state-summary.md"), &summary)?;
            print!("{summary}");
        }
    }
    Ok(())
}

fn write(path: &std::path::Path, contents: &str) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|error| format!("{}: {error}", path.display()))
}

/// Print one structure's field map, for inspecting a base the summary only counts.
fn describe(model: &StateModel, structures: &[Structure], base: u32) -> String {
    let mut text = String::new();
    for structure in structures.iter().filter(|s| s.base == base) {
        text.push_str(&format!(
            "{:#010x} ({}) — {} operators, {} fields, observed extent {:#x}\n",
            structure.base,
            structure.kind.label(),
            structure.operators.len(),
            structure.fields.len(),
            structure.observed_extent,
        ));
        for field in structure.fields.values() {
            let widths: Vec<String> = field.widths.iter().map(u8::to_string).collect();
            text.push_str(&format!(
                "  +{:#06x}  w={:8} {:10} depth {}  {} readers, {} writers\n",
                field.offset,
                widths.join("|"),
                if field.indexed { "indexed" } else { "scalar" },
                field.depth,
                field.readers.len(),
                field.writers.len(),
            ));
        }
        let evidence = structure.name_evidence(&engine_state::SUBJECT_VOCABULARY);
        text.push_str(&format!("  name evidence (inferred): {evidence:?}\n"));
    }
    if text.is_empty() {
        text.push_str(&format!("{base:#010x}: no structure recovered at this base\n"));
        let _ = model;
    }
    text
}

fn summarise(
    model: &StateModel,
    structures: &[Structure],
    curve: &[engine_state::ReachRow],
    agreement: &[engine_state::Agreement],
) -> String {
    let mut text = String::new();
    text.push_str(
        "# The engine's state model, from the operator bodies\n\nGenerated by `cargo run --release --example operator_state`.\nEvery base, offset, width and count below is **observed in a local binary**. The attribution of\na callee's `[this+n]` to the object its caller named is stated in `engine_state`'s module docs and\nis the one assumption. Every *name* for a structure is **inferred** and is left to the reader.\n\n",
    );

    let coverage = &model.coverage;
    text.push_str(&format!(
        "## Coverage, and what this instrument cannot reach\n\n| | |\n| --- | ---: |\n| operators | {} |\n| join depth used | {} |\n| operators with at least one recovered field | {} |\n| operators with none | {} |\n| call sites whose object was named | {} |\n| **call sites whose object was not named** | **{}** |\n| **indirect calls (destination unknown)** | **{}** |\n| **virtual-dispatch edges named but not followed** | **{}** |\n| **bodies the walk cannot finish** | **{}** |\n\n",
        coverage.operators,
        model.depth,
        coverage.operators - coverage.operators_without_fields,
        coverage.operators_without_fields,
        coverage.tracked_calls,
        coverage.untracked_calls,
        coverage.indirect_calls,
        coverage.virtual_calls,
        coverage.incomplete_bodies,
    ));
    text.push_str("Every bounded negative in this document — \"no operator writes field X\" — is bounded by those\nfour bold rows and by nothing else.\n\n");

    text.push_str("## The saturation curve\n\nHow far the `this` chain is followed, against what it buys. A join that reaches every object from\nevery operator would describe the call graph rather than the operator, which is the failure the\nimport-depth curve in `native-operator-bodies.md` documents.\n\n| depth | operators with fields | distinct bases | distinct fields | accesses |\n| ---: | ---: | ---: | ---: | ---: |\n");
    for row in curve {
        text.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            row.depth,
            row.operators_with_fields,
            row.distinct_bases,
            row.distinct_fields,
            row.accesses
        ));
    }
    text.push('\n');

    let indirect = structures
        .iter()
        .filter(|s| s.kind == BaseKind::Indirect)
        .count();
    let static_count = structures.len() - indirect;
    let fields: usize = structures.iter().map(|s| s.fields.len()).sum();
    text.push_str(&format!(
        "## Candidate structures\n\n| | |\n| --- | ---: |\n| candidate structures | {} |\n| reached through a pointer in `.data` (heap objects) | {indirect} |\n| materialised as an immediate (objects in `.data`) | {static_count} |\n| distinct field offsets recovered | {fields} |\n\n",
        structures.len()
    ));

    text.push_str("The largest, by how many operators reach them. The subject column is **inferred** and is the\nword frequencies of the operator names, not a reading.\n\n| base | kind | operators | fields | written | extent | name evidence (inferred) |\n| --- | --- | ---: | ---: | ---: | ---: | --- |\n");
    for structure in structures.iter().take(24) {
        if structure.operators.len() < REPORT_MINIMUM {
            break;
        }
        let written = structure
            .fields
            .values()
            .filter(|field| !field.writers.is_empty())
            .count();
        let evidence: Vec<String> = structure
            .name_evidence(&engine_state::SUBJECT_VOCABULARY)
            .into_iter()
            .take(4)
            .map(|(token, count)| format!("{token} {count}"))
            .collect();
        text.push_str(&format!(
            "| `{:#010x}` | {} | {} | {} | {written} | `{:#x}` | {} |\n",
            structure.base,
            structure.kind.label(),
            structure.operators.len(),
            structure.fields.len(),
            structure.observed_extent,
            if evidence.is_empty() {
                "—".to_owned()
            } else {
                evidence.join(", ")
            },
        ));
    }
    text.push('\n');

    text.push_str("## The two instruments on statically allocated objects\n\nA field of an object in `.data` can be reached two ways that share no mechanism: through the `this`\njoin, and as a plain absolute address in some other body. Where both see one offset, two independent\ninstruments agree.\n\n| base | range end | interior bases | both | join only | absolute only |\n| --- | --- | ---: | ---: | ---: | ---: |\n");
    for row in agreement.iter().take(12) {
        text.push_str(&format!(
            "| `{:#010x}` | `{:#010x}` | {} | {} | {} | {} |\n",
            row.base,
            row.territory_end,
            row.interior_bases,
            row.both.len(),
            row.join_only.len(),
            row.absolute_only.len()
        ));
    }
    text.push('\n');
    text
}
