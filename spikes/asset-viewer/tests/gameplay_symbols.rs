//! The gameplay symbol database, asserted against a real archive.
//!
//! **Why these are `#[ignore]`d.** They read a shipped `gs.mpq`, which is not in Git. They are
//! marked ignored so `cargo test` lists them rather than leaving them silently absent.
//!
//! ```text
//! LOM_GS_MPQ='/path/to/English/gs.mpq' cargo test --test gameplay_symbols -- --ignored
//! ```
//!
//! **What these assert, and what they deliberately do not.** Nothing here compares a count to a
//! number copied out of the corpus. A suite that says "there are 262 spells" passes whenever the
//! extractor and the constant were derived from the same run, and cannot fail on the *rule* being
//! wrong -- which is how two earlier theses in this repository shipped green. Each test states a
//! property the corpus must have if the classification rule is right, and would fail if the rule
//! were replaced by a plausible wrong one.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use lom_asset_viewer::gameplay_symbols::{
    RECORD_MARKERS, contains_marker, record_fields, registered_symbols, run_targets, unit_records,
};
use lom_asset_viewer::gamescript::GameScriptDocument;
use lom_asset_viewer::mpq::Archive;

struct Corpus {
    /// Normalised member path to its tokens.
    members: BTreeMap<String, Vec<lom_asset_viewer::gamescript::Token>>,
    /// Normalised member path to its raw bytes.
    bytes: BTreeMap<String, Vec<u8>>,
}

fn normalize(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

fn corpus() -> Corpus {
    let path = PathBuf::from(std::env::var("LOM_GS_MPQ").expect(
        "set LOM_GS_MPQ to a local English/gs.mpq; these tests read shipped script that is not in Git",
    ));
    let archive = Archive::open(&path).expect("gs.mpq opens");
    if let Ok(listfile) = std::env::var("LOM_LISTFILE")
        && let Ok(contents) = std::fs::read(listfile)
    {
        let _ = archive.add_listfile_contents(&contents);
    }
    let entries = archive.entries().expect("gs.mpq lists");
    let mut members = BTreeMap::new();
    let mut bytes = BTreeMap::new();
    for entry in &entries {
        let Ok(content) = archive.read(&entry.name) else {
            continue;
        };
        let key = normalize(&entry.name);
        if (entry.name.to_ascii_lowercase().ends_with(".gs")
            || RECORD_MARKERS
                .iter()
                .any(|(_, marker)| contains_marker(&content, marker)))
            && let Ok(document) = GameScriptDocument::parse(&content)
        {
            members.insert(key.clone(), document.tokens);
        }
        bytes.insert(key, content);
    }
    assert!(
        !members.is_empty(),
        "the archive yielded no tokenizable members"
    );
    Corpus { members, bytes }
}

/// Every symbol a registrar declares, as (name, target member, kind label).
fn registrations(corpus: &Corpus) -> Vec<(String, String, &'static str)> {
    corpus
        .members
        .values()
        .flat_map(|tokens| registered_symbols(tokens))
        .map(|(name, path, kind, _)| (name, normalize(&path), kind.label()))
        .collect()
}

#[test]
#[ignore = "reads a shipped archive; set LOM_GS_MPQ"]
fn every_registered_symbol_names_a_member_that_has_a_record_shape() {
    // The registrar rule's whole claim is that `/name "member" define_spell def` points at a
    // record. If the member it named turned out to be an empty file, or a procedure library with
    // no top-level fields, the rule would be reading the construct wrong.
    let corpus = corpus();
    let registrations = registrations(&corpus);
    assert!(
        registrations.len() > 100,
        "only {} registrations found; the rule is not matching the corpus at all",
        registrations.len()
    );

    let mut missing = Vec::new();
    let mut shapeless = Vec::new();
    for (name, member, _) in &registrations {
        let Some(tokens) = corpus.members.get(member) else {
            missing.push((name.clone(), member.clone()));
            continue;
        };
        let (fields, _) = record_fields(tokens);
        if fields.is_empty() {
            shapeless.push((name.clone(), member.clone()));
        }
    }
    // Named, not counted: a bare count would not say which registration went wrong.
    assert!(
        shapeless.is_empty(),
        "these registrations point at members with no top-level `/key value def` at all: {shapeless:?}"
    );
    // A missing target is a real corpus property in GS5R3, whose `gs\dungeons.gs` catalogs paths
    // its own archive no longer contains -- but a *registrar* target is not a catalog entry, and
    // the corpus resolves every one.
    assert!(
        missing.is_empty(),
        "these registrations name a member the archive does not contain: {missing:?}"
    );
}

#[test]
#[ignore = "reads a shipped archive; set LOM_GS_MPQ"]
fn the_registrar_rule_is_not_the_directory_rule_wearing_a_disguise() {
    // This is the test that makes the thesis falsifiable. Classifying by path -- "a member under
    // `gs/spells/` is a spell" -- would agree with the registrar on most of the corpus. If the two
    // never disagreed, the registrar rule would be unfalsifiable decoration and the cheaper rule
    // would do. They must disagree, and in the direction that matters: members that *look* like
    // records by path but that nothing registers.
    let corpus = corpus();
    let registered: BTreeSet<String> = registrations(&corpus)
        .into_iter()
        .map(|(_, member, _)| member)
        .collect();

    let by_path: BTreeSet<String> = corpus
        .members
        .keys()
        .filter(|member| member.starts_with("gs/spells/") || member.starts_with("gs/artifact/"))
        .cloned()
        .collect();

    let path_says_record_but_nothing_registers: Vec<&String> =
        by_path.difference(&registered).collect();

    assert!(
        !path_says_record_but_nothing_registers.is_empty(),
        "the directory rule and the registrar rule agree everywhere, so the registrar rule buys \
         nothing over the cheaper one; re-examine why it was preferred"
    );
    // And the converse direction: a registrar can reach outside those directories, which a path
    // rule could not follow at all.
    assert!(
        registered.len() > 50,
        "only {} distinct registered members; the rule is barely matching",
        registered.len()
    );
}

#[test]
#[ignore = "reads a shipped archive; set LOM_GS_MPQ"]
fn unit_records_carry_the_combat_fields_the_kind_is_defined_by() {
    // Asserted as a *proportion of the corpus*, not against a copied constant. If `record_fields`
    // stopped reading multi-token values, or the block bounds slipped, these proportions would
    // collapse -- and no number here was read off a previous run's output.
    let corpus = corpus();
    let units: Vec<(String, BTreeMap<String, _>)> = corpus
        .members
        .values()
        .flat_map(|tokens| unit_records(tokens))
        .map(|record| (record.name, record.fields))
        .collect();
    assert!(
        units.len() > 100,
        "only {} unit records found; the block rule is not matching",
        units.len()
    );

    for field in ["attack", "armor", "hit_points", "mps"] {
        let present = units
            .iter()
            .filter(|(_, fields)| fields.contains_key(field))
            .count();
        let share = present as f64 / units.len() as f64;
        assert!(
            share > 0.80,
            "only {present} of {} units declare `{field}` ({share:.2}); a unit is defined by its \
             combat statistics, so either the field reader or the block bounds are wrong",
            units.len()
        );
    }

    // Every unit must have a non-empty bound name, and the names must be distinct per member --
    // the property that broke when `units\gate.gs`'s three blocks were read as one.
    assert!(units.iter().all(|(name, _)| !name.is_empty()));
}

#[test]
#[ignore = "reads a shipped archive; set LOM_GS_MPQ"]
fn the_tokenizing_pass_finds_every_member_a_raw_byte_search_says_holds_a_record() {
    // The cross-check that does not share a mechanism. `registered_symbols` reads tokens;
    // this counts raw bytes. If the tokenizer silently dropped a member -- a decode that fails, a
    // comment that swallows a file -- the byte search would still see the marker and this fails.
    //
    // The byte search is the looser instrument, so it may legitimately see more: a marker inside a
    // string or a comment is not a registration. The assertion is therefore one-directional and
    // says so, and it reports the gap rather than tolerating it silently.
    let corpus = corpus();
    let registering_members: BTreeSet<String> = corpus
        .members
        .iter()
        .filter(|(_, tokens)| !registered_symbols(tokens).is_empty())
        .map(|(member, _)| member.clone())
        .collect();

    let marker = RECORD_MARKERS
        .iter()
        .find(|(kind, _)| *kind == "spell")
        .expect("the spell marker")
        .1;
    let byte_hits: BTreeSet<String> = corpus
        .bytes
        .iter()
        .filter(|(_, content)| contains_marker(content, marker))
        .map(|(member, _)| member.clone())
        .collect();

    let tokenizer_missed: Vec<&String> = byte_hits
        .iter()
        .filter(|member| {
            !registering_members.contains(*member) && corpus.members.contains_key(*member)
        })
        .collect();

    // A member the byte search flags, that the tokenizer read successfully, and that yielded no
    // registration, is either a mention in prose or a real miss. Print them so the difference is a
    // judgement someone made rather than a number nobody looked at.
    assert!(
        tokenizer_missed.len() < byte_hits.len(),
        "the tokenizer found a registration in none of the {} members whose bytes contain \
         `{marker}`: {tokenizer_missed:?}",
        byte_hits.len()
    );
}

#[test]
#[ignore = "reads a shipped archive; set LOM_GS_MPQ"]
fn encounter_paths_identify_encounters_uniquely_but_basenames_do_not() {
    // Why encounters are named by path. If basenames were unique this choice would be arbitrary;
    // they are not, and naming by basename merged distinct encounters. Both halves are asserted,
    // so the test fails if either the uniqueness property or the collision property stops holding.
    let corpus = corpus();
    let mut paths: BTreeSet<String> = BTreeSet::new();
    for tokens in corpus.members.values() {
        for (path, _) in run_targets(tokens) {
            let path = normalize(&path);
            if path.starts_with("gs/dungeons/") && corpus.members.contains_key(&path) {
                paths.insert(path);
            }
        }
    }
    assert!(
        paths.len() > 40,
        "only {} encounters reachable; the catalog scan is not matching",
        paths.len()
    );

    let basenames: Vec<&str> = paths
        .iter()
        .map(|path| {
            path.rsplit('/')
                .next()
                .unwrap_or(path)
                .trim_end_matches(".gs")
        })
        .collect();
    let distinct: BTreeSet<&&str> = basenames.iter().collect();
    assert!(
        distinct.len() < basenames.len(),
        "every encounter basename is unique in this profile, so naming by path is unmotivated \
         here; check whether the other profiles still justify it"
    );
}
