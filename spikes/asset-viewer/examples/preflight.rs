//! Offline pre-flight compatibility check for a multiplayer session.
//!
//! The engine compares six values between peers and reports `Divergence` when any disagrees.
//! Three of the six are named after installed content, so in principle a person could be told
//! "these two installs will diverge" before anyone starts a game. This tool does as much of that
//! as the binary actually supports, and says plainly where it stops:
//!
//! | Divergence class | What this tool can say |
//! | --- | --- |
//! | `'EXE' version` | **exact.** The engine's algorithm is reproduced byte for byte. |
//! | `'GS' files` | **comparison only.** The algorithm is reproduced; the engine's value covers only the script members one top-level run reaches, which this tool does not determine. An exact content comparison is reported alongside and is authoritative. |
//! | `'IMP' files` | **nothing.** No code computing it was found. `imp.mpq` is compared byte for byte instead, which is what matters operationally even though the engine will not tell you about it. |
//!
//! Run it over two or more installed `English/` directories:
//!
//! ```text
//! cargo run --release --example preflight -- \
//!     "<install A>/English" "<install B>/English" ["<install C>/English" ...]
//! ```
//!
//! Every pair is reported, because the question a person has is "can these two play together?"
//! and with three installs there are three answers, not one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lom_asset_viewer::install_checksum::{ContentVerdict, exe_checksum, script_checksum_continued};
use lom_asset_viewer::mpq::Archive;

/// Files whose contents are compared for every install. `lomse.exe` is first because it is the
/// only one with an exactly reproducible engine checksum.
const COMPARED_FILES: [&str; 4] = ["lomse.exe", "gs.mpq", "imp.mpq", "pic.mpq"];

/// Archive whose script members feed the `'GS' files` accumulator.
const SCRIPT_ARCHIVE: &str = "gs.mpq";

/// What one install looks like to the checker.
struct Install {
    label: String,
    directory: PathBuf,
    /// Contents of each of [`COMPARED_FILES`] that exists, keyed by name.
    files: BTreeMap<String, Vec<u8>>,
    /// The reproduced `'EXE' version` value, if `lomse.exe` was readable.
    exe_version: Option<u32>,
    /// Script members of the archive: name to contents, and the accumulated engine checksum.
    scripts: Option<ScriptInventory>,
}

struct ScriptInventory {
    members: BTreeMap<String, Vec<u8>>,
    /// Bytes across all members, so a size difference is visible without diffing.
    total_bytes: usize,
    /// The engine's accumulator applied to every member. See the module docs for why this is a
    /// comparison and not a prediction.
    accumulated: i32,
    /// Members the archive listed but could not be read. Reported rather than dropped, because a
    /// silently skipped member would make two different installs look identical.
    unreadable: Vec<String>,
}

fn main() {
    let directories: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if directories.len() < 2 {
        eprintln!(
            "usage: preflight <install A English dir> <install B English dir> [more...]\n\
             \n\
             Reports, for every pair, whether the engine's content-derived divergence classes\n\
             would agree. Needs at least two installs to have anything to compare."
        );
        std::process::exit(2);
    }

    let installs: Vec<Install> = directories.iter().map(|d| read_install(d)).collect();

    println!("# Multiplayer pre-flight check");
    println!();
    println!(
        "Reproduced from `lomse.exe`: `'EXE' version` at 0x004b4a95-0x004b4b2a (zero-extended \
         byte sum), `'GS' files` at 0x004d497b-0x004d4990 (sign-extended byte sum)."
    );

    report_installs(&installs);
    report_pairs(&installs);
}

/// Archive metadata StormLib maintains, never handed to the script loader.
///
/// `(listfile)` is a real member, but its contents are the member *names*, so including it would
/// report a difference whenever two archives merely name their unnamed members differently -- a
/// difference the game's script loader never sees.
const ARCHIVE_METADATA: [&str; 3] = ["(listfile)", "(attributes)", "(signature)"];

/// Members StormLib had to invent a name for, because the archive's listfile does not name them.
///
/// These names are positional, not identities: the same bytes can be `File00000003.xxx` in one
/// archive and `File00000227.xxx` in another. They are counted but never presented as a
/// per-member difference, because the name comparison would be meaningless.
fn is_synthesised_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".xxx") else {
        return false;
    };
    let Some(digits) = stem.strip_prefix("File") else {
        return false;
    };
    !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit())
}

fn read_install(directory: &Path) -> Install {
    // The interesting name is the wrapper application, not `English` and not the game directory,
    // which is identical in every install and made all three columns read the same.
    let label = directory
        .ancestors()
        .find(|ancestor| {
            ancestor
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".app"))
        })
        .or_else(|| directory.parent())
        .and_then(|chosen| chosen.file_name())
        .or_else(|| directory.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| directory.display().to_string());

    let mut files = BTreeMap::new();
    for name in COMPARED_FILES {
        if let Ok(bytes) = std::fs::read(directory.join(name)) {
            files.insert(name.to_owned(), bytes);
        }
    }
    let exe_version = files.get("lomse.exe").map(|bytes| exe_checksum(bytes));
    let scripts = read_scripts(&directory.join(SCRIPT_ARCHIVE));

    Install {
        label,
        directory: directory.to_path_buf(),
        files,
        exe_version,
        scripts,
    }
}

/// Every member of the script archive, with the engine's accumulator run over all of them.
///
/// Every member is included, not only those with a `.gs` extension: the accumulator sits on the
/// script loader's input, and the corpus contains script members under other extensions. Taking
/// the whole archive is the conservative choice -- it can report a difference the engine would not
/// see, which is a false alarm, but it cannot miss one the engine would.
fn read_scripts(archive: &Path) -> Option<ScriptInventory> {
    let mpq = Archive::open(archive).ok()?;
    let entries = mpq.entries().ok()?;
    let mut members: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut unreadable = Vec::new();
    let mut accumulated = 0_i32;
    let mut total_bytes = 0_usize;
    for entry in entries {
        if ARCHIVE_METADATA.contains(&entry.name.as_str()) {
            continue;
        }
        match mpq.read(&entry.name) {
            Ok(bytes) => {
                accumulated = script_checksum_continued(accumulated, &bytes);
                total_bytes += bytes.len();
                members.insert(entry.name.clone(), bytes);
            }
            Err(_) => unreadable.push(entry.name.clone()),
        }
    }
    Some(ScriptInventory {
        members,
        total_bytes,
        accumulated,
        unreadable,
    })
}

fn report_installs(installs: &[Install]) {
    println!();
    println!("## Installs");
    println!();
    for install in installs {
        println!("### {}", install.label);
        println!();
        println!("`{}`", install.directory.display());
        println!();
        for name in COMPARED_FILES {
            match install.files.get(name) {
                Some(bytes) => println!("- `{name}`: {} bytes", bytes.len()),
                None => println!("- `{name}`: **missing**"),
            }
        }
        match &install.exe_version {
            Some(value) => println!("- `'EXE' version` = `{value}` (`{value:#010x}`)"),
            None => println!("- `'EXE' version`: cannot compute, `lomse.exe` unreadable"),
        }
        match &install.scripts {
            Some(scripts) => {
                println!(
                    "- `{SCRIPT_ARCHIVE}`: {} member(s), {} byte(s), engine accumulator over all \
                     members = `{}`",
                    scripts.members.len(),
                    scripts.total_bytes,
                    scripts.accumulated
                );
                if !scripts.unreadable.is_empty() {
                    println!(
                        "- **{} member(s) listed but unreadable**, so the accumulator above is \
                         incomplete: {}",
                        scripts.unreadable.len(),
                        scripts.unreadable.join(", ")
                    );
                }
            }
            None => println!("- `{SCRIPT_ARCHIVE}`: could not be opened as an MPQ"),
        }
        println!();
    }
}

fn report_pairs(installs: &[Install]) {
    println!("## Pairwise verdicts");
    for (index, left) in installs.iter().enumerate() {
        for right in installs.iter().skip(index + 1) {
            println!();
            println!("### {} vs {}", left.label, right.label);
            println!();
            report_exe_version(left, right);
            report_script_class(left, right);
            report_imp_class(left, right);
            report_other_files(left, right);
        }
    }
    println!();
    println!("## How to read this");
    println!();
    println!(
        "`'EXE' version` is a prediction about the engine, because the algorithm is fully \
         reproduced. `'GS' files` and `'IMP' files` are **not** predictions: the first reproduces \
         the algorithm over a member set the engine may not load, and the second has no recovered \
         algorithm at all. For both, the exact content comparison is the claim to act on -- two \
         peers running different bytes in a file the simulation reads cannot stay in lockstep, \
         whether or not the engine's own check notices."
    );
}

fn report_exe_version(left: &Install, right: &Install) {
    match (left.exe_version, right.exe_version) {
        (Some(a), Some(b)) if a == b => {
            println!("- `'EXE' version`: **will not diverge** -- both `{a}` (`{a:#010x}`).")
        }
        (Some(a), Some(b)) => println!(
            "- `'EXE' version`: **will diverge** -- `{a}` vs `{b}`. The engine reports this one \
             itself."
        ),
        _ => println!("- `'EXE' version`: cannot say, an executable was unreadable."),
    }
}

fn report_script_class(left: &Install, right: &Install) {
    let (Some(a), Some(b)) = (&left.scripts, &right.scripts) else {
        println!("- `'GS' files`: cannot say, an archive could not be opened.");
        return;
    };
    let differing = differing_members(a, b);
    if differing.is_empty() {
        println!(
            "- `'GS' files`: **script content is identical** -- {} member(s) on both sides, all \
             matching byte for byte. The engine's accumulator also agrees (`{}`).",
            a.members.len(),
            a.accumulated
        );
        return;
    }
    let named: Vec<&(String, String)> = differing
        .iter()
        .filter(|(name, _)| !is_synthesised_name(name))
        .collect();
    let synthesised = differing.len() - named.len();
    println!(
        "- `'GS' files`: **script content differs** -- {} named member(s) differ{}. Engine \
         accumulator over all members: `{}` vs `{}`{}.",
        named.len(),
        if synthesised > 0 {
            format!(
                ", plus {synthesised} member(s) the archive does not name (counted, not listed: \
                 their names are positional)"
            )
        } else {
            String::new()
        },
        a.accumulated,
        b.accumulated,
        if a.accumulated == b.accumulated {
            " (**equal despite differing content**, so the sum alone would have missed this)"
        } else {
            ""
        }
    );
    for (name, verdict) in named.iter().take(12) {
        println!("    - `{name}`: {verdict}");
    }
    if named.len() > 12 {
        println!("    - ... and {} more named member(s)", named.len() - 12);
    }
}

fn differing_members(left: &ScriptInventory, right: &ScriptInventory) -> Vec<(String, String)> {
    let mut differing = Vec::new();
    for (name, bytes) in &left.members {
        match right.members.get(name) {
            Some(other) => {
                let verdict = ContentVerdict::compare(bytes, other);
                if !verdict.is_identical() {
                    differing.push((name.clone(), verdict.describe()));
                }
            }
            None => differing.push((name.clone(), String::from("absent on the right"))),
        }
    }
    for name in right.members.keys() {
        if !left.members.contains_key(name) {
            differing.push((name.clone(), String::from("absent on the left")));
        }
    }
    differing
}

fn report_imp_class(left: &Install, right: &Install) {
    let verdict = compare_file(left, right, "imp.mpq");
    match verdict {
        Some(verdict) if verdict.is_identical() => println!(
            "- `'IMP' files`: `imp.mpq` is identical. No algorithm for this class was recovered, \
             so this is a content statement, not a prediction."
        ),
        Some(verdict) => println!(
            "- `'IMP' files`: `imp.mpq` differs ({}). No algorithm was recovered, so whether the \
             engine would report it is unknown.",
            verdict.describe()
        ),
        None => println!("- `'IMP' files`: cannot say, `imp.mpq` missing on one side."),
    }
}

fn report_other_files(left: &Install, right: &Install) {
    for name in COMPARED_FILES {
        if name == "imp.mpq" || name == SCRIPT_ARCHIVE || name == "lomse.exe" {
            continue;
        }
        match compare_file(left, right, name) {
            Some(verdict) => println!("- `{name}` (no divergence class): {}", verdict.describe()),
            None => println!("- `{name}` (no divergence class): missing on one side."),
        }
    }
}

fn compare_file(left: &Install, right: &Install, name: &str) -> Option<ContentVerdict> {
    let a = left.files.get(name)?;
    let b = right.files.get(name)?;
    Some(ContentVerdict::compare(a, b))
}
