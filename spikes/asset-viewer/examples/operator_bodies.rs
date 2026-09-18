//! Walk every native operator body and emit the classification table and its summary.
//!
//! ```text
//! cargo run --release --example operator_bodies -- <lomse.exe> [--out DIRECTORY]
//! ```
//!
//! Without `--out` the summary goes to stdout and nothing is written. With it, three files are
//! produced: the per-operator table, the global-address clusters, and the summary that the docs
//! quote. The tables carry addresses, names and counts — derived analysis — and never the bytes or
//! the strings of the binary.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use lom_asset_viewer::native_table::{self, PeImage};
use lom_asset_viewer::operator_bodies::{
    self, Behaviour, CLASSIFY_DEPTH, GlobalAccess, ImportKind, MAXIMUM_SEARCH_DEPTH,
    OperatorReport,
};

/// Address gap below which two globals are treated as one subject.
///
/// Not fitted to an expected answer: the summary prints the whole sensitivity sweep, and 0x100 is
/// the point on it where the engine's `.data` still resolves into distinguishable subjects. A page
/// merges almost everything into twelve blobs; 0x40 splits single structures apart.
const CLUSTER_GAP: u32 = 0x100;

/// Gaps the summary reports the cluster count for, so the choice above can be judged.
const GAP_SWEEP: [u32; 5] = [0x20, 0x40, 0x100, 0x400, 0x1000];

/// Clusters smaller than this are noise in the summary; they stay in the full table.
const CLUSTER_REPORT_MINIMUM: usize = 12;

fn main() -> Result<(), String> {
    let mut arguments = std::env::args().skip(1);
    let executable = arguments
        .next()
        .ok_or("usage: operator_bodies <lomse.exe> [--out DIRECTORY]")?;
    let mut out: Option<PathBuf> = None;
    let mut gap = CLUSTER_GAP;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--out" => out = arguments.next().map(PathBuf::from),
            "--gap" => {
                gap = arguments
                    .next()
                    .and_then(|value| {
                        u32::from_str_radix(value.trim_start_matches("0x"), 16).ok()
                    })
                    .ok_or("--gap takes a hexadecimal byte count")?;
            }
            other => return Err(format!("unrecognised argument {other}")),
        }
    }

    let bytes = std::fs::read(&executable).map_err(|error| format!("{executable}: {error}"))?;
    let image = PeImage::parse(&bytes).map_err(|error| error.to_string())?;
    let runs = native_table::extract(&bytes).map_err(|error| error.to_string())?;

    // One entry point per name, in table order, matching what `--scan-natives` reports.
    let mut seen = BTreeSet::new();
    let mut operators = Vec::new();
    for entry in runs.iter().flat_map(|run| run.entries.iter()) {
        let name = entry.name.to_ascii_lowercase();
        if seen.insert(name.clone()) {
            operators.push((name, entry.entry_point));
        }
    }

    let analysis = operator_bodies::analyse(&image, &operators).map_err(|error| error.to_string())?;
    let clusters = operator_bodies::cluster_globals(&analysis.reports, gap);

    let summary = summarise(&analysis, &clusters, gap);
    match out {
        None => print!("{summary}"),
        Some(directory) => {
            std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
            write(&directory.join("operator-bodies.tsv"), &operator_table(&analysis.reports))?;
            write(&directory.join("global-clusters.tsv"), &cluster_table(&clusters))?;
            write(&directory.join("summary.md"), &summary)?;
            print!("{summary}");
        }
    }
    Ok(())
}

fn write(path: &std::path::Path, contents: &str) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|error| format!("{}: {error}", path.display()))
}

/// The per-operator table. One row per operator, tab separated, sorted by name so a diff between
/// two runs is a diff of findings rather than of table order.
fn operator_table(reports: &[OperatorReport]) -> String {
    const COLUMNS: [&str; 28] = [
        "name",
        "entry_point",
        "behaviour",
        "boundary",
        "instructions",
        "span_bytes",
        "arity",
        "arity_candidates",
        "nominal_arity",
        "declared_arity",
        "arity_agrees",
        "unbounded",
        "helper_pops",
        "helper_pushes",
        "inline_pops",
        "inline_pushes",
        "calls",
        "call_targets",
        "tail_calls",
        "indirect_calls",
        "globals_read",
        "globals_written",
        "global_addresses",
        "string_refs",
        "import_kinds_at_depth",
        "state_write_depth",
        "calls_mutating_method",
        "direct_imports",
    ];

    let mut rows: Vec<&OperatorReport> = reports.iter().collect();
    rows.sort_by(|left, right| left.name.cmp(&right.name));

    let mut text = COLUMNS.join("\t");
    text.push('\n');
    for report in rows {
        let body = &report.body;
        let reads = body
            .globals
            .iter()
            .filter(|global| global.access != GlobalAccess::Write)
            .count();
        let writes = body
            .globals
            .iter()
            .filter(|global| global.access == GlobalAccess::Write)
            .count();
        let list = |values: Vec<String>| {
            if values.is_empty() {
                "-".to_owned()
            } else {
                values.join(",")
            }
        };
        let number = |value: Option<usize>| {
            value.map_or_else(|| "-".to_owned(), |value| value.to_string())
        };
        let fields: Vec<String> = vec![
            report.name.clone(),
            format!("{:#010x}", report.entry_point),
            report.behaviour.label().to_owned(),
            body.boundary_failure().unwrap_or("complete").to_owned(),
            body.instructions.to_string(),
            body.span_end.saturating_sub(report.entry_point).to_string(),
            number(body.arity),
            list(body.arity_candidates.iter().map(usize::to_string).collect()),
            number(body.nominal_arity()),
            number(report.declared_arity),
            match report.arity_disagreement() {
                None => "yes".to_owned(),
                Some(_) => "NO".to_owned(),
            },
            if body.operand_count_unbounded { "yes" } else { "no" }.to_owned(),
            body.helper_pops.to_string(),
            body.helper_pushes.to_string(),
            body.inline_pops.to_string(),
            body.inline_pushes.to_string(),
            body.calls.len().to_string(),
            list(
                body.calls
                    .iter()
                    .chain(body.tail_calls.iter())
                    .map(|target| format!("{target:#x}"))
                    .collect(),
            ),
            body.tail_calls.len().to_string(),
            body.indirect_calls.to_string(),
            reads.to_string(),
            writes.to_string(),
            list(
                body.globals
                    .iter()
                    .map(|global| format!("{:#x}", global.address))
                    .collect(),
            ),
            body.string_refs.len().to_string(),
            list(
                report
                    .reach
                    .import_depth
                    .iter()
                    .map(|(kind, depth)| format!("{}@{depth}", kind.label()))
                    .collect(),
            ),
            report
                .reach
                .write_depth
                .map_or_else(|| "-".to_owned(), |depth| depth.to_string()),
            if report.reach.calls_mutating_method { "yes" } else { "no" }.to_owned(),
            list(body.direct_imports.iter().cloned().collect()),
        ];
        debug_assert_eq!(fields.len(), COLUMNS.len());
        text.push_str(&fields.join("\t"));
        text.push('\n');
    }
    text
}

fn cluster_table(clusters: &[operator_bodies::GlobalCluster]) -> String {
    let mut text =
        String::from("start\tend\tbytes\tdistinct_addresses\toperators\toperator_names\n");
    for cluster in clusters {
        let mut names: Vec<&str> = cluster.operators.iter().map(String::as_str).collect();
        names.sort_unstable();
        text.push_str(&format!(
            "{:#010x}\t{:#010x}\t{}\t{}\t{}\t{}\n",
            cluster.start,
            cluster.end,
            cluster.end - cluster.start,
            cluster.addresses,
            cluster.operators.len(),
            names.join(","),
        ));
    }
    text
}

fn summarise(
    analysis: &operator_bodies::Analysis,
    clusters: &[operator_bodies::GlobalCluster],
    gap: u32,
) -> String {
    let reports = &analysis.reports;
    let total = reports.len();
    let mut text = String::new();
    text.push_str("# Native operator bodies\n\nGenerated by `cargo run --release --example operator_bodies`.\nEvidence class: **observed in a local binary** for every address, count and call edge below;\nthe reading of what a cluster *is* is **inferred** and marked where it appears.\n\n");

    text.push_str(&format!(
        "## Coverage\n\n| | |\n| --- | ---: |\n| operators walked | {total} |\n| import thunks resolved | {} |\n| distinct call targets in the image | {} |\n| shared operand-fetch helpers | {} |\n| shared result-push helpers | {} |\n\n",
        analysis.index.import_count(),
        analysis.index.function_entry_count(),
        addresses(analysis.index.pop_helpers()),
        addresses(analysis.index.push_helpers()),
    ));

    // Boundary.
    let mut failures: BTreeMap<&str, usize> = BTreeMap::new();
    for report in reports {
        *failures
            .entry(report.body.boundary_failure().unwrap_or("complete"))
            .or_default() += 1;
    }
    let overruns = reports
        .iter()
        .filter(|report| report.overruns_next_entry_point())
        .count();
    text.push_str("## Function boundary\n\n| outcome | operators | share |\n| --- | ---: | ---: |\n");
    for (outcome, count) in &failures {
        text.push_str(&format!(
            "| {outcome} | {count} | {:.1}% |\n",
            percent(*count, total)
        ));
    }
    text.push_str(&format!(
        "\nBodies whose walk ran past the next operator entry point: **{overruns}** ({:.1}%). Operators are not laid out contiguously, so this is an upper bound on boundary failure rather than a count of them.\n\n",
        percent(overruns, total)
    ));

    // Behaviour.
    let mut behaviours: BTreeMap<&str, usize> = BTreeMap::new();
    for report in reports {
        *behaviours.entry(report.behaviour.label()).or_default() += 1;
    }
    text.push_str("## Behaviour class\n\n| class | operators | share |\n| --- | ---: | ---: |\n");
    let mut ordered: Vec<(&&str, &usize)> = behaviours.iter().collect();
    ordered.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
    for (class, count) in ordered {
        text.push_str(&format!(
            "| {class} | {count} | {:.1}% |\n",
            percent(*count, total)
        ));
    }
    let classified = total - behaviours.get("unknown").copied().unwrap_or(0);
    text.push_str(&format!(
        "\nClassified: **{classified}** of {total} ({:.1}%). `unknown` is what an incomplete walk produces, and is left as it is.\n\n",
        percent(classified, total)
    ));

    // Import reach, printed as a curve so saturation is visible instead of assumed.
    text.push_str("## Import reach by call depth\n\nEach cell is the share of operators that reach that kind of import within that many calls. The classifier reads the column at depth ");
    text.push_str(&CLASSIFY_DEPTH.to_string());
    text.push_str(".\n\n| kind |");
    for depth in 0..=MAXIMUM_SEARCH_DEPTH {
        text.push_str(&format!(" {depth} |"));
    }
    text.push_str("\n| --- |");
    for _ in 0..=MAXIMUM_SEARCH_DEPTH {
        text.push_str(" ---: |");
    }
    text.push('\n');
    for kind in ImportKind::ALL {
        let counts: Vec<usize> = (0..=MAXIMUM_SEARCH_DEPTH)
            .map(|depth| {
                reports
                    .iter()
                    .filter(|report| report.reach.reaches(kind, depth))
                    .count()
            })
            .collect();
        if counts.last().copied().unwrap_or(0) == 0 {
            continue;
        }
        text.push_str(&format!("| {} |", kind.label()));
        for count in counts {
            text.push_str(&format!(" {:.0}% |", percent(count, total)));
        }
        text.push('\n');
    }
    let mutating: Vec<usize> = (0..=MAXIMUM_SEARCH_DEPTH)
        .map(|depth| {
            reports
                .iter()
                .filter(|report| report.reach.mutates(depth))
                .count()
        })
        .collect();
    let via_method = reports
        .iter()
        .filter(|report| report.reach.calls_mutating_method)
        .count();
    text.push_str("| *writes engine state* |");
    for count in mutating {
        text.push_str(&format!(" {:.0}% |", percent(count, total)));
    }
    text.push_str(&format!(
        "\n\nSeparately, **{:.0}%** of operators call a method on a singleton they named that stores through `this`. That is too common to classify on, and is carried as its own column rather than folded into the row above.",
        percent(via_method, total)
    ));
    text.push_str("\n\nA kind reached by most operators is not a signal about any of them; the row is printed so that can be read off rather than assumed.\n\n");

    // Arity.
    let measured = reports
        .iter()
        .filter(|report| report.body.arity.is_some())
        .count();
    let branching = reports
        .iter()
        .filter(|report| report.body.arity.is_none() && report.body.arity_candidates.len() > 1)
        .count();
    let disagreements: Vec<&OperatorReport> = reports
        .iter()
        .filter(|report| report.arity_disagreement().is_some())
        .collect();
    let higher = disagreements
        .iter()
        .filter(|report| {
            let (measured, declared) = report.arity_disagreement().expect("filtered");
            measured > declared
        })
        .count();
    let lower = disagreements.len() - higher;
    text.push_str(&format!(
        "## Operand counts\n\n| | |\n| --- | ---: |\n| single arity on every returning path | {measured} |\n| paths disagree, so the nominal count is the successful path | {branching} |\n| nominal count **higher** than the count already recorded | **{higher}** |\n| nominal count **lower** than the count already recorded | **{lower}** |\n\n",
    ));
    text.push_str("The disagreements, largest gap first:\n\n| operator | body walk | previously recorded |\n| --- | ---: | ---: |\n");
    let mut sorted = disagreements.clone();
    sorted.sort_by_key(|report| {
        let (measured, declared) = report.arity_disagreement().expect("filtered");
        std::cmp::Reverse(measured.abs_diff(declared))
    });
    for report in sorted.iter().take(40) {
        let (measured, declared) = report.arity_disagreement().expect("filtered");
        text.push_str(&format!(
            "| `{}` | {measured} | {declared} |\n",
            report.name
        ));
    }
    if sorted.len() > 40 {
        text.push_str(&format!(
            "\n…and {} more, in `operator-bodies.tsv` where `arity_agrees` is `NO`.\n",
            sorted.len() - 40
        ));
    }
    text.push('\n');

    // Clusters.
    text.push_str("## How many clusters the gap makes\n\n| gap | clusters |\n| --- | ---: |\n");
    for sweep in GAP_SWEEP {
        text.push_str(&format!(
            "| `{sweep:#x}` | {} |\n",
            operator_bodies::cluster_globals(reports, sweep).len()
        ));
    }
    text.push('\n');
    text.push_str(&format!(
        "## Global-address clusters (gap {gap:#x})\n\nEach row is a run of data addresses no more than one page apart, and the operators that touch it. The reading in the docs is inferred from the operator names in the cluster; the addresses and counts are observed.\n\n| range | bytes | addresses | operators | a few of them |\n| --- | ---: | ---: | ---: | --- |\n"
    ));
    for cluster in clusters
        .iter()
        .filter(|cluster| cluster.operators.len() >= CLUSTER_REPORT_MINIMUM)
        .take(30)
    {
        let sample: Vec<&str> = cluster
            .operators
            .iter()
            .take(6)
            .map(String::as_str)
            .collect();
        text.push_str(&format!(
            "| `{:#010x}`–`{:#010x}` | {} | {} | {} | {} |\n",
            cluster.start,
            cluster.end,
            cluster.end - cluster.start,
            cluster.addresses,
            cluster.operators.len(),
            sample.join(", "),
        ));
    }
    text.push('\n');
    text
}

fn addresses(set: &BTreeSet<u32>) -> String {
    if set.is_empty() {
        return "none found".to_owned();
    }
    set.iter()
        .map(|address| format!("`{address:#010x}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 * 100.0 / whole as f64
    }
}

/// Keep the unused-import lint honest about the enum the table prints.
#[allow(dead_code)]
fn behaviour_labels() -> Vec<&'static str> {
    vec![Behaviour::Unknown.label()]
}
