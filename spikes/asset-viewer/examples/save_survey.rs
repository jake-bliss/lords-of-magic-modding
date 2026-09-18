//! Parse every `.sav` in a directory and report its sections, its fields and every invariant.
//!
//! **No save is committed** -- they are proprietary game data -- so this takes a directory rather
//! than embedding a fixture. Point it at an install's `English/savegame/`:
//!
//! ```sh
//! cargo run --release --example save_survey -- \
//!   "$HOME/Applications/Steambuild 32 64bit DXVK.app/Contents/SharedSupport/prefix/drive_c/\
//! Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English/savegame"
//! ```
//!
//! More than one directory may be given, which is how the six shipped demo saves and the 3.02
//! install's two files are surveyed in one run.
//!
//! Every file in the directory is attempted regardless of extension: two of the corpus files
//! (`quickstart`, `Merlin I`) have none, and a survey that filtered on `.sav` would have silently
//! skipped the only mid-game player states in existence here.
//!
//! **Every invariant prints the value it measured, not just pass or fail.** That is deliberate and
//! it is not decoration. The `LS_ALRM` turn index was wrong for a whole analysis pass while a
//! pass/fail check reported green -- *some* header word equalled the turn, so the invariant
//! "held"; it was the wrong word. Only the number shows that.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lom_asset_viewer::save::{
    AlarmSection, SECTION_TAGS, SaveError, SaveFile, TagCensus, UserRecord,
};

/// How far the `LS_SPR_` fixed-stride search looks for a plausible per-section header.
const STRIDE_SEARCH_LIMIT: usize = 1024;

fn main() -> ExitCode {
    let directories: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if directories.is_empty() {
        eprintln!("usage: save_survey DIRECTORY [DIRECTORY ...]");
        return ExitCode::from(2);
    }

    let mut paths = Vec::new();
    for directory in &directories {
        let Ok(entries) = std::fs::read_dir(directory) else {
            eprintln!("could not read {}", directory.display());
            return ExitCode::from(2);
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        found.sort();
        paths.extend(found);
    }

    if paths.is_empty() {
        eprintln!("no files in the given directories -- an empty run is not a pass");
        return ExitCode::from(2);
    }

    let mut parsed = 0_usize;
    let mut failed = 0_usize;
    let mut invariant_failures = 0_usize;
    let mut invariant_totals: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();
    let mut content_digests: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // `LS_SPR_`'s no-fixed-stride argument is a property of the corpus, **not of any one file**:
    // individual files do admit a header size that divides evenly, and `combat.sav` admits exactly
    // one (717). Only the intersection across files is empty. Reporting it per file would be
    // claiming more than the measurement supports, so it is accumulated and checked once.
    let mut stride_intersection: Option<Vec<usize>> = None;

    for path in &paths {
        let label = display_name(path);
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                failed += 1;
                println!("FAIL {label:<16} could not read: {error}");
                continue;
            }
        };

        println!("{}", "=".repeat(96));
        println!("{label}   {} bytes", bytes.len());

        let save = match SaveFile::parse(&bytes) {
            Ok(save) => save,
            Err(error) => {
                failed += 1;
                println!("  FAIL {error}");
                if let SaveError::TagCensusFailed(census) = &error {
                    print_census(census);
                }
                continue;
            }
        };
        parsed += 1;

        println!(
            "  tag order  {}",
            save.container
                .tag_order()
                .iter()
                .map(|tag| tag.name())
                .collect::<Vec<_>>()
                .join(" ")
        );
        println!("  sections");
        for location in save.container.locations() {
            println!(
                "    {:<8} tag @ {:>8}  payload @ {:>8}  {:>8} bytes",
                location.tag.name(),
                location.tag_offset,
                location.payload_offset,
                location.payload_len,
            );
        }

        println!("  fields");
        println!(
            "    LS_VER_  version {}  (stores multiplayer slots: {})",
            save.version.version,
            save.version.stores_multiplayer_slots()
        );
        println!(
            "    LS_MULT  declared setup {} bytes, {} of {} slots occupied",
            save.multiplayer.declared_setup_len,
            save.multiplayer.occupied_slots().count(),
            save.multiplayer.slots.len()
        );
        for (index, slot) in save.multiplayer.occupied_slots() {
            println!(
                "             slot {index:>2}  code {:#06x}  name {:?}  +{} leaked padding bytes",
                slot.lord_code,
                slot.name_lossy(),
                slot.name_padding().len()
            );
        }
        println!(
            "    LS_MAP_  {}x{} bpc {}  plane count {}  trailer {}",
            save.map.map.width,
            save.map.map.height,
            save.map.map.bits_per_pixel,
            save.map.plane_count,
            save.map.trailer,
        );
        println!(
            "             visibility {}",
            format_histogram(&save.map.visibility_histogram())
        );
        let strides = save.sprites.candidate_fixed_strides(STRIDE_SEARCH_LIMIT);
        println!(
            "    LS_SPR_  {} records, {} bytes carried undecoded; header sizes 0..={} that divide evenly: {:?}",
            save.sprites.record_count,
            save.sprites.records_raw().len(),
            STRIDE_SEARCH_LIMIT,
            strides,
        );
        stride_intersection = Some(match stride_intersection {
            None => strides,
            Some(previous) => previous
                .into_iter()
                .filter(|header| strides.contains(header))
                .collect(),
        });
        println!(
            "    LS_USER  {} records of {} bytes; indexes {:?}",
            save.users.records.len(),
            UserRecord::LEN,
            save.users
                .records
                .iter()
                .map(UserRecord::index)
                .collect::<Vec<_>>()
        );
        println!(
            "             record 0: +4 {:#x}  +8 {}  +12 {:#x} ({})  +16 {}  +20 {}",
            save.users.records[0].unknown_4(),
            save.users.records[0].unknown_8(),
            save.users.records[0].unknown_12_bits(),
            save.users.records[0].unknown_12_as_f32(),
            save.users.records[0].unknown_16(),
            save.users.records[0].unknown_20(),
        );
        println!(
            "    LS_GAME  turn {}  unknown_4 {}  live_count {}  records {}  surplus {}  trailer {}",
            save.game.turn,
            save.game.unknown_4,
            save.game.live_count,
            save.game.records.len(),
            save.game.record_surplus(),
            save.game.trailer,
        );
        println!(
            "    LS_PLR_  {} record bytes undecoded, terminator at +{}, lord codes {:?}",
            save.players.records_raw.len(),
            save.players.sentinel_offset(),
            save.players.lord_codes,
        );
        println!(
            "    LS_REGN  {}x{}  grid {} bytes  tail {} bytes (structure unknown)",
            save.regions.width,
            save.regions.height,
            save.regions.grid_len(),
            save.regions.tail_len(),
        );
        println!(
            "    LS_ALRM  header {:?}  turn@{} {}  countdown {} -> turn {:?}  {} record bytes undecoded",
            save.alarms.header,
            AlarmSection::TURN_INDEX,
            save.alarms.turn(),
            save.alarms.countdown(),
            save.alarms.turn_from_countdown(),
            save.alarms.records_raw.len(),
        );

        println!("  invariants");
        for invariant in save.invariants() {
            let verdict = if invariant.passed { "PASS" } else { "FAIL" };
            if !invariant.passed {
                invariant_failures += 1;
            }
            let entry = invariant_totals.entry(invariant.name).or_insert((0, 0));
            entry.1 += 1;
            if invariant.passed {
                entry.0 += 1;
            }
            println!(
                "    {verdict} {:<56} {}",
                invariant.name, invariant.measured
            );
        }

        // Two files in the corpus are the same game state under two names, and they differ on disk
        // only in the leaked name padding. Group by a digest that excludes that padding, so the
        // survey reports distinct *states* rather than distinct files.
        content_digests
            .entry(content_digest(&save))
            .or_default()
            .push(label);
    }

    println!("{}", "=".repeat(96));
    println!("files\t{}", paths.len());
    println!("parsed\t{parsed}");
    println!("failed\t{failed}");
    println!("invariant-failures\t{invariant_failures}");
    println!("\nper-invariant, passed of attempted:");
    for (name, (passed, attempted)) in &invariant_totals {
        println!("  {passed:>3}/{attempted:<3} {name}");
    }

    let strides = stride_intersection.unwrap_or_default();
    println!(
        "\nLS_SPR_ header sizes 0..={STRIDE_SEARCH_LIMIT} that divide evenly in EVERY file: {strides:?}"
    );
    println!(
        "  -> {}",
        if strides.is_empty() {
            "empty, so no fixed record stride exists -- measured here, not quoted from a past run"
        } else {
            "NOT empty: a fixed stride may exist after all, and this claim needs revisiting"
        }
    );

    println!("\ndistinct game states (digest ignores the leaked name padding):");
    for (index, names) in content_digests.values().enumerate() {
        println!("  state {:>2}  {}", index + 1, names.join(", "));
    }
    println!("distinct-states\t{}", content_digests.len());

    if failed > 0 || invariant_failures > 0 || !strides.is_empty() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// `install/file`, so the same shipped save under two installs is distinguishable in the output.
fn display_name(path: &Path) -> String {
    let file = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    // Prefer the enclosing `.app` bundle, because every install stores its saves in a directory
    // called `savegame` and the immediate parent therefore cannot tell two installs apart.
    let install = path.ancestors().find_map(|ancestor| {
        let name = ancestor.file_name()?.to_string_lossy().into_owned();
        name.ends_with(".app").then_some(name)
    });
    match install.or_else(|| {
        path.parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
    }) {
        Some(parent) => format!("{parent}/{file}"),
        None => file,
    }
}

fn print_census(census: &TagCensus) {
    println!("       tag census:");
    for tag in SECTION_TAGS {
        println!("         {:<8} {}", tag.name(), census.count(tag));
    }
}

fn format_histogram(histogram: &BTreeMap<i16, usize>) -> String {
    histogram
        .iter()
        .map(|(level, count)| format!("{level}={count}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A cheap order-independent digest of everything decoded, with the uninitialised name padding
/// left out. Not cryptographic; it only has to separate the corpus's game states.
fn content_digest(save: &SaveFile) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut eat = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    };
    eat(&save.version.version.to_le_bytes());
    eat(&save.multiplayer.setup);
    for slot in &save.multiplayer.slots {
        eat(&slot.lord_code.to_le_bytes());
        eat(slot.name());
    }
    for cell in &save.map.map.cells {
        eat(&cell.tag.to_le_bytes());
        eat(&cell.value_bits.to_le_bytes());
    }
    for word in &save.map.plane {
        eat(&word.to_le_bytes());
    }
    eat(&save.sprites.record_count.to_le_bytes());
    eat(save.sprites.records_raw());
    for record in &save.users.records {
        eat(&record.raw);
    }
    eat(&save.game.turn.to_le_bytes());
    for record in &save.game.records {
        eat(&record.id.to_le_bytes());
        eat(&record.a.to_le_bytes());
        eat(&record.b.to_le_bytes());
    }
    eat(&save.players.records_raw);
    for cell in &save.regions.cells {
        eat(cell);
    }
    eat(&save.regions.tail_raw);
    for word in &save.alarms.header {
        eat(&word.to_le_bytes());
    }
    eat(&save.alarms.records_raw);
    format!("{hash:016x}")
}
