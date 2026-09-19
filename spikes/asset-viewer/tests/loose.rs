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
        rows.insert(
            (
                fields[0].to_owned(),
                fields[1].to_owned(),
                fields[2].to_owned(),
            ),
            fields[3].to_owned(),
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

/// The `lom.cfg` layout equation, checked between two independently produced reports.
///
/// `20 + 4 * help-panel-count + 36` is the whole structural claim: four volume words and a count
/// word ahead of the vector (4x4 + 4 = 20), then two words, a 16-byte GUID and three words behind
/// it (4 + 4 + 16 + 4 + 4 + 4 = 36). Every term is a field the writer emits, in its order; the
/// total is not sourced as a total. The
/// inventory measured the file; the parser measured the count. If the layout is wrong the two
/// numbers stop agreeing, and no amount of the parser agreeing with itself can hide that.
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
        assert_eq!(recorded["sha256"], row.sha256, "{}", row.relative_path);
        assert_eq!(
            recorded["magic"],
            row.magic.to_string(),
            "{}",
            row.relative_path
        );
    }
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
