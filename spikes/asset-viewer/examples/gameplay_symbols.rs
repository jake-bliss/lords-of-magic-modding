//! Build the semantic symbol/index database for gameplay data, and the searchable reference.
//!
//! This is Phase 2's remaining deliverable. It reads each profile's `gs.mpq`, recovers every
//! gameplay symbol it can classify with stated evidence, records that symbol's fields, its static
//! references, and how its definition compares across the three profiles, and writes the result as
//! TSVs plus a generated Markdown reference with one stable anchor per symbol.
//!
//! **Why TSV.** `reports/gs/` and `reports/natives/` already work this way, the files stay diffable
//! in Git so a regeneration shows up as a reviewable change, and `reports/**/*.csv` is gitignored
//! while `.tsv` is not -- the repository has already decided that aggregate TSV is the committed
//! form. A database file would be none of those things.
//!
//! **What is emitted.** Counts, names, classifications, numeric field values, and positions. A
//! field whose value is a procedure, dictionary or array is recorded as its shape and size, never
//! its body, so no script text reaches the reports.
//!
//! Usage:
//!
//! ```text
//! cargo run --release --example gameplay_symbols -- \
//!     --profile vanilla  --gs '/path/vanilla/English/gs.mpq' \
//!     --profile patch302 --gs '/path/302/English/gs.mpq' \
//!     --profile gs5r3    --gs '/path/gs5r3/English/gs.mpq' \
//!     --listfile ../../artifacts/reference-listfiles/lords-of-magic.txt \
//!     --out ../../reports/gameplay
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use lom_asset_viewer::gameplay_symbols::{
    DefinitionDiff, EvidenceClass, FieldValue, RECORD_MARKERS, Reference, Symbol, SymbolDatabase,
    SymbolKind, ValueShape, anchor_for, compare_definitions, contains_marker, field_statistics,
    qualified, record_fields, references_in, registered_symbols, run_targets, token_fingerprint,
    unit_records,
};
use lom_asset_viewer::gamescript::{GameScriptDocument, Token, TokenKind};
use lom_asset_viewer::mpq::Archive;

/// A profile's label, its recovered database, and the byte/token fingerprint of each of its
/// members. Named because three functions pass it around and clippy is right that the tuple had
/// stopped being readable.
type Built = (String, SymbolDatabase, Fingerprints);

/// Normalised member path to its (raw byte hash, token fingerprint).
type Fingerprints = BTreeMap<String, (u64, u64)>;

/// The directory an encounter member lives under.
///
/// The *catalog* is deliberately not named here. Vanilla and 3.02 catalog their encounters from
/// `gs\\dungeons.gs`, but GS5R3 reorganised the tree into per-faith subdirectories and catalogs
/// them from `gs\\DUNGEONS5.gs` instead, leaving 266 of `dungeons.gs`'s 312 edges pointing at
/// paths its own archive no longer contains. Hardcoding one catalog member found 46 of GS5R3's
/// 314 encounters and reported the other 268 as missing dependencies. Every member is scanned for
/// `run` edges into this directory, and the union is taken.
const ENCOUNTER_PREFIX: &str = "gs/dungeons/";
/// The operator that declares a building type's level range.
const BUILDING_OPERATOR: &str = "define_building_levels";

struct ProfileInput {
    label: String,
    archive: PathBuf,
}

struct Member {
    name: String,
    tokens: Vec<Token>,
    byte_hash: u64,
    token_hash: u64,
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mut profiles: Vec<ProfileInput> = Vec::new();
    let mut listfile: Option<PathBuf> = None;
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
            "--profile" => profiles.push(ProfileInput {
                label: value(index),
                archive: PathBuf::new(),
            }),
            "--gs" => match profiles.last_mut() {
                Some(profile) => profile.archive = PathBuf::from(value(index)),
                None => {
                    eprintln!("--gs must follow a --profile");
                    std::process::exit(2);
                }
            },
            "--listfile" => listfile = Some(PathBuf::from(value(index))),
            "--out" => output = Some(PathBuf::from(value(index))),
            other => {
                eprintln!("unexpected argument {other}");
                std::process::exit(2);
            }
        }
        index += 2;
    }
    if profiles.is_empty() {
        eprintln!("usage: --profile LABEL --gs PATH/gs.mpq [...] [--listfile PATH] [--out DIR]");
        std::process::exit(2);
    }

    let mut built: Vec<Built> = Vec::new();
    for profile in &profiles {
        match build(profile, listfile.as_deref()) {
            Ok(result) => built.push((profile.label.clone(), result.0, result.1)),
            Err(message) => {
                eprintln!("{}: {message}", profile.label);
                std::process::exit(1);
            }
        }
    }

    for (label, database, _) in &built {
        report(label, database);
    }

    if let Some(directory) = output
        && let Err(message) = write_reports(&directory, &built)
    {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

/// Read and tokenize every `.gs` member, then recover symbols from the token streams.
fn build(
    profile: &ProfileInput,
    listfile: Option<&Path>,
) -> Result<(SymbolDatabase, Fingerprints), String> {
    let archive = Archive::open(&profile.archive)
        .map_err(|error| format!("could not open the archive: {error}"))?;
    if let Some(path) = listfile
        && let Ok(contents) = std::fs::read(path)
    {
        let _ = archive.add_listfile_contents(&contents);
    }
    let entries = archive
        .entries()
        .map_err(|error| format!("could not list the archive: {error}"))?;

    let mut database = SymbolDatabase::default();
    let mut members: Vec<Member> = Vec::new();

    // What the `.gs` filter costs, measured rather than assumed. Vanilla's listfile leaves 372
    // entries unnamed, so they are excluded by extension while still being GameScript; a raw byte
    // search -- no tokenizer, no decoding -- says how many hold a record this pass will therefore
    // miss. The two searches share no mechanism, which is the only reason the second one is worth
    // running at all.
    let mut scan: Vec<String> = Vec::new();
    for entry in entries.iter() {
        if entry.name.to_ascii_lowercase().ends_with(".gs") {
            scan.push(entry.name.clone());
            continue;
        }
        database.excluded_members += 1;
        let Ok(bytes) = archive.read(&entry.name) else {
            continue;
        };
        let mut marked = false;
        for (kind, marker) in RECORD_MARKERS {
            if contains_marker(&bytes, marker) {
                marked = true;
                *database
                    .excluded_members_with_a_record_marker
                    .entry(kind.to_owned())
                    .or_default() += 1;
            }
        }
        // An entry the listfile could not name is still GameScript. Vanilla has 372 of them, and
        // 12 hold unit records -- one of which, `boat`, no named member defines anywhere. The raw
        // byte marker is the gate: it shares no mechanism with the tokenizer, so it is a real
        // second opinion about what an entry is, and it admits nothing that lacks a record.
        if marked {
            scan.push(entry.name.clone());
        }
    }

    for name in &scan {
        let entry = entries
            .iter()
            .find(|entry| &entry.name == name)
            .expect("name came from entries");
        let Ok(bytes) = archive.read(&entry.name) else {
            database.unreadable_members.push(entry.name.clone());
            continue;
        };
        let Ok(document) = GameScriptDocument::parse(&bytes) else {
            database.unreadable_members.push(entry.name.clone());
            continue;
        };
        database.parsed_members += 1;
        members.push(Member {
            name: entry.name.clone(),
            byte_hash: fnv1a(&bytes),
            token_hash: token_fingerprint(&document),
            tokens: document.tokens,
        });
    }

    // Members are addressed case-insensitively with either separator, because the corpus writes
    // `"gs/spells/FIRE/bolt_fire.gs"` inside an archive whose own entry is `gs\spells\...`.
    let by_path: BTreeMap<String, usize> = members
        .iter()
        .enumerate()
        .map(|(index, member)| (normalize_member(&member.name), index))
        .collect();
    let fingerprints: Fingerprints = members
        .iter()
        .map(|member| {
            (
                normalize_member(&member.name),
                (member.byte_hash, member.token_hash),
            )
        })
        .collect();

    // 1. Spells and artifacts: a registrar operator names the kind.
    for member in &members {
        for (name, path, kind, line) in registered_symbols(&member.tokens) {
            let target = normalize_member(&path);
            let Some(&defining) = by_path.get(&target) else {
                database
                    .unresolved_registrations
                    .push((name.clone(), path.clone()));
                continue;
            };
            let (fields, _) = record_fields(&members[defining].tokens);
            insert(
                &mut database,
                Symbol {
                    name,
                    kind,
                    evidence: EvidenceClass::RegisteredByOperator,
                    member: members[defining].name.clone(),
                    line: 1,
                    offset: 0,
                    registered_in: member.name.clone(),
                    fields,
                },
                line,
            );
        }
    }

    // 2. Units: a delimited block, bound by the `/name exch def` that follows it.
    for member in &members {
        for record in unit_records(&member.tokens) {
            let line = record.line;
            insert(
                &mut database,
                Symbol {
                    name: record.name,
                    kind: SymbolKind::Unit,
                    evidence: EvidenceClass::DelimitedBlock,
                    member: member.name.clone(),
                    line,
                    offset: record.offset,
                    registered_in: String::new(),
                    fields: record.fields,
                },
                line,
            );
        }
    }

    // 3. Encounters: members the dungeon catalog reaches by `run`. The catalog states membership;
    //    the kind comes from the catalog's own identity, so this is the weaker `catalog-entry`.
    for catalog in &members {
        for (path, line) in run_targets(&catalog.tokens) {
            let target = normalize_member(&path);
            if !target.starts_with(ENCOUNTER_PREFIX) {
                continue;
            }
            let Some(&defining) = by_path.get(&target) else {
                database
                    .unresolved_registrations
                    .push((symbol_name_from_path(&target), path.clone()));
                continue;
            };
            let (fields, _) = record_fields(&members[defining].tokens);
            // Named from the archive's own entry, not from the lowercased lookup key: the corpus
            // ships `encounter27B` and `encounter28B`, and folding their case would rename them.
            let actual = normalize_case_only(&members[defining].name);
            insert(
                &mut database,
                Symbol {
                    name: symbol_name_from_path(&actual),
                    kind: SymbolKind::Encounter,
                    evidence: EvidenceClass::CatalogEntry,
                    member: members[defining].name.clone(),
                    line: 1,
                    offset: 0,
                    registered_in: catalog.name.clone(),
                    fields,
                },
                line,
            );
        }
    }

    // 4. Factions: recovered from the corpus's own `/faith` field values rather than from a list
    //    written here, so the set is measured. They are engine constants: nothing defines them.
    let mut faiths: BTreeMap<String, usize> = BTreeMap::new();
    for symbol in database.symbols.values() {
        if let Some(value) = symbol.fields.get("faith")
            && value.shape == ValueShape::Name
        {
            *faiths.entry(value.text.clone()).or_default() += 1;
        }
    }
    let faith_names: Vec<(String, usize)> = faiths.into_iter().collect();
    for (faith, used_by) in faith_names {
        let mut fields = BTreeMap::new();
        fields.insert(
            "symbols_declaring_this_faith".to_owned(),
            FieldValue {
                shape: ValueShape::Number,
                text: used_by.to_string(),
                number: Some(used_by as f64),
                line: 0,
            },
        );
        insert(
            &mut database,
            Symbol {
                name: faith.clone(),
                kind: SymbolKind::Faction,
                evidence: EvidenceClass::EngineConstant,
                member: String::new(),
                line: 0,
                offset: 0,
                registered_in: String::new(),
                fields,
            },
            0,
        );
    }

    // 5. Buildings: positional arguments at a `define_building_levels` call site. There is no
    //    record file and no registrar, so this is the weakest class the database carries. Every
    //    member is scanned rather than one named one, for the reason `ENCOUNTER_PREFIX` records.
    for member in &members {
        for (name, fields, line, offset) in building_call_sites(&member.tokens) {
            insert(
                &mut database,
                Symbol {
                    name,
                    kind: SymbolKind::Building,
                    evidence: EvidenceClass::CallSiteTuple,
                    member: member.name.clone(),
                    line,
                    offset,
                    registered_in: member.name.clone(),
                    fields,
                },
                line,
            );
        }
    }

    // References: every mention of a known symbol name anywhere in the corpus.
    let wanted: BTreeSet<String> = database
        .symbols
        .values()
        .map(|symbol| symbol.name.clone())
        .collect();
    for member in &members {
        database
            .references
            .extend(references_in(&member.name, &member.tokens, &wanted));
    }
    database.references.sort();

    Ok((database, fingerprints))
}

fn insert(database: &mut SymbolDatabase, mut symbol: Symbol, line: usize) {
    if symbol.line == 0 && line != 0 {
        symbol.line = line;
    }
    // The passes run strongest evidence first, so first-write-wins is the intended precedence.
    // A collision is still a fact about the corpus -- vanilla ships `units\\licr3old.gs` next to
    // `units\\licr3.gs`, both binding `licr3` -- so the loser is recorded, not discarded.
    let key = qualified(symbol.kind, &symbol.name);
    if let Some(existing) = database.symbols.get(&key) {
        if existing.member != symbol.member {
            database
                .name_collisions
                .push((key, existing.member.clone(), symbol.member.clone()));
        }
        return;
    }
    database.symbols.insert(key, symbol);
}

/// Building types declared by `BTYPE first last <procs...> "CODE" define_building_levels`.
///
/// Only the parts that survive a positional read are taken: the type name, the two level bounds,
/// and the four-character code immediately before the operator. The three procedure operands
/// between them are *not* all braced -- one is `buildingdict /name get` -- so a strict positional
/// walk would misalign, and reporting fields it cannot align is how this record would go wrong.
fn building_call_sites(
    tokens: &[Token],
) -> Vec<(String, BTreeMap<String, FieldValue>, usize, usize)> {
    let mut found = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let TokenKind::ExecutableName(operator) = &token.kind else {
            continue;
        };
        if operator != BUILDING_OPERATOR {
            continue;
        }
        let code = match index.checked_sub(1).map(|at| &tokens[at].kind) {
            Some(TokenKind::StringLiteral(text)) => text.clone(),
            _ => continue,
        };
        // Walk back for the nearest `NAME number number` triple, which opens the call.
        let mut opener = None;
        for at in (0..index.saturating_sub(1)).rev() {
            let TokenKind::ExecutableName(name) = &tokens[at].kind else {
                continue;
            };
            let (Some(TokenKind::Number(first)), Some(TokenKind::Number(last))) = (
                tokens.get(at + 1).map(|token| &token.kind),
                tokens.get(at + 2).map(|token| &token.kind),
            ) else {
                continue;
            };
            opener = Some((
                name.clone(),
                first.clone(),
                last.clone(),
                tokens[at].line,
                tokens[at].offset,
            ));
            break;
        }
        let Some((name, first, last, line, offset)) = opener else {
            continue;
        };
        let mut fields = BTreeMap::new();
        fields.insert("code".to_owned(), scalar_text(&code, line));
        fields.insert("first_level".to_owned(), scalar_number(&first, line));
        fields.insert("last_level".to_owned(), scalar_number(&last, line));
        found.push((name, fields, line, offset));
    }
    found
}

fn scalar_text(text: &str, line: usize) -> FieldValue {
    FieldValue {
        shape: ValueShape::Text,
        text: text.to_owned(),
        number: None,
        line,
    }
}

fn scalar_number(text: &str, line: usize) -> FieldValue {
    FieldValue {
        shape: ValueShape::Number,
        text: text.to_owned(),
        number: text.parse().ok(),
        line,
    }
}

fn normalize_member(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

/// Separator-normalised but case-preserving, for names a reader will see.
fn normalize_case_only(name: &str) -> String {
    name.replace('\\', "/")
}

/// An encounter's name: its path below the dungeon catalog's directory, without the extension.
///
/// Not the basename. `gs/dungeons/earth/encounter11.gs` and `gs/dungeons/hidden/encounter11.gs`
/// are different encounters, and naming both `encounter11` merged them -- 34 of vanilla's 247
/// vanished that way before the collision report made it visible. An encounter has no
/// script-level bound name the way a unit does, so its path *is* its identity.
fn symbol_name_from_path(path: &str) -> String {
    path.strip_prefix(ENCOUNTER_PREFIX)
        .unwrap_or(path)
        .trim_end_matches(".gs")
        .to_owned()
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn report(label: &str, database: &SymbolDatabase) {
    println!("profile\t{label}");
    println!("parsed-members\t{}", database.parsed_members);
    // A "no symbol of this kind" answer is only as strong as what the scan could read. State the
    // unreachable count next to the totals rather than leaving a reader to assume it was zero.
    println!(
        "members-the-scan-could-not-read\t{}",
        database.unreadable_members.len()
    );
    println!(
        "registrations-naming-a-member-not-in-this-archive\t{}",
        database.unresolved_registrations.len()
    );
    // The `.gs` filter's cost, from the independent raw byte search.
    println!(
        "entries-excluded-by-the-gs-filter\t{}",
        database.excluded_members
    );
    for (kind, _) in RECORD_MARKERS {
        println!(
            "excluded-entries-containing-a-{kind}-marker\t{}",
            database
                .excluded_members_with_a_record_marker
                .get(kind)
                .copied()
                .unwrap_or(0)
        );
    }
    println!("symbol-name-collisions\t{}", database.name_collisions.len());
    for (name, kept, dropped) in &database.name_collisions {
        println!("collision\t{name}\t{kept}\t{dropped}");
    }
    println!("symbols\t{}", database.symbols.len());
    for kind in SymbolKind::ALL {
        let count = database
            .symbols
            .values()
            .filter(|symbol| symbol.kind == kind)
            .count();
        println!("kind\t{}\t{count}", kind.label());
    }
    for evidence in [
        EvidenceClass::RegisteredByOperator,
        EvidenceClass::DelimitedBlock,
        EvidenceClass::CatalogEntry,
        EvidenceClass::EngineConstant,
        EvidenceClass::CallSiteTuple,
    ] {
        let count = database
            .symbols
            .values()
            .filter(|symbol| symbol.evidence == evidence)
            .count();
        println!("evidence\t{}\t{count}", evidence.label());
    }
    println!("references\t{}", database.references.len());
    let unreferenced = database
        .symbols
        .keys()
        .filter(|name| {
            let symbol = &database.symbols[*name];
            !database
                .references
                .iter()
                .any(|reference| reference.symbol == symbol.name)
        })
        .count();
    println!("symbols-with-no-static-reference\t{unreferenced}");
}

fn write_reports(directory: &Path, built: &[Built]) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;

    let labels: Vec<&str> = built.iter().map(|(label, _, _)| label.as_str()).collect();
    let every_symbol: BTreeSet<&String> = built
        .iter()
        .flat_map(|(_, database, _)| database.symbols.keys())
        .collect();

    // symbols.tsv -- one row per symbol per profile it appears in.
    let mut text = String::from(
        "name\tkind\tevidence\tprofiles\tmember\tline\tbyte-offset\tregistered-in\tdisplay-name\tfields\treferences\tanchor\n",
    );
    for name in &every_symbol {
        let present: Vec<&str> = built
            .iter()
            .filter(|(_, database, _)| database.symbols.contains_key(*name))
            .map(|(label, _, _)| label.as_str())
            .collect();
        let (_, database, _) = built
            .iter()
            .find(|(_, database, _)| database.symbols.contains_key(*name))
            .expect("every symbol came from some profile");
        let symbol = &database.symbols[*name];
        let references = database
            .references
            .iter()
            .filter(|reference| reference.symbol == symbol.name)
            .count();
        writeln!(
            text,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{references}\t{}",
            symbol.name,
            symbol.kind.label(),
            symbol.evidence.label(),
            present.join(","),
            symbol.member,
            symbol.line,
            symbol.offset,
            if symbol.registered_in.is_empty() {
                "-"
            } else {
                &symbol.registered_in
            },
            symbol.display_name().unwrap_or("-"),
            symbol.fields.len(),
            anchor_for(symbol.kind, &symbol.name),
        )
        .expect("string write");
    }
    write(directory.join("symbols.tsv"), &text)?;

    // fields.tsv -- one row per symbol per field per profile, so a value change is a line diff.
    let mut text = String::from("profile\tsymbol\tkind\tfield\tshape\tvalue\tline\n");
    for (label, database, _) in built {
        for symbol in database.symbols.values() {
            for (field, value) in &symbol.fields {
                writeln!(
                    text,
                    "{label}\t{}\t{}\t{field}\t{}\t{}\t{}",
                    symbol.name,
                    symbol.kind.label(),
                    value.shape.label(),
                    value.text.replace(['\t', '\n'], " "),
                    value.line,
                )
                .expect("string write");
            }
        }
    }
    write(directory.join("fields.tsv"), &text)?;

    // references.tsv -- every static reference, in the profile it was seen in.
    let mut text = String::from("profile\tsymbol\tkind\treferencing-member\tline\tform\n");
    for (label, database, _) in built {
        // A bare name in a script does not say which kind it means, and for the ten potions it
        // genuinely means two. Every kind carrying that name is listed rather than one guessed at.
        let mut kinds_by_name: BTreeMap<&str, Vec<SymbolKind>> = BTreeMap::new();
        for symbol in database.symbols.values() {
            kinds_by_name
                .entry(symbol.name.as_str())
                .or_default()
                .push(symbol.kind);
        }
        for reference in &database.references {
            let kinds = kinds_by_name
                .get(reference.symbol.as_str())
                .map(|kinds| {
                    kinds
                        .iter()
                        .map(|kind| kind.label())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_else(|| "-".to_owned());
            writeln!(
                text,
                "{label}\t{}\t{kinds}\t{}\t{}\t{}",
                reference.symbol, reference.member, reference.line, reference.form,
            )
            .expect("string write");
        }
    }
    write(directory.join("references.tsv"), &text)?;

    // field-ranges.tsv -- the observed range, mode and shape mix per kind and field.
    let mut text = String::from(
        "profile\tkind\tfield\tpresent\tabsent\tnumeric\tminimum\tmaximum\tmodal-value\tmodal-count\tdistinct\tshapes\n",
    );
    for (label, database, _) in built {
        for statistics in field_statistics(&database.symbols) {
            let shapes = statistics
                .shapes
                .iter()
                .map(|(shape, count)| format!("{}:{count}", shape.label()))
                .collect::<Vec<_>>()
                .join(" ");
            writeln!(
                text,
                "{label}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{shapes}",
                statistics.kind.label(),
                statistics.field,
                statistics.present,
                statistics.absent,
                statistics.numeric,
                optional(statistics.minimum),
                optional(statistics.maximum),
                optional(statistics.modal_value),
                statistics.modal_count,
                statistics.distinct_numeric,
            )
            .expect("string write");
        }
    }
    write(directory.join("field-ranges.tsv"), &text)?;

    // profile-diff.tsv -- presence and, where present in both, what class of difference.
    let mut text = String::from("symbol\tkind\t");
    for label in &labels {
        write!(text, "in-{label}\t").expect("string write");
    }
    let mut pairs = Vec::new();
    for left in 0..built.len() {
        for right in (left + 1)..built.len() {
            pairs.push((left, right));
            write!(text, "{}-vs-{}\t", labels[right], labels[left]).expect("string write");
        }
    }
    text.push_str("changed-fields\n");
    for name in &every_symbol {
        let (_, database, _) = built
            .iter()
            .find(|(_, database, _)| database.symbols.contains_key(*name))
            .expect("origin");
        let symbol = &database.symbols[*name];
        write!(text, "{name}\t{}\t", symbol.kind.label()).expect("string write");
        for (_, database, _) in built {
            write!(
                text,
                "{}\t",
                if database.symbols.contains_key(*name) {
                    "yes"
                } else {
                    "no"
                }
            )
            .expect("string write");
        }
        let mut changed: BTreeSet<String> = BTreeSet::new();
        for (left, right) in &pairs {
            let fingerprint = |index: usize| -> Option<(u64, u64)> {
                let (_, database, fingerprints) = &built[index];
                let symbol = database.symbols.get(*name)?;
                if symbol.member.is_empty() {
                    // A symbol with no defining member -- every faction -- has nothing to
                    // fingerprint. Say so with a sentinel rather than reporting a false identity.
                    return Some((0, 0));
                }
                fingerprints.get(&normalize_member(&symbol.member)).copied()
            };
            let difference = compare_definitions(fingerprint(*left), fingerprint(*right));
            write!(text, "{}\t", difference.label()).expect("string write");
            if difference == DefinitionDiff::TokenLevel {
                changed.extend(changed_fields(
                    built[*left].1.symbols.get(*name),
                    built[*right].1.symbols.get(*name),
                ));
            }
        }
        writeln!(
            text,
            "{}",
            if changed.is_empty() {
                "-".to_owned()
            } else {
                changed.into_iter().collect::<Vec<_>>().join(",")
            }
        )
        .expect("string write");
    }
    write(directory.join("profile-diff.tsv"), &text)?;

    write_renames(directory, built)?;
    write_reference(directory, built, &every_symbol)?;
    Ok(())
}

/// Symbols present in one profile and absent from another whose *defining member* is nonetheless
/// token-for-token identical to a symbol on the other side.
///
/// Without this, the three-profile comparison is badly misleading. GS5R3 reorganised and renamed
/// its spell and artifact sets wholesale -- vanilla's `lightning_spll` in `gs\spells\lightning.gs`
/// is GS5R3's `bolt_air` in `gs\spells\AIR\bolt_air.gs` -- so a diff keyed on name alone reports
/// a rename as one removal plus one addition. Matching on the token fingerprint of the defining
/// member finds the pairs a name comparison cannot, and it is evidence of a *rename* rather than
/// proof of one: two spells could always have been written identically.
fn write_renames(directory: &Path, built: &[Built]) -> Result<(), String> {
    let mut text = String::from(
        "left-profile\tleft-symbol\tright-profile\tright-symbol\tkind\tleft-member\tright-member\n",
    );
    let mut pairs = 0_usize;
    for left in 0..built.len() {
        for right in (left + 1)..built.len() {
            let (left_label, left_database, left_prints) = &built[left];
            let (right_label, right_database, right_prints) = &built[right];
            // Only symbols the other profile does not have under the same key are candidates.
            let fingerprint = |database: &SymbolDatabase,
                               prints: &BTreeMap<String, (u64, u64)>,
                               symbol: &Symbol|
             -> Option<u64> {
                let _ = database;
                if symbol.member.is_empty() {
                    return None;
                }
                prints
                    .get(&normalize_member(&symbol.member))
                    .map(|(_, tokens)| *tokens)
            };
            let mut right_by_print: BTreeMap<u64, Vec<&Symbol>> = BTreeMap::new();
            for (key, symbol) in &right_database.symbols {
                if left_database.symbols.contains_key(key) {
                    continue;
                }
                if let Some(print) = fingerprint(right_database, right_prints, symbol) {
                    right_by_print.entry(print).or_default().push(symbol);
                }
            }
            for (key, symbol) in &left_database.symbols {
                if right_database.symbols.contains_key(key) {
                    continue;
                }
                let Some(print) = fingerprint(left_database, left_prints, symbol) else {
                    continue;
                };
                let Some(candidates) = right_by_print.get(&print) else {
                    continue;
                };
                for candidate in candidates {
                    if candidate.kind != symbol.kind {
                        continue;
                    }
                    pairs += 1;
                    writeln!(
                        text,
                        "{left_label}\t{}\t{right_label}\t{}\t{}\t{}\t{}",
                        symbol.name,
                        candidate.name,
                        symbol.kind.label(),
                        symbol.member,
                        candidate.member,
                    )
                    .expect("string write");
                }
            }
        }
    }
    println!("rename-candidates-by-identical-token-fingerprint\t{pairs}");
    write(directory.join("renames.tsv"), &text)
}

/// Field keys whose recorded value differs between two readings of the same symbol.
fn changed_fields(left: Option<&Symbol>, right: Option<&Symbol>) -> BTreeSet<String> {
    let (Some(left), Some(right)) = (left, right) else {
        return BTreeSet::new();
    };
    let mut changed = BTreeSet::new();
    let keys: BTreeSet<&String> = left.fields.keys().chain(right.fields.keys()).collect();
    for key in keys {
        let before = left.fields.get(key).map(|value| &value.text);
        let after = right.fields.get(key).map(|value| &value.text);
        if before != after {
            changed.insert(key.clone());
        }
    }
    changed
}

/// The generated Markdown reference: one heading per symbol, anchored, with its fields and where
/// it is referenced from.
fn write_reference(
    directory: &Path,
    built: &[Built],
    every_symbol: &BTreeSet<&String>,
) -> Result<(), String> {
    let mut text = String::from(
        "# Gameplay symbol reference (generated)\n\n\
         Generated by `cargo run --release --example gameplay_symbols`. Do not edit by hand.\n\n\
         Every heading is a stable anchor: a symbol named `bolt_fire` classified as a spell is at\n\
         `#spell-bolt-fire`. The narrative, the annotated examples and the coverage boundary are in\n\
         [gameplay-reference.md](../../docs/gameplay-reference.md); this file is the index.\n\n\
         Aggregate values only. A field whose value is a procedure, dictionary or array is shown as\n\
         its shape and token count, never its body.\n\n",
    );
    for kind in SymbolKind::ALL {
        let names: Vec<&String> = every_symbol
            .iter()
            .copied()
            .filter(|name| {
                built.iter().any(|(_, database, _)| {
                    database
                        .symbols
                        .get(*name)
                        .is_some_and(|symbol| symbol.kind == kind)
                })
            })
            .collect();
        writeln!(text, "## {} ({})\n", kind.label(), names.len()).expect("string write");
        for name in names {
            let (_, database, _) = built
                .iter()
                .find(|(_, database, _)| database.symbols.contains_key(name))
                .expect("origin");
            let symbol = &database.symbols[name];
            let present: Vec<&str> = built
                .iter()
                .filter(|(_, database, _)| database.symbols.contains_key(name))
                .map(|(label, _, _)| label.as_str())
                .collect();
            writeln!(text, "### {} {}\n", kind.label(), symbol.name).expect("string write");
            writeln!(
                text,
                "- display name: {}\n- evidence: `{}`\n- profiles: {}\n- defined in: `{}` line {}\n- references: {}\n",
                symbol.display_name().unwrap_or("(none declared)"),
                symbol.evidence.label(),
                present.join(", "),
                if symbol.member.is_empty() { "(no defining member)" } else { &symbol.member },
                symbol.line,
                database
                    .references
                    .iter()
                    .filter(|reference| reference.symbol == symbol.name)
                    .count(),
            )
            .expect("string write");
            if !symbol.fields.is_empty() {
                text.push_str("| field | shape | value |\n|---|---|---|\n");
                for (field, value) in &symbol.fields {
                    writeln!(
                        text,
                        "| `{field}` | {} | `{}` |",
                        value.shape.label(),
                        value.text.replace('|', "\\|"),
                    )
                    .expect("string write");
                }
                text.push('\n');
            }
        }
    }
    write(directory.join("reference.md"), &text)
}

fn optional(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| format!("{value}"))
}

fn write(path: PathBuf, text: &str) -> Result<(), String> {
    std::fs::write(&path, text)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    println!("wrote\t{}", path.display());
    Ok(())
}

// Referenced so the import list documents the full surface this driver consumes.
#[allow(dead_code)]
fn _unused(reference: &Reference) -> &str {
    reference.form
}
