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
//! More than one directory may be given, which is how the six shipped demo saves, the 3.02
//! install's two extra files and the GS5R3 install's four are surveyed in one run.
//!
//! Every file in the directory is attempted regardless of extension: two of the corpus files
//! (`quickstart`, `Merlin I`) have none, and a survey that filtered on `.sav` would have silently
//! skipped the only mid-game player states in existence here.
//!
//! **Every invariant prints the value it measured, not just pass or fail.** That is deliberate and
//! it is not decoration. The `LS_ALRM` turn index was wrong for two analysis passes while a
//! pass/fail check reported green -- *some* payload word equalled the turn, so the invariant
//! "held"; it was the wrong word, and there was no header for it to be a word of. Only the number
//! shows that.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lom_asset_viewer::save::{
    SECTION_TAGS, SaveContainer, SaveError, SaveFile, TagCensus, UserRecord,
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
    let mut structural_failures = 0_usize;
    let mut regularity_failures = 0_usize;
    let mut invariant_totals: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();
    // Grouped on the **normalized bytes themselves**, not on a hash of them. "7 distinct states"
    // is a headline claim, and a 64-bit non-cryptographic digest is not something a headline claim
    // should rest on when the exact bytes are right there.
    let mut states: Vec<(Vec<u8>, Vec<String>)> = Vec::new();
    // `LS_SPR_`'s no-fixed-stride argument is a property of the corpus, **not of any one file**:
    // individual files do admit a header size that divides evenly, and `combat.sav` admits exactly
    // one (717). Only the intersection across files is empty. Reporting it per file would be
    // claiming more than the measurement supports, so it is accumulated and checked once.
    let mut stride_intersection: Option<Vec<usize>> = None;
    let mut sprite_round_trips = 0_usize;
    let mut file_round_trips = 0_usize;
    let mut sprite_echo_clean = 0_usize;

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

        // Locate first and report from the container, so the section map and the structural
        // checks both appear even when the full parse refuses the file. A diagnostic reachable
        // only after a successful parse never fires on the files that need it.
        let located = SaveContainer::locate(&bytes);
        if let Ok(container) = &located {
            println!(
                "  tag order  {}",
                container
                    .tag_order()
                    .iter()
                    .map(|tag| tag.name())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            println!("  sections");
            for location in container.locations() {
                println!(
                    "    {:<8} tag @ {:>8}  payload @ {:>8}  {:>8} bytes",
                    location.tag.name(),
                    location.tag_offset,
                    location.payload_offset,
                    location.payload_len,
                );
            }
            println!("  structural checks");
            for check in container.structural_checks(&bytes) {
                let verdict = if check.passed { "PASS" } else { "FAIL" };
                if !check.passed {
                    structural_failures += 1;
                }
                let entry = invariant_totals.entry(check.name).or_insert((0, 0));
                entry.1 += 1;
                if check.passed {
                    entry.0 += 1;
                }
                println!("    {verdict} {:<56} {}", check.name, check.measured);
            }
        }

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

        println!("  fields");
        println!(
            "    LS_VER_  version {}  (stores multiplayer slots: {})",
            save.version.version,
            save.version.stores_multiplayer_slots()
        );
        println!(
            "    LS_MULT  declared setup {} bytes, slot block {}, {} slots occupied",
            save.multiplayer.declared_setup_len,
            match save.multiplayer.slots {
                Some(_) => "present",
                None => "absent (version < 99)",
            },
            save.multiplayer.occupied_slots().count(),
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
            "    LS_SPR_  {} records over {} bytes; classes {}",
            save.sprites.live_record_count(),
            save.sprites.records_raw().len() + 4,
            save.sprites
                .class_histogram()
                .iter()
                .map(|(class, count)| format!("{class}:{count}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
        println!(
            "             class-id echo: {} of {} records disagree",
            save.sprites.class_id_echo_disagreements(),
            save.sprites.records.len(),
        );
        if save.sprites.class_id_echo_disagreements() == 0 {
            sprite_echo_clean += 1;
        }
        // The round trip is printed per file rather than only aggregated, because "31/31" hides
        // which file broke on the run where it is not 31.
        let reencoded = save.sprites.encode();
        let original = {
            let mut bytes = Vec::with_capacity(4 + save.sprites.records_raw().len());
            bytes.extend_from_slice(&save.sprites.record_count.to_le_bytes());
            bytes.extend_from_slice(save.sprites.records_raw());
            bytes
        };
        let round_trips = reencoded == original;
        if save.reencode_with_sprites(&bytes) == bytes {
            file_round_trips += 1;
        }
        if round_trips {
            sprite_round_trips += 1;
        }
        println!(
            "             re-encode from the decoded records: {}",
            if round_trips {
                "byte-identical".to_string()
            } else {
                format!(
                    "DIFFERS -- {} bytes out vs {} in",
                    reencoded.len(),
                    original.len()
                )
            }
        );
        println!(
            "             header sizes 0..={STRIDE_SEARCH_LIMIT} that divide evenly: {strides:?}",
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
        // `.first()`, never `[0]`. `UserSection::parse` now guarantees eight records, and this
        // stays defensive anyway: the panic this replaces was a decode path killing the process,
        // and the cost of not repeating it is one `if let`.
        if let Some(record) = save.users.records.first() {
            println!(
                "             record 0: +4 {:#x}  +8 {}  +12 {:#x} ({})  +16 {}  +20 {}",
                record.unknown_4(),
                record.unknown_8(),
                record.unknown_12_bits(),
                record.unknown_12_as_f32(),
                record.unknown_16(),
                record.unknown_20(),
            );
        }
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
            "    LS_PLR_  {} records, lengths {:?}, slots {:?}, terminator at +{}, lord codes {:?}",
            save.players.records.len(),
            save.players.record_lengths(),
            save.players
                .records
                .iter()
                .map(|record| record.slot_index)
                .collect::<Vec<u32>>(),
            save.players.sentinel_offset(),
            save.players.lord_codes,
        );
        for record in &save.players.records {
            println!(
                "      slot {:>2}  {} bytes  queue {}  units {}  name {:?}",
                record.slot_index,
                record.encoded_len(),
                record.queue.len(),
                record
                    .armies
                    .iter()
                    .map(|army| army.units.len())
                    .sum::<usize>(),
                record.name_lossy().unwrap_or_default(),
            );
        }
        println!(
            "    LS_REGN  {}x{}  grid {} bytes  tail {} bytes  {} regions (array {} + 1)",
            save.regions.width,
            save.regions.height,
            save.regions.grid_len(),
            save.regions.tail_len(),
            save.regions.regions.len(),
            save.regions.array_count,
        );
        println!(
            "    LS_ALRM  queues {:?}  turn {:?}  countdown {:?} -> turn {:?}",
            save.alarms
                .queues
                .iter()
                .map(|queue| queue.records.len())
                .collect::<Vec<usize>>(),
            save.alarms.turn(),
            save.alarms.countdown(),
            save.alarms.turn_from_countdown(),
        );
        let mut callbacks: BTreeMap<String, usize> = BTreeMap::new();
        for (_, record) in save.alarms.records() {
            for name in &record.names {
                *callbacks
                    .entry(String::from_utf8_lossy(name).into_owned())
                    .or_default() += 1;
            }
        }
        println!("      callbacks {callbacks:?}");

        println!("  corpus regularities (a failure here is a DISCOVERY, not a bad file)");
        for check in save.regularities() {
            let verdict = if check.passed { "  ok" } else { "NEW!" };
            if !check.passed {
                regularity_failures += 1;
            }
            let entry = invariant_totals.entry(check.name).or_insert((0, 0));
            entry.1 += 1;
            if check.passed {
                entry.0 += 1;
            }
            println!("    {verdict} {:<56} {}", check.name, check.measured);
        }

        // Two files in the corpus are the same game state under two names, and they differ on disk
        // only in the leaked name padding. Group by a digest that excludes that padding, so the
        // survey reports distinct *states* rather than distinct files.
        let normalized = normalized_state(&save);
        match states.iter_mut().find(|(bytes, _)| *bytes == normalized) {
            Some((_, names)) => names.push(label),
            None => states.push((normalized, vec![label])),
        }
    }

    println!("{}", "=".repeat(96));
    println!("files\t{}", paths.len());
    println!("parsed\t{parsed}");
    println!("failed\t{failed}");
    println!("structural-failures\t{structural_failures}");
    println!(
        "regularity-failures\t{regularity_failures}   (a nonzero count here is a finding, not an error)"
    );
    println!("\nper-check, passed of attempted:");
    for (name, (passed, attempted)) in &invariant_totals {
        println!("  {passed:>3}/{attempted:<3} {name}");
    }

    let strides = stride_intersection.unwrap_or_default();
    println!(
        "\nLS_SPR_ class-id echo agrees in every record of {sprite_echo_clean}/{parsed} file(s)"
    );
    println!(
        "  -> an INTEGRITY check, not a layout check. The writer emits the same [object+4]\n     twice, at 0x004F6C25 and 0x004F6A8B, so this CANNOT fail on a save this engine wrote.\n     A failure means the file is damaged or came from another writer; it cannot detect a\n     wrong layout."
    );
    println!(
        "\nLS_SPR_ re-encoded byte-identically from its decoded records in {sprite_round_trips}/{parsed} file(s)\nwhole files reassembled byte-identically with LS_SPR_ regenerated: {file_round_trips}/{parsed}"
    );
    println!(
        "  -> proves LOSSLESS PRESERVATION and correct container splicing, and nothing more:\n     once parse succeeds the re-encode is an IDENTITY. It does not prove any record's\n     internal field boundaries and does not exclude compensating errors -- swap class 0's\n     12+88 for 16+84 and both trips still match byte for byte. The disassembly, and the\n     independently written fixture the version sweep drives, are the evidence for those."
    );
    println!(
        "\nLS_SPR_ header sizes 0..={STRIDE_SEARCH_LIMIT} that divide evenly in EVERY file: {strides:?}"
    );
    println!(
        "  -> {}",
        if strides.is_empty() {
            "no common candidate exists WITH A HEADER OF AT MOST 1024 BYTES. That is a bounded\n     search, not a proof of no stride: a larger header was not tried. The primary evidence\n     for variable-length records is the disassembly -- virtual dispatch through the 10-entry\n     table at 0x004F73B8 -- and the three variable-length classes it reaches. This arithmetic\n     corroborates them; it does not carry the claim on its own.\n     (Was: \"length-prefixed strings at irregular offsets\". REFUTED 2026-09-18 -- 0x00427A40\n     reads a counted RAW BYTE array with a jle guard and no terminator, and decoded at its\n     true offset its payload is small integers, not text.)"
        } else {
            "NOT empty: a fixed stride may exist after all, and this claim needs revisiting"
        }
    );

    println!(
        "\ndistinct game states (compared on normalized bytes; the leaked name padding is excluded):"
    );
    for (index, (_, names)) in states.iter().enumerate() {
        println!("  state {:>2}  {}", index + 1, names.join(", "));
    }
    println!("distinct-states\t{}", states.len());
    println!(
        "  -> {} file(s) carrying {} state(s). The file count is duplication across installs, NOT\n     corroboration; the six shipped demo saves are authored scenarios that may share a\n     generator, so agreement among them is weaker evidence than the count suggests.",
        paths.len(),
        states.len()
    );

    // Regularity failures deliberately do NOT fail the run. They are findings.
    if failed > 0 || structural_failures > 0 || !strides.is_empty() {
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

/// Every decoded field, serialized, with the uninitialised name padding left out.
///
/// Used to group files by **game state** rather than by bytes, because two saves of one state
/// differ on disk in the leaked name padding. Compared directly rather than hashed: the caller
/// publishes the group count as a headline number, and a 64-bit non-cryptographic hash is the
/// wrong thing to rest that on when the bytes are available.
///
/// Every field the parser decodes goes in. An earlier version omitted the map dimensions, the
/// trailer words, the plane count, `LS_GAME`'s four non-turn head fields, the player lord codes and
/// the region dimensions -- so two saves differing only in `game.unknown_4` collapsed into one
/// state, in the very function whose job is to tell states apart.
fn normalized_state(save: &SaveFile) -> Vec<u8> {
    let mut out = Vec::new();
    let mut word = |value: u32| out.extend_from_slice(&value.to_le_bytes());

    word(save.version.version);

    word(save.multiplayer.declared_setup_len);
    out.extend_from_slice(&save.multiplayer.setup);
    match &save.multiplayer.slots {
        None => out.push(0),
        Some(slots) => {
            out.push(1);
            for slot in slots {
                out.extend_from_slice(&slot.lord_code.to_le_bytes());
                // The name only, never the padding past its terminator.
                out.extend_from_slice(slot.name());
                out.push(0);
            }
        }
    }

    out.extend_from_slice(&save.map.map.width.to_le_bytes());
    out.extend_from_slice(&save.map.map.height.to_le_bytes());
    out.extend_from_slice(&save.map.map.bits_per_pixel.to_le_bytes());
    out.extend_from_slice(&save.map.plane_count.to_le_bytes());
    out.extend_from_slice(&save.map.trailer.to_le_bytes());
    for cell in &save.map.map.cells {
        out.extend_from_slice(&cell.tag.to_le_bytes());
        out.extend_from_slice(&cell.value_bits.to_le_bytes());
    }
    for value in &save.map.plane {
        out.extend_from_slice(&value.to_le_bytes());
    }

    out.extend_from_slice(&save.sprites.record_count.to_le_bytes());
    out.extend_from_slice(save.sprites.records_raw());

    for record in &save.users.records {
        out.extend_from_slice(&record.raw);
    }

    out.extend_from_slice(&save.game.turn.to_le_bytes());
    out.extend_from_slice(&save.game.unknown_4.to_le_bytes());
    out.extend_from_slice(&save.game.zero_8.to_le_bytes());
    out.extend_from_slice(&save.game.live_count.to_le_bytes());
    out.extend_from_slice(&save.game.declared_record_size.to_le_bytes());
    out.extend_from_slice(&save.game.trailer.to_le_bytes());
    for record in &save.game.records {
        out.extend_from_slice(&record.id.to_le_bytes());
        out.extend_from_slice(&record.a.to_le_bytes());
        out.extend_from_slice(&record.b.to_le_bytes());
    }

    out.extend_from_slice(&save.players.records_raw);
    out.extend_from_slice(&save.players.sentinel.to_le_bytes());
    for code in &save.players.lord_codes {
        out.extend_from_slice(&code.to_le_bytes());
    }

    out.extend_from_slice(&save.regions.width.to_le_bytes());
    out.extend_from_slice(&save.regions.height.to_le_bytes());
    for cell in &save.regions.cells {
        out.extend_from_slice(cell);
    }
    out.extend_from_slice(&save.regions.tail_raw);

    for queue in &save.alarms.queues {
        out.extend_from_slice(&(queue.records.len() as u32).to_le_bytes());
        for record in &queue.records {
            for word in &record.words {
                out.extend_from_slice(&word.to_le_bytes());
            }
            for name in &record.names {
                out.extend_from_slice(name);
            }
            for argument in &record.arguments {
                out.extend_from_slice(&argument.to_le_bytes());
            }
            out.extend_from_slice(&record.trailer.to_le_bytes());
        }
    }

    out
}
