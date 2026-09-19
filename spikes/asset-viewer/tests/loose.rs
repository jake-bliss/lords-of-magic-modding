//! Regression anchors for the loose-file sweep and the two install-root configuration formats.
//!
//! The checks that run everywhere read the **committed reports**, not values transcribed out of the
//! parser. Two of them are cross-table equations: `reports/loose/inventory-*.tsv` records the size
//! of `lom.cfg` and `settings.cfg` as the bytes on disk, and `reports/loose/config-fields.tsv`
//! records what the parsers made of those same files. The layout claim in `loose.rs` is exactly the
//! statement that those two numbers are related by a fixed equation, so asserting the equation
//! fails when the layout is wrong -- which a table of expected field values copied out of the
//! parser could never do.
//!
//! The checks that need the proprietary tree are `#[ignore]`d, so a machine without the installs
//! reports them as ignored rather than as passed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lom_asset_viewer::loose::{self, LomConfig, MagicSignature, SettingsConfig, magic_signature};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the crate sits two directories below the repository root")
}

const PROFILES: [&str; 4] = ["baseline", "development", "gs5r3", "patch302"];

/// Rows of one committed inventory, keyed by relative path.
///
/// The first line is a `# profile` comment rather than a column, because the label is one value for
/// the whole file and repeating it on 467 rows would make four otherwise-identical reports differ
/// on every line.
fn inventory_rows(profile: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let path = repository_root()
        .join("reports/loose")
        .join(format!("inventory-{profile}.tsv"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is committed: {error}", path.display()));
    let mut lines = text.lines();
    let comment = lines
        .next()
        .expect("the report opens with a profile comment");
    assert_eq!(
        comment,
        format!("# profile\t{profile}"),
        "{} names a different profile from its file name",
        path.display()
    );
    let columns: Vec<&str> = lines
        .next()
        .expect("the report has a header")
        .split('\t')
        .collect();
    let mut rows = BTreeMap::new();
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            fields.len(),
            columns.len(),
            "row {:?} of {} has {} fields against {} columns",
            fields.first(),
            path.display(),
            fields.len(),
            columns.len()
        );
        let row: BTreeMap<String, String> = columns
            .iter()
            .zip(fields.iter())
            .map(|(column, field)| ((*column).to_owned(), (*field).to_owned()))
            .collect();
        let key = row["relative_path"].clone();
        assert!(
            rows.insert(key.clone(), row).is_none(),
            "{key} appears twice in {}",
            path.display()
        );
    }
    rows
}

/// `reports/loose/config-fields.tsv` as `(profile, file, field) -> value`.
fn config_fields() -> BTreeMap<(String, String, String), String> {
    let path = repository_root().join("reports/loose/config-fields.tsv");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is committed: {error}", path.display()));
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("profile\tfile\tfield\tvalue"),
        "{} has an unexpected header",
        path.display()
    );
    let mut rows = BTreeMap::new();
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 4, "row {line:?} does not have four fields");
        let key = (
            fields[0].to_owned(),
            fields[1].to_owned(),
            fields[2].to_owned(),
        );
        // A duplicate key used to overwrite silently, which would let a report carrying two
        // different values for one field look consistent to every check that reads it.
        assert!(
            rows.insert(key.clone(), fields[3].to_owned()).is_none(),
            "{:?} appears twice in {}",
            key,
            path.display()
        );
    }
    rows
}

#[test]
fn every_inventory_row_carries_a_usable_digest_and_size() {
    for profile in PROFILES {
        let rows = inventory_rows(profile);
        assert!(
            !rows.is_empty(),
            "the {profile} inventory has no rows at all"
        );
        for (path, row) in &rows {
            // A row for a file that could not be read carries no digest by design; asserting the
            // digest shape on it would fail the suite for a filesystem race with a message naming
            // neither the race nor the file.
            if row["probe_error"].starts_with(loose::UNREADABLE_PREFIX) {
                assert_eq!(
                    row["sha256"],
                    loose::UNREADABLE_DIGEST,
                    "{profile}:{path} is unreadable and must carry no digest"
                );
                assert_eq!(row["size"], "0", "{profile}:{path}");
                continue;
            }
            let digest = &row["sha256"];
            assert_eq!(
                digest.len(),
                64,
                "{profile}:{path} has a {}-character digest",
                digest.len()
            );
            assert!(
                digest
                    .chars()
                    .all(|character| character.is_ascii_hexdigit() && !character.is_uppercase()),
                "{profile}:{path} has a digest that is not lowercase hex: {digest}"
            );
            row["size"]
                .parse::<u64>()
                .unwrap_or_else(|error| panic!("{profile}:{path} has an unreadable size: {error}"));
            assert!(
                matches!(row["extension_disagrees"].as_str(), "yes" | "no"),
                "{profile}:{path} has a non-boolean disagreement column"
            );
        }
    }
}

/// One digest, one content length -- across all four reports.
///
/// The four installs are copy-on-write clones, so the same file appears up to four times. If two
/// rows anywhere share a digest but disagree on size, either the digest is not of the bytes the row
/// claims or the walk mismatched a path with its contents.
#[test]
fn equal_digests_across_the_profiles_agree_on_length() {
    let mut sizes: BTreeMap<String, (u64, String)> = BTreeMap::new();
    for profile in PROFILES {
        for (path, row) in inventory_rows(profile) {
            let size: u64 = row["size"].parse().expect("checked elsewhere");
            let digest = row["sha256"].clone();
            if let Some((seen, seen_at)) = sizes.get(&digest) {
                assert_eq!(
                    *seen, size,
                    "digest {digest} is {seen} bytes at {seen_at} and {size} bytes at {profile}:{path}"
                );
            } else {
                sizes.insert(digest, (size, format!("{profile}:{path}")));
            }
        }
    }
}

/// The `lom.cfg` size equation, across two reports.
///
/// `20 + 4 * help-panel-count + 36`: four head words and a count word ahead of the vector
/// (4x4 + 4 = 20), then two words, a 16-byte GUID and three words behind it
/// (4 + 4 + 16 + 4 + 4 + 4 = 36). Every term is a field the writer emits, in its order.
///
/// **What this cannot do**, stated because an earlier version of this comment claimed otherwise:
/// it cannot fail on a wrong layout. `LomConfig::parse` refuses any image whose length disagrees
/// with its own count, so `config-fields.tsv` can never hold a counter-example -- the producer
/// enforces the invariant before either number is emitted. It is also blind to a permutation of
/// equal-width slots. What it does catch is the two reports drifting out of step with each other,
/// which is worth catching and is all it is here for. The checks that bear on slot *identity* are
/// the two below.
#[test]
fn the_recorded_lom_cfg_size_matches_its_recorded_help_panel_count() {
    let fields = config_fields();
    for profile in PROFILES {
        let count: usize = fields[&(
            profile.to_owned(),
            "lom.cfg".to_owned(),
            "help-panel-count".to_owned(),
        )]
            .parse()
            .expect("the count is a number");
        let checks = fields[&(
            profile.to_owned(),
            "lom.cfg".to_owned(),
            "help-panel-checks".to_owned(),
        )]
            .split(',')
            .count();
        assert_eq!(
            checks, count,
            "{profile} records {count} help panels but lists {checks} check values"
        );
        let on_disk: u64 = inventory_rows(profile)["English/lom.cfg"]["size"]
            .parse()
            .expect("checked elsewhere");
        assert_eq!(
            on_disk as usize,
            20 + 4 * count + 36,
            "{profile}: lom.cfg is {on_disk} bytes, which the {count}-entry layout does not explain"
        );
        assert_eq!(
            fields[&(
                profile.to_owned(),
                "lom.cfg".to_owned(),
                "round-trips".to_owned()
            )],
            "true",
            "{profile}: lom.cfg did not re-encode to its own bytes"
        );
    }
}

/// The `settings.cfg` record rule, checked the same way.
///
/// Each record is `KEY`, one space, the value, one `CR`. Summing that over the recorded settings
/// has to come out at the recorded file size; a different separator, a `CRLF`, or a dropped record
/// all break the sum.
#[test]
fn the_recorded_settings_cfg_size_is_the_sum_of_its_records() {
    let fields = config_fields();
    for profile in PROFILES {
        let mut total = 0_usize;
        let mut records = 0_usize;
        for ((row_profile, file, field), value) in &fields {
            if row_profile != profile || file != "settings.cfg" {
                continue;
            }
            let Some(key) = field.strip_prefix("setting:") else {
                continue;
            };
            total += key.len() + 1 + value.len() + 1;
            records += 1;
        }
        let declared: usize = fields[&(
            profile.to_owned(),
            "settings.cfg".to_owned(),
            "records".to_owned(),
        )]
            .parse()
            .expect("the record count is a number");
        assert_eq!(
            records, declared,
            "{profile}: settings.cfg declares {declared} records and lists {records}"
        );
        let on_disk: usize = inventory_rows(profile)["English/settings.cfg"]["size"]
            .parse()
            .expect("checked elsewhere");
        assert_eq!(
            total, on_disk,
            "{profile}: settings.cfg is {on_disk} bytes and its records sum to {total}"
        );
    }
}

/// Where the magic signature is decisive, the existing probe must reach the same verdict.
///
/// This is the cross-check between the two classifiers. It deliberately covers only the formats
/// both instruments claim to recognise; the point of recording both columns is the cases where they
/// differ, and those are named in `docs/loose-files.md` rather than asserted here.
#[test]
fn the_two_classifiers_agree_wherever_both_are_decisive() {
    for profile in PROFILES {
        for (path, row) in inventory_rows(profile) {
            let expected = match row["magic"].as_str() {
                "wave-audio" => "wave-audio",
                "smacker-video" => "smacker-video",
                "mpq-archive" => "mpq-archive",
                "iff-pbm" => "iff-pbm",
                _ => continue,
            };
            assert_eq!(
                row["probe_kind"], expected,
                "{profile}:{path} is {} by magic but {} by probe",
                row["magic"], row["probe_kind"]
            );
        }
    }
}

/// Pinned because it refuted the expectation this sweep started with.
///
/// `.lgd` was expected to carry the `LS_VER_` serialisation header alongside the saves. It does
/// not: every legend scenario in the corpus opens with a bare `u32`. The signature therefore names
/// game *state*, shipped or saved, and a later change that starts reporting `.lgd` as serialised
/// state would be re-introducing the refuted claim.
#[test]
fn legend_scenarios_do_not_carry_the_serialisation_header() {
    for profile in PROFILES {
        let mut seen = 0_usize;
        for (path, row) in inventory_rows(profile) {
            if row["extension"] != "lgd" {
                continue;
            }
            seen += 1;
            assert_eq!(
                row["magic"], "unrecognised",
                "{profile}:{path} now reports a magic signature"
            );
            assert_eq!(row["probe_kind"], "legend-scenario");
        }
        assert!(seen > 0, "{profile} has no legend scenarios to check");
    }
}

/// The saves and the shipped starting states are one family.
#[test]
fn shipped_starting_states_share_the_save_serialisation() {
    let rows = inventory_rows("baseline");
    for name in [
        "English/savegame/combat.sav",
        "English/savegame/experience.sav",
        "English/savegame/magic.sav",
        "English/savegame/merc.sav",
        "English/savegame/temple.sav",
        "English/savegame/quickstart",
    ] {
        let row = rows
            .get(name)
            .unwrap_or_else(|| panic!("{name} is in the baseline inventory"));
        assert_eq!(row["magic"], "lom-serialised", "{name}");
        // And the existing probe cannot name it, which is the gap this sweep is reporting rather
        // than papering over.
        assert_eq!(row["probe_kind"], "unknown", "{name}");
    }
}

#[test]
fn the_digest_in_the_report_is_the_digest_of_the_recorded_length() {
    // A zero-length file has one possible digest, so the report's own empty entries are a cheap
    // end-to-end check that the walker hashed contents rather than names.
    let empty = loose::sha256_hex(b"");
    for profile in PROFILES {
        for (path, row) in inventory_rows(profile) {
            if row["probe_error"].starts_with(loose::UNREADABLE_PREFIX) {
                continue;
            }
            if row["size"] == "0" {
                assert_eq!(row["sha256"], empty, "{profile}:{path} is empty");
            } else {
                assert_ne!(
                    row["sha256"], empty,
                    "{profile}:{path} has the empty digest but a nonzero length"
                );
            }
        }
    }
}

/// The branch that turns an unreadable file into a row.
///
/// Nothing else in the suite runs it. The sweep is supposed to survive a profile being played
/// underneath it, and a tolerance nobody exercises is a tolerance that works until the day it is
/// needed. Uses a `chmod 000` file, which is the reachable half of the race -- the other half, a
/// file that vanishes between the walk and the read, cannot be provoked deterministically here.
#[test]
#[cfg(unix)]
fn an_unreadable_file_becomes_a_row_rather_than_ending_the_sweep() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!(
        "lom-loose-unreadable-{}-{}",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("the fixture tree is created");
    std::fs::write(root.join("readable.txt"), b"fine\n").expect("written");
    let locked = root.join("locked.bin");
    std::fs::write(&locked, b"secret").expect("written");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");

    let result = loose::inventory(&root);
    // Restore before asserting, so a failure does not leave an undeletable fixture behind.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).expect("chmod back");
    let rows = result.expect("one unreadable file must not end the sweep");
    std::fs::remove_dir_all(&root).expect("the fixture tree is removed");

    assert_eq!(rows.len(), 2, "both files are rows");
    let locked_row = rows
        .iter()
        .find(|row| row.relative_path == "locked.bin")
        .expect("the unreadable file is still a row");
    assert_eq!(locked_row.sha256, loose::UNREADABLE_DIGEST);
    assert_eq!(locked_row.size, 0);
    assert!(
        locked_row
            .probe_error
            .as_deref()
            .is_some_and(|error| error.starts_with(loose::UNREADABLE_PREFIX)),
        "the row must say why it has no digest: {:?}",
        locked_row.probe_error
    );
    // And the good file is unaffected, which is the whole point of not aborting.
    let readable = rows
        .iter()
        .find(|row| row.relative_path == "readable.txt")
        .expect("the readable file is a row");
    assert_eq!(readable.sha256, loose::sha256_hex(b"fine\n"));
}

/// Two named `lom.cfg` slots against two independently named `settings.cfg` keys.
///
/// This is the first check in the file that bears on *which slot holds what* rather than on how the
/// bytes divide up. `lom.cfg`'s `building-speech-flag` is named by `getbuildingspeechflag`, read
/// out of the engine; `settings.cfg`'s `BUILDING_SPEECH` is a literal in a text file that knows
/// nothing about the engine. Nothing makes them agree except the naming being right.
///
/// **It is a weak check and should be read as one.** Both pairs are constant across all four
/// profiles -- `BUILDING_SPEECH 1` and `CENTER_MOVE 0` everywhere -- so the corpus offers one
/// value per pair, and any permutation that happens to preserve the value slips through. It binds
/// names, not values. The corpus cannot currently do better for these two fields.
#[test]
fn named_lom_cfg_slots_agree_with_the_independently_named_settings_keys() {
    let fields = config_fields();
    for profile in PROFILES {
        for (slot, key) in [
            ("building-speech-flag", "BUILDING_SPEECH"),
            ("center-on-movement", "CENTER_MOVE"),
        ] {
            let from_binary = &fields[&(profile.to_owned(), "lom.cfg".to_owned(), slot.to_owned())];
            let from_text = &fields[&(
                profile.to_owned(),
                "settings.cfg".to_owned(),
                format!("setting:{key}"),
            )];
            assert_eq!(
                from_binary, from_text,
                "{profile}: lom.cfg {slot} is {from_binary} and settings.cfg {key} is {from_text}"
            );
        }
    }
}

/// The slots that actually vary across the corpus, pinned against their neighbours.
///
/// A permutation of equal-width slots is only detectable where the two slots hold different values
/// in some profile. Two places in `lom.cfg` qualify, and both are checked here.
///
/// The `used-drawblt` word is `1` in two profiles and `-1` in the other two, while both words
/// beside it -- `show-completed-quests` and `building-speech-flag` -- are `1` everywhere. So a swap
/// of the final word with either neighbour changes what this test reads, in `gs5r3` and
/// `patch302`. The `balkoth-kill-counter`/`center-on-movement` pair, by contrast, is `0` in all
/// four profiles, and no check over this corpus can tell those two apart: that limit is recorded in
/// `docs/loose-files.md` rather than papered over here.
///
/// The GUID pins the 16-byte slot's position: three profiles are clones of one prefix and share a
/// GUID, `patch302` regenerated and differs. A misplaced 16-byte window would not reproduce that
/// pattern.
#[test]
fn the_slots_that_vary_pin_their_own_position() {
    let fields = config_fields();
    let value = |profile: &str, field: &str| {
        fields[&(profile.to_owned(), "lom.cfg".to_owned(), field.to_owned())].clone()
    };

    for profile in ["baseline", "development"] {
        assert_eq!(value(profile, "used-drawblt"), "1", "{profile}");
    }
    for profile in ["gs5r3", "patch302"] {
        let tail = value(profile, "used-drawblt");
        assert_eq!(tail, "-1", "{profile}");
        // The discrimination itself: the final word is not either of the words beside it.
        assert_ne!(
            tail,
            value(profile, "show-completed-quests"),
            "{profile}: the final word reads the same as its left neighbour, so a swap of the two \
             would be invisible here"
        );
        assert_ne!(tail, value(profile, "building-speech-flag"), "{profile}");
    }

    let baseline = value("baseline", "install-guid");
    assert_eq!(value("development", "install-guid"), baseline);
    assert_eq!(value("gs5r3", "install-guid"), baseline);
    assert_ne!(value("patch302", "install-guid"), baseline);
    assert_ne!(
        baseline, "00000000-0000-0000-0000-000000000000",
        "a nil GUID would mean the 16-byte window landed on padding"
    );
}

/// The walker itself, on a tree built for the purpose.
///
/// Every other check in this file reads a committed report, which cannot fail when the *walker*
/// changes -- if it started handing an empty name to `asset::probe` and every `.smp` became
/// `unknown`, the committed TSVs would still parse and still agree with each other. This runs the
/// real `loose::inventory`.
///
/// The fixture is deliberately **not** shaped like the game corpus, and proves nothing about any
/// game format. It exercises the plumbing: that the walk recurses, that paths come back relative
/// and slash-joined, that the digest is of the contents, that the extension is lowercased, that the
/// filename reaches `probe`, and that an empty file is handled rather than skipped.
#[test]
fn the_walker_reports_contents_not_names() {
    let root = std::env::temp_dir().join(format!(
        "lom-loose-walker-{}-{}",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("nested/deeper")).expect("the fixture tree is created");

    // A minimal RIFF/WAVE: a header claiming a `fmt ` chunk the probe can read.
    let mut wave = Vec::new();
    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&36_u32.to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt ");
    wave.extend_from_slice(&16_u32.to_le_bytes());
    wave.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    wave.extend_from_slice(&2_u16.to_le_bytes()); // channels
    wave.extend_from_slice(&22_050_u32.to_le_bytes());
    wave.extend_from_slice(&44_100_u32.to_le_bytes());
    wave.extend_from_slice(&2_u16.to_le_bytes());
    wave.extend_from_slice(&8_u16.to_le_bytes());
    wave.extend_from_slice(b"data");
    wave.extend_from_slice(&0_u32.to_le_bytes());
    // The extension says `.txt` and the bytes say WAVE. The walker must report the bytes.
    std::fs::write(root.join("nested/MISLEADING.TXT"), &wave).expect("written");
    std::fs::write(root.join("nested/deeper/notes.txt"), b"plain text\n").expect("written");
    std::fs::write(root.join("empty.bin"), b"").expect("written");

    let rows = loose::inventory(&root).expect("the fixture tree walks");
    let by_path: BTreeMap<String, &loose::LooseFile> = rows
        .iter()
        .map(|row| (row.relative_path.clone(), row))
        .collect();
    assert_eq!(
        by_path.keys().cloned().collect::<Vec<_>>(),
        vec![
            "empty.bin".to_owned(),
            "nested/MISLEADING.TXT".to_owned(),
            "nested/deeper/notes.txt".to_owned(),
        ],
        "the walk must recurse and report relative, slash-joined paths"
    );

    let misleading = by_path["nested/MISLEADING.TXT"];
    assert_eq!(misleading.magic, MagicSignature::WaveAudio);
    assert_eq!(misleading.size as usize, wave.len());
    assert_eq!(misleading.sha256, loose::sha256_hex(&wave));
    assert_eq!(misleading.extension, "txt", "the extension is lowercased");
    // `probe` sees the real name; the WAVE magic outranks the `.txt` extension inside it too.
    assert_eq!(misleading.probe_kind, "wave-audio");
    assert_eq!(misleading.probe_error, None);

    let empty = by_path["empty.bin"];
    assert_eq!(empty.size, 0);
    assert_eq!(empty.sha256, loose::sha256_hex(b""));
    assert_eq!(empty.probe_kind, "empty");

    let notes = by_path["nested/deeper/notes.txt"];
    assert_eq!(notes.magic, MagicSignature::AsciiText);
    assert_eq!(notes.probe_kind, "text");

    std::fs::remove_dir_all(&root).expect("the fixture tree is removed");
}

// ---------------------------------------------------------------------------------------------
// Corpus-gated
// ---------------------------------------------------------------------------------------------

/// The installed English directory, when one was named.
fn game_directory() -> PathBuf {
    std::env::var_os("LOM_GAME_DIR")
        .map(PathBuf::from)
        .expect("set LOM_GAME_DIR to the installed English directory")
}

/// Re-encoding the installed files must reproduce them byte for byte.
///
/// This is the check that the parser read the real thing, as opposed to a fixture shaped like the
/// real thing: the input is whatever is on disk, and every byte the parser misplaced comes back
/// wrong.
#[test]
#[ignore = "needs LOM_GAME_DIR"]
fn the_installed_configuration_files_round_trip() {
    let directory = game_directory();

    let lom = std::fs::read(directory.join("lom.cfg")).expect("lom.cfg is installed");
    let parsed = LomConfig::parse(&lom).expect("lom.cfg parses");
    assert_eq!(parsed.to_bytes(), lom, "lom.cfg did not re-encode exactly");

    let settings =
        std::fs::read(directory.join("settings.cfg")).expect("settings.cfg is installed");
    let parsed = SettingsConfig::parse(&settings).expect("settings.cfg parses");
    assert!(
        parsed.round_trips(&settings),
        "settings.cfg did not re-encode exactly"
    );
    assert!(
        parsed.unparsed.is_empty(),
        "settings.cfg has records this parser could not split: {:?}",
        parsed.unparsed
    );
}

/// The installed files must be the ones the committed report describes.
///
/// Digest, not mtime: the GS5R3 profile is played, so its logs and saves move constantly, and a
/// timestamp check would fail for reasons that say nothing about the parsers.
#[test]
#[ignore = "needs LOM_GAME_DIR and LOM_PROFILE"]
fn the_installed_configuration_matches_the_committed_report() {
    let directory = game_directory();
    let profile = std::env::var("LOM_PROFILE").expect("set LOM_PROFILE alongside LOM_GAME_DIR");
    let fields = config_fields();
    for name in ["lom.cfg", "settings.cfg"] {
        let bytes = std::fs::read(directory.join(name)).expect("the file is installed");
        let recorded = fields
            .get(&(profile.clone(), name.to_owned(), "sha256".to_owned()))
            .unwrap_or_else(|| panic!("{profile}/{name} is in the committed report"));
        assert_eq!(
            &loose::sha256_hex(&bytes),
            recorded,
            "{profile}/{name} on disk is not the file the report describes"
        );
    }
}

/// The whole sweep, re-run against the tree, must reproduce the committed table.
#[test]
#[ignore = "needs LOM_INSTALL_ROOT and LOM_PROFILE"]
fn the_committed_inventory_reproduces() {
    let root = std::env::var_os("LOM_INSTALL_ROOT")
        .map(PathBuf::from)
        .expect("set LOM_INSTALL_ROOT to the installed game root");
    let profile = std::env::var("LOM_PROFILE").expect("set LOM_PROFILE alongside LOM_INSTALL_ROOT");
    let committed = inventory_rows(&profile);
    let fresh = loose::inventory(&root).expect("the tree walks");
    assert_eq!(
        fresh.len(),
        committed.len(),
        "the tree now holds {} files against {} in the report",
        fresh.len(),
        committed.len()
    );
    for row in fresh {
        let recorded = committed
            .get(&row.relative_path)
            .unwrap_or_else(|| panic!("{} is not in the committed report", row.relative_path));
        // Every column, not just the digest. Comparing two of eight let a change that broke the
        // classification of all 337 map components past this check unnoticed.
        assert_eq!(recorded["sha256"], row.sha256, "{}", row.relative_path);
        assert_eq!(
            recorded["size"],
            row.size.to_string(),
            "{}",
            row.relative_path
        );
        assert_eq!(
            recorded["magic"],
            row.magic.to_string(),
            "{}",
            row.relative_path
        );
        assert_eq!(
            recorded["probe_kind"], row.probe_kind,
            "{}",
            row.relative_path
        );
        assert_eq!(
            recorded["probe_error"],
            row.probe_error.clone().unwrap_or_else(|| "-".to_owned()),
            "{}",
            row.relative_path
        );
        assert_eq!(
            recorded["extension"],
            if row.extension.is_empty() {
                "-".to_owned()
            } else {
                row.extension.clone()
            },
            "{}",
            row.relative_path
        );
        assert_eq!(
            recorded["extension_disagrees"],
            if row.extension_disagrees_with_magic() {
                "yes"
            } else {
                "no"
            },
            "{}",
            row.relative_path
        );
    }
}

/// The committed configuration report, re-derived from the installed files -- **every row**.
///
/// This is the check the first version of this work left missing, and the second version left
/// incomplete. `the_committed_inventory_reproduces` does this for the inventory; nothing did it for
/// `config-fields.tsv`, so the two report-backed identity tests read a table no test re-derived.
///
/// The comparison is for **exact equality of the whole field map**, not a spot-check of the fields
/// that looked interesting. A subset check passed while `format`, `round-trips`,
/// `unparsed-records` and `terminated` went unverified -- so a parser that reported
/// `terminated = false` while preserving all 23 entries, or one whose round-trip broke, would have
/// slipped through. Extra stale rows are rejected for the same reason: a report may not carry a
/// field the parser no longer produces.
#[test]
#[ignore = "needs LOM_GAME_DIR and LOM_PROFILE"]
fn the_committed_configuration_report_reproduces() {
    let directory = game_directory();
    let profile = std::env::var("LOM_PROFILE").expect("set LOM_PROFILE alongside LOM_GAME_DIR");
    let committed = config_fields();

    // Built to mirror `describe_loose_config`'s output exactly, field for field.
    let mut actual: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut put = |file: &str, field: &str, value: String| {
        assert!(
            actual
                .insert((file.to_owned(), field.to_owned()), value)
                .is_none(),
            "{file}/{field} was derived twice"
        );
    };

    let bytes = std::fs::read(directory.join("lom.cfg")).expect("lom.cfg is installed");
    let config = LomConfig::parse(&bytes).expect("lom.cfg parses");
    put("lom.cfg", "size", bytes.len().to_string());
    put("lom.cfg", "sha256", loose::sha256_hex(&bytes));
    put("lom.cfg", "format", "lom.cfg".to_owned());
    put(
        "lom.cfg",
        "round-trips",
        (config.to_bytes() == bytes).to_string(),
    );
    put(
        "lom.cfg",
        "last-music-volume",
        config.last_music_volume.to_string(),
    );
    put(
        "lom.cfg",
        "last-sound-fx-volume",
        config.last_sound_fx_volume.to_string(),
    );
    put(
        "lom.cfg",
        "last-speech-volume",
        config.last_speech_volume.to_string(),
    );
    put(
        "lom.cfg",
        "last-ambient-volume",
        config.last_ambient_volume.to_string(),
    );
    match &config.help_panel_checks {
        Some(checks) => {
            put("lom.cfg", "help-panel-count", checks.len().to_string());
            put(
                "lom.cfg",
                "help-panel-checks",
                checks
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        None => {
            put("lom.cfg", "help-panel-count", "absent".to_owned());
            put("lom.cfg", "help-panel-checks", "absent".to_owned());
        }
    }
    put(
        "lom.cfg",
        "balkoth-kill-counter",
        config.balkoth_kill_counter.to_string(),
    );
    put(
        "lom.cfg",
        "center-on-movement",
        config.center_on_movement.to_string(),
    );
    put("lom.cfg", "install-guid", config.install_guid_text());
    put(
        "lom.cfg",
        "building-speech-flag",
        config.building_speech_flag.to_string(),
    );
    put(
        "lom.cfg",
        "show-completed-quests",
        config.show_completed_quests.to_string(),
    );
    put(
        "lom.cfg",
        "used-drawblt",
        (config.used_drawblt as i32).to_string(),
    );

    let bytes = std::fs::read(directory.join("settings.cfg")).expect("settings.cfg is installed");
    let settings = SettingsConfig::parse(&bytes).expect("settings.cfg parses");
    put("settings.cfg", "size", bytes.len().to_string());
    put("settings.cfg", "sha256", loose::sha256_hex(&bytes));
    put("settings.cfg", "format", "settings.cfg".to_owned());
    put(
        "settings.cfg",
        "round-trips",
        settings.round_trips(&bytes).to_string(),
    );
    put(
        "settings.cfg",
        "records",
        settings.entries.len().to_string(),
    );
    put(
        "settings.cfg",
        "unparsed-records",
        settings.unparsed.len().to_string(),
    );
    put(
        "settings.cfg",
        "terminated",
        settings.terminated.to_string(),
    );
    for entry in &settings.entries {
        put(
            "settings.cfg",
            &format!("setting:{}", entry.key),
            entry.raw_value.clone(),
        );
    }

    let recorded: BTreeMap<(String, String), String> = committed
        .iter()
        .filter(|((row_profile, _, _), _)| row_profile == &profile)
        .map(|((_, file, field), value)| ((file.clone(), field.clone()), value.clone()))
        .collect();

    for (key, value) in &actual {
        let Some(found) = recorded.get(key) else {
            panic!(
                "{profile}: {}/{} is not in the committed report",
                key.0, key.1
            );
        };
        assert_eq!(
            found, value,
            "{profile}: {}/{} was re-derived as {value}",
            key.0, key.1
        );
    }
    for key in recorded.keys() {
        assert!(
            actual.contains_key(key),
            "{profile}: the committed report carries {}/{}, which the parsers no longer produce",
            key.0,
            key.1
        );
    }
}

/// The one hop the committed tables cannot preserve, read from the image instead.
///
/// `docs/loose-files.md` pairs each `lom.cfg` audio word with a channel. The chain is: file word ->
/// config-object slot `0x5aa144..0x5aa150` -> one of `setlastaudiosettings`'s four calls -> a
/// `set*volume` helper. Only the **last** link is in the committed tables, and it is solid:
/// `setmusicvolume`'s own row lists call target `0x479a00` and a write to sound-object `+0x1368`.
///
/// The middle link is not, and cannot be. `operator-field-access.tsv` is produced by walking into
/// the callees listed in `operator-bodies.tsv`, so the two tables are **one extraction pass read
/// twice**: both record the *set* of four globals and the *set* of four call targets, and neither
/// records which global was pushed before which call. Exchanging the loads of `0x5aa144` and
/// `0x5aa148` would leave every committed table byte-identical while swapping the meanings of
/// `lom.cfg[0x00]` and `[0x04]`.
///
/// So this decodes `setlastaudiosettings` and asserts the ordered pairing directly. It replaces a
/// hand disassembly that nothing could re-run.
#[test]
#[ignore = "needs LOM_GAME_DIR"]
fn each_audio_word_reaches_the_helper_its_channel_is_named_after() {
    use iced_x86::{Decoder, DecoderOptions, Mnemonic, OpKind};
    use lom_asset_viewer::native_table::PeImage;

    // Global -> the helper the file word is claimed to reach, in file order.
    const EXPECTED: [(u32, u32, &str); 4] = [
        (0x005a_a144, 0x0047_9a00, "music"),
        (0x005a_a148, 0x0047_9b40, "sound effects"),
        (0x005a_a14c, 0x0047_9c30, "speech"),
        (0x005a_a150, 0x0047_9d10, "ambient"),
    ];
    const SET_LAST_AUDIO_SETTINGS: u32 = 0x0048_7520;

    let bytes = std::fs::read(game_directory().join("lomse.exe")).expect("lomse.exe is installed");
    let image = PeImage::parse(&bytes).expect("lomse.exe is a 32-bit PE");
    let start = image
        .file_offset(SET_LAST_AUDIO_SETTINGS)
        .expect("setlastaudiosettings is inside a mapped section");
    // The body is 79 bytes to its `ret`; 0x60 covers it without running into the next function.
    let mut decoder = Decoder::with_ip(
        32,
        &image.bytes()[start..start + 0x60],
        u64::from(SET_LAST_AUDIO_SETTINGS),
        DecoderOptions::NONE,
    );

    let globals: Vec<u32> = EXPECTED.iter().map(|(global, _, _)| *global).collect();
    let mut pending: Option<u32> = None;
    let mut observed: Vec<(u32, u32)> = Vec::new();
    for instruction in decoder.iter() {
        match instruction.mnemonic() {
            // `mov r32, [imm32]` -- the load of one persisted word.
            Mnemonic::Mov if instruction.op1_kind() == OpKind::Memory => {
                let address = instruction.memory_displacement32();
                if globals.contains(&address) {
                    assert!(
                        pending.is_none(),
                        "two audio words were loaded with no call between them, so the pairing \
                         cannot be read off the order"
                    );
                    pending = Some(address);
                }
            }
            Mnemonic::Call => {
                if let Some(global) = pending.take() {
                    observed.push((global, instruction.near_branch32()));
                }
            }
            Mnemonic::Ret => break,
            _ => {}
        }
    }

    let expected: Vec<(u32, u32)> = EXPECTED
        .iter()
        .map(|(global, helper, _)| (*global, *helper))
        .collect();
    assert_eq!(
        observed,
        expected,
        "the audio words do not reach the helpers this repository says they do; \
         expected {}",
        EXPECTED
            .iter()
            .map(|(global, helper, channel)| format!("{global:#010x}->{helper:#010x} ({channel})"))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// Every `.wav` and `.smk` really is what its extension says.
///
/// Stated before looking: if `English/Wav/` and `English/smk/` are plain media directories the
/// signature matches the extension in all 65 files; if either directory hides something the counts
/// come apart, and that is the finding. They matched, so this pins it.
#[test]
#[ignore = "needs LOM_GAME_DIR"]
fn the_media_directories_hold_only_what_they_claim() {
    let directory = game_directory();
    for (subdirectory, expected) in [
        ("Wav", MagicSignature::WaveAudio),
        ("smk", MagicSignature::SmackerVideo),
    ] {
        let rows = loose::inventory(&directory.join(subdirectory)).expect("the directory walks");
        assert!(!rows.is_empty(), "{subdirectory} is empty");
        for row in rows {
            let bytes = std::fs::read(directory.join(subdirectory).join(&row.relative_path))
                .expect("the file reads");
            assert_eq!(
                magic_signature(&bytes),
                expected,
                "{subdirectory}/{}",
                row.relative_path
            );
        }
    }
}
