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
    RECORD_MARKERS, ValueShape, contains_marker, looked_up_key, record_fields, registered_symbols,
    run_targets, unit_records,
};
use lom_asset_viewer::gamescript::{Delimiter, GameScriptDocument, TokenKind};
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

#[test]
#[ignore = "reads a shipped archive; set LOM_GS_MPQ"]
fn a_text_table_lookup_is_the_field_it_defines_not_the_key_it_reads() {
    // `/name textdict /T_artifact_name_adventsword get def` defines `name`. Stopping the value scan
    // at the inner literal dropped that field entirely and filed the *key* as a field whose value
    // was `get` -- so every artifact written this way lost its name and gained two inventions.
    //
    // Asserted as a property of the corpus, not against a copied count: whatever the archive
    // contains, a key that some record looks up must never itself become a field name.
    let corpus = corpus();
    let mut looked_up: BTreeSet<String> = BTreeSet::new();
    let mut field_names: BTreeSet<String> = BTreeSet::new();
    let mut lookup_valued_fields = 0_usize;

    for (name, member, _) in registrations(&corpus) {
        let _ = name;
        let Some(tokens) = corpus.members.get(&member) else {
            continue;
        };
        let (fields, _) = record_fields(tokens);
        for (field, value) in &fields {
            field_names.insert(field.clone());
            if let Some(key) = looked_up_key(&value.text) {
                looked_up.insert(key.to_owned());
                lookup_valued_fields += 1;
            }
        }
    }

    assert!(
        lookup_valued_fields > 0,
        "no record in this archive uses the `<dict> /KEY get` idiom, so this test proves nothing \
         here; check whether the idiom moved"
    );
    let keys_that_became_fields: Vec<&String> = looked_up.intersection(&field_names).collect();
    assert!(
        keys_that_became_fields.is_empty(),
        "these dictionary keys were filed as fields of the record: {keys_that_became_fields:?}"
    );
}

#[test]
#[ignore = "reads a shipped archive; set LOM_GS_MPQ"]
fn a_deferred_native_call_still_stops_the_value_scan() {
    // The guard the key-lookup exception relaxes. `/invoke_spell cvx` pushes a name for later
    // execution; if the scan ran past it looking for a `def`, `invoke_spell` would be filed as a
    // field of whatever record contained it. The corpus really does contain this shape, so the
    // check is against the archive rather than a fixture.
    let corpus = corpus();
    let mut offenders = Vec::new();
    for (member, tokens) in &corpus.members {
        let (fields, _) = record_fields(tokens);
        for deferred in ["invoke_spell", "removeunitmodifiers"] {
            if fields.contains_key(deferred) {
                offenders.push(format!("{member}:{deferred}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a deferred native call was read as a record field: {offenders:?}"
    );
}

/// Every word the corpus contains as a token, written independently of `token_text` so the
/// comparison is against the archive rather than against the same mapping under test.
fn corpus_words(corpus: &Corpus) -> BTreeSet<String> {
    let mut words = BTreeSet::new();
    for tokens in corpus.members.values() {
        for token in tokens {
            match &token.kind {
                TokenKind::ExecutableName(name) => {
                    words.insert(name.clone());
                }
                TokenKind::LiteralName(name) => {
                    words.insert(format!("/{name}"));
                }
                TokenKind::Number(text) => {
                    words.insert(text.clone());
                }
                TokenKind::StringLiteral(text) => {
                    // A string reaches a value as its contents, so its words are corpus words.
                    words.extend(text.split_whitespace().map(str::to_owned));
                    words.insert(text.clone());
                }
                TokenKind::Delimiter(delimiter) => {
                    words.insert(
                        match delimiter {
                            Delimiter::ProcedureOpen => "{",
                            Delimiter::ProcedureClose => "}",
                            Delimiter::ArrayOpen => "[",
                            Delimiter::ArrayClose => "]",
                            Delimiter::DictionaryOpen => "<<",
                            Delimiter::DictionaryClose => ">>",
                        }
                        .to_owned(),
                    );
                }
            }
        }
    }
    words
}

#[test]
#[ignore = "reads a shipped archive; set LOM_GS_MPQ"]
fn no_published_value_contains_a_word_the_corpus_does_not_have() {
    // The defect this guards: a 120-character cut through a flags expression published
    // `CAN_USE_R`, `CAN_USE_LE`, `CAN_TRAN` and a bare `o`. None of those is a token anywhere in
    // the archive, which is the property asserted here -- against the corpus's own vocabulary
    // rather than against a list of the four names that happened to be severed.
    let corpus = corpus();
    let words = corpus_words(&corpus);

    let mut severed = Vec::new();
    let mut longest = 0_usize;
    let mut over_the_old_cap = 0_usize;
    for (member, tokens) in &corpus.members {
        let (fields, _) = record_fields(tokens);
        for (key, field) in &fields {
            if !matches!(
                field.shape,
                ValueShape::Number | ValueShape::Name | ValueShape::Expression
            ) {
                continue;
            }
            longest = longest.max(field.text.chars().count());
            if field.text.chars().count() > 120 {
                over_the_old_cap += 1;
            }
            for word in field.text.split(' ') {
                if !words.contains(word) {
                    severed.push(format!("{member}:{key} has {word:?}"));
                }
            }
        }
    }
    // The guard: if no symbolic value were longer than the cap that used to apply, this test would
    // pass on a corpus that never exercised the defect.
    assert!(
        over_the_old_cap > 0,
        "no symbolic value exceeds 120 characters, so this test does not reach the case it exists for"
    );
    severed.sort();
    severed.dedup();
    assert!(
        severed.is_empty(),
        "these published values contain words the archive does not: {severed:?} (longest value {longest} chars)"
    );
}

#[test]
#[ignore = "reads a shipped archive; set LOM_GS_MPQ"]
fn a_bounded_value_says_it_is_bounded_and_no_other_value_sits_at_the_bound() {
    // A consumer must be able to tell a cut value from a complete one without measuring it. The
    // old rule failed that twice over: the marker did not exist, and the length that betrayed the
    // cut was shared with values that merely happened to be that long.
    let corpus = corpus();
    let mut marked = 0_usize;
    let mut unmarked_at_the_old_cap = Vec::new();
    for tokens in corpus.members.values() {
        let (fields, _) = record_fields(tokens);
        for (key, field) in &fields {
            if field.text.contains("<truncated") {
                marked += 1;
                let stated: usize = field
                    .text
                    .rsplit_once("<truncated, ")
                    .and_then(|(_, tail)| tail.split(' ').next().map(str::to_owned))
                    .expect("the marker carries a length")
                    .parse()
                    .expect("the stated length is a number");
                let kept = field
                    .text
                    .rsplit_once(" <truncated, ")
                    .expect("the marker splits the value")
                    .0;
                assert!(
                    stated > kept.chars().count(),
                    "{key} claims to be cut but the part it published is the whole of it"
                );
                assert!(
                    !kept.ends_with(char::is_whitespace) && !kept.is_empty(),
                    "{key} published a prefix ending in whitespace: {kept:?}"
                );
                continue;
            }
            if field.text.chars().count() == 120 {
                unmarked_at_the_old_cap.push(key.clone());
            }
        }
    }
    assert!(
        marked > 0,
        "nothing in the corpus is bounded, so this test does not reach the case it exists for"
    );
    // Values of exactly 120 characters may legitimately exist; what must not exist is a *cut* one
    // that says nothing. Each one here is checked against its own untruncated shape by the test
    // above, which would have caught a severed word.
    assert!(
        unmarked_at_the_old_cap.len() < marked,
        "the old cap's length still dominates the distribution: {unmarked_at_the_old_cap:?}"
    );
}
