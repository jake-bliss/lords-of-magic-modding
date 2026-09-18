//! A semantic symbol/index database over the GameScript corpus.
//!
//! The vocabulary work in `gamescript.rs` answers "what names exist and who defines them". This
//! module answers the next question: **which of those names is a unit, a spell, an artifact, an
//! encounter, a faction or a building, and what are its field values**.
//!
//! The classification is deliberately not a path heuristic. Three of the six kinds are registered
//! by a *named engine operator* whose own name states the kind, which is the strongest evidence the
//! corpus can offer short of running the engine:
//!
//! ```text
//! /bolt_fire "gs/spells/FIRE/bolt_fire.gs" define_spell def
//! ```
//!
//! One token triple carries the symbol name, the kind, and the defining member. A directory-name
//! rule would agree with this most of the time and be unfalsifiable where it did not; the registrar
//! rule can be *wrong out loud*, because a file nothing registers produces no symbol.
//!
//! The six kinds are not equally well served, and the record says so rather than the prose alone.
//! See [`EvidenceClass`]: `RegisteredByOperator` and `DelimitedBlock` are structural readings of a
//! construct whose meaning the operator name states, while `EngineConstant` and `CallSiteTuple` are
//! weaker -- they record that the corpus *selects* a thing whose definition lives somewhere this
//! module cannot see.
//!
//! Nothing here reproduces script text. Field values are carried as short normalised token spans so
//! that a range or a default can be computed and cited; the emitted reports are aggregate data.

use std::collections::{BTreeMap, BTreeSet};

use crate::gamescript::{Delimiter, GameScriptDocument, Token, TokenKind};

/// What a symbol is.
///
/// The ordering is the order the reference presents them, which is strongest evidence first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SymbolKind {
    Unit,
    Spell,
    Artifact,
    Encounter,
    Faction,
    Building,
}

impl SymbolKind {
    pub const ALL: [Self; 6] = [
        Self::Unit,
        Self::Spell,
        Self::Artifact,
        Self::Encounter,
        Self::Faction,
        Self::Building,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Spell => "spell",
            Self::Artifact => "artifact",
            Self::Encounter => "encounter",
            Self::Faction => "faction",
            Self::Building => "building",
        }
    }
}

/// How firmly the corpus establishes that a symbol is what this module calls it.
///
/// This is carried on every record rather than stated once in prose, because the six kinds do not
/// share an evidence level and a reader querying one symbol must not have to remember which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvidenceClass {
    /// `/name "member" define_spell def`. The engine operator that consumes the pair names the
    /// kind. Observed in a local archive.
    RegisteredByOperator,
    /// A `begin_unit_definition` ... `end_unit_definition` block whose result is bound by a
    /// following `/name exch def`. The delimiters are engine operators and name the kind.
    /// Observed in a local archive.
    DelimitedBlock,
    /// The member is reached by `"member" run` from inside a catalog array, and the member has the
    /// record shape for its kind. The catalog states membership; the *kind* is Inferred from the
    /// catalog's own identity.
    CatalogEntry,
    /// A name the corpus never defines and uses as a selector -- a faith or a building type.
    /// The symbol is real, but its definition and its field values live outside GameScript.
    /// Inferred.
    EngineConstant,
    /// Field values recovered from positional arguments at a call site rather than from a
    /// `/key value def` record. Inferred: the parameter names come from the callee's own prologue,
    /// and nothing here checks the caller passes them in that order.
    CallSiteTuple,
}

impl EvidenceClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::RegisteredByOperator => "registered-by-operator",
            Self::DelimitedBlock => "delimited-block",
            Self::CatalogEntry => "catalog-entry",
            Self::EngineConstant => "engine-constant",
            Self::CallSiteTuple => "call-site-tuple",
        }
    }
}

/// The shape of a field's value, which decides whether it can carry a range.
///
/// Only [`ValueShape::Number`] is summarised numerically. A field whose value is a procedure has no
/// single value at all -- `/duration{ismyside?{600}{600 vulnerability_factor mul}ifelse}def` is a
/// real spell field -- and reporting a range over such a field would be inventing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueShape {
    Number,
    Text,
    Name,
    Procedure,
    Dictionary,
    Array,
    Expression,
}

impl ValueShape {
    pub fn label(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Text => "text",
            Self::Name => "name",
            Self::Procedure => "procedure",
            Self::Dictionary => "dictionary",
            Self::Array => "array",
            Self::Expression => "expression",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldValue {
    pub shape: ValueShape,
    /// The value's tokens, space-joined. Short by construction: a procedure, dictionary or array
    /// value is recorded as its shape and its token count, never its body.
    pub text: String,
    /// Parsed value when the shape is [`ValueShape::Number`].
    pub number: Option<f64>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub evidence: EvidenceClass,
    /// The member that defines the symbol's fields. Empty for symbols with no defining member,
    /// which is every [`EvidenceClass::EngineConstant`].
    pub member: String,
    /// Line within `member`, or within the registering member for a symbol with no body.
    ///
    /// A line number alone does not locate anything in this corpus. Vanilla's `gs\building.gs`
    /// is a single 14,336-byte line with no terminator at all, and 501 GS5R3 members are the
    /// same shape, so every symbol in such a member is honestly reported at line 1. Use
    /// [`Symbol::offset`] to find it again.
    pub line: usize,
    /// Byte offset of the symbol's defining token within `member`.
    pub offset: usize,
    /// The member that registers the symbol, when that differs from the one defining it.
    pub registered_in: String,
    pub fields: BTreeMap<String, FieldValue>,
}

impl Symbol {
    /// The symbol's human-facing name, when it declares one.
    pub fn display_name(&self) -> Option<&str> {
        self.fields
            .get("name")
            .filter(|value| value.shape == ValueShape::Text)
            .map(|value| value.text.as_str())
    }
}

/// One static reference to a symbol from somewhere in the corpus.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reference {
    pub symbol: String,
    pub member: String,
    pub line: usize,
    /// `executable` for a call/read of the name, `literal` for a `/name` mention.
    pub form: &'static str,
}

/// A symbol's key: its kind and its name.
///
/// Name alone is not a key. GS5R3 registers `potion_health` through **both** `define_artifact` and
/// `define_spell`, from two different members -- a potion really is both -- and vanilla has a
/// `dummy` unit beside a `dummy` encounter. Keying on name alone silently dropped one of each.
pub fn qualified(kind: SymbolKind, name: &str) -> String {
    format!("{}:{}", kind.label(), name)
}

/// Every symbol and reference recovered from one profile.
#[derive(Debug, Clone, Default)]
pub struct SymbolDatabase {
    /// Keyed by [`qualified`], not by bare name.
    pub symbols: BTreeMap<String, Symbol>,
    pub references: Vec<Reference>,
    /// Members that could not be read or tokenized. Counted so that a "no such symbol" answer can
    /// state what the instrument could not reach.
    pub unreadable_members: Vec<String>,
    pub parsed_members: usize,
    /// Members registered by a registrar operator that the archive does not contain.
    pub unresolved_registrations: Vec<(String, String)>,
    /// Archive entries the `.gs` filter excluded, and how many of those a **raw byte scan** --
    /// a mechanism the tokenizer shares nothing with -- finds a record marker in.
    ///
    /// Vanilla's listfile leaves 372 entries unnamed, so they carry no `.gs` suffix and the
    /// tokenizing pass never sees them. Without this count, "vanilla has 154 units" would read as
    /// a measurement when it is a measurement *minus an unknown*. With it, the shortfall is named.
    pub excluded_members: usize,
    pub excluded_members_with_a_record_marker: BTreeMap<String, usize>,
    /// Symbol names claimed by more than one defining member. The first claim is kept; each later
    /// one is recorded here rather than dropped silently.
    ///
    /// These are real: vanilla ships `units\\licr3old.gs` beside `units\\licr3.gs`, and GS5R3
    /// ships a `units\\test.gs` that binds `deldr`. A database reporting one symbol per name
    /// without saying so would hide a superseded definition a modder needs to know about.
    pub name_collisions: Vec<(String, String, String)>,
}

/// Byte sequences whose presence in a member means it probably holds a record of that kind.
///
/// Used only against entries the `.gs` filter excluded, and only as a *raw byte search*. It shares
/// no mechanism with the tokenizer, which is the point: a shortfall the tokenizing pass cannot see
/// is not a shortfall a second tokenizing pass would find either.
pub const RECORD_MARKERS: [(&str, &str); 3] = [
    ("unit", UNIT_BLOCK_OPEN),
    ("spell", "define_spell"),
    ("artifact", "define_artifact"),
];

/// Whether `bytes` contains `needle`, byte for byte, with no decoding at all.
pub fn contains_marker(bytes: &[u8], needle: &str) -> bool {
    let needle = needle.as_bytes();
    bytes.windows(needle.len()).any(|window| window == needle)
}

/// The registrar operators, and the kind each one declares.
///
/// Extending this list is how a new kind is added. A kind with no registrar is not representable by
/// [`registered_symbols`] and has to be recovered some weaker way, which is the situation buildings
/// are in.
pub const REGISTRARS: [(&str, SymbolKind); 2] = [
    ("define_spell", SymbolKind::Spell),
    ("define_artifact", SymbolKind::Artifact),
];

/// The operators that delimit a unit record.
pub const UNIT_BLOCK_OPEN: &str = "begin_unit_definition";
pub const UNIT_BLOCK_CLOSE: &str = "end_unit_definition";

fn name_of(token: &Token) -> Option<(&str, bool)> {
    match &token.kind {
        TokenKind::ExecutableName(name) => Some((name.as_str(), true)),
        TokenKind::LiteralName(name) => Some((name.as_str(), false)),
        _ => None,
    }
}

fn depth_delta(token: &Token) -> isize {
    match &token.kind {
        TokenKind::Delimiter(
            Delimiter::ProcedureOpen | Delimiter::ArrayOpen | Delimiter::DictionaryOpen,
        ) => 1,
        TokenKind::Delimiter(
            Delimiter::ProcedureClose | Delimiter::ArrayClose | Delimiter::DictionaryClose,
        ) => -1,
        _ => 0,
    }
}

/// Symbols registered by `/name "member" REGISTRAR def` anywhere in `tokens`.
///
/// Returns the symbol name, the member path it names, the kind, and the line of the registration.
pub fn registered_symbols(tokens: &[Token]) -> Vec<(String, String, SymbolKind, usize)> {
    let mut found = Vec::new();
    for window in tokens.windows(4) {
        let TokenKind::LiteralName(symbol) = &window[0].kind else {
            continue;
        };
        let TokenKind::StringLiteral(path) = &window[1].kind else {
            continue;
        };
        let TokenKind::ExecutableName(registrar) = &window[2].kind else {
            continue;
        };
        let TokenKind::ExecutableName(terminator) = &window[3].kind else {
            continue;
        };
        if terminator != "def" {
            continue;
        }
        let Some((_, kind)) = REGISTRARS
            .iter()
            .find(|(candidate, _)| candidate == registrar)
        else {
            continue;
        };
        found.push((symbol.clone(), path.clone(), *kind, window[0].line));
    }
    found
}

/// Member paths reached by `"member" run` anywhere in `tokens`.
///
/// This is the catalog idiom: `gs\unittype.gs` and `gs\dungeons.gs` both list their members this
/// way. Unlike [`registered_symbols`] the construct carries no kind, so the caller supplies it.
pub fn run_targets(tokens: &[Token]) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    for window in tokens.windows(2) {
        let TokenKind::StringLiteral(path) = &window[0].kind else {
            continue;
        };
        let TokenKind::ExecutableName(operator) = &window[1].kind else {
            continue;
        };
        if operator == "run" {
            found.push((path.clone(), window[0].line));
        }
    }
    found
}

/// The `/key value def` fields written at `base_depth` within `tokens[range]`.
///
/// A field runs from its literal name to the next `def` at the same depth. That is the shape every
/// record kind in this corpus uses, and it tolerates the multi-token values the corpus really
/// contains -- `/flags UNITTYPELAND CAN_ATTACK or CAN_DEFEND or def` is one field, not four.
///
/// A literal name with no following `def` at its own depth is not a field and is skipped. So is a
/// second write of a key already seen, because GameScript's `def` is last-write-wins only at run
/// time and this module does not execute anything; the first write is recorded and the collision
/// is reported by `duplicate_keys`.
pub fn record_fields(tokens: &[Token]) -> (BTreeMap<String, FieldValue>, usize) {
    let mut fields: BTreeMap<String, FieldValue> = BTreeMap::new();
    let mut duplicate_keys = 0_usize;
    let mut depth = 0_isize;
    let mut index = 0_usize;

    while index < tokens.len() {
        let token = &tokens[index];
        let delta = depth_delta(token);
        if delta != 0 {
            depth += delta;
            index += 1;
            continue;
        }
        let TokenKind::LiteralName(key) = &token.kind else {
            index += 1;
            continue;
        };
        if depth != 0 {
            index += 1;
            continue;
        }

        // Find the `def` that closes this field, at the same depth the key sits at.
        let mut scan = index + 1;
        let mut inner = 0_isize;
        let mut end = None;
        while scan < tokens.len() {
            let delta = depth_delta(&tokens[scan]);
            if delta != 0 {
                inner += delta;
                scan += 1;
                continue;
            }
            if inner == 0 {
                if let TokenKind::ExecutableName(word) = &tokens[scan].kind
                    && word == "def"
                {
                    end = Some(scan);
                    break;
                }
                // Another literal name at this depth before any `def` means the first one was not
                // a field -- it is a dictionary-literal key or a deferred name. Stop rather than
                // swallow the rest of the file into one giant value.
                if matches!(tokens[scan].kind, TokenKind::LiteralName(_)) {
                    break;
                }
            }
            scan += 1;
        }

        let Some(end) = end else {
            index += 1;
            continue;
        };
        let value = &tokens[index + 1..end];
        if value.is_empty() {
            index = end + 1;
            continue;
        }
        let field = classify_value(value, token.line);
        if fields.contains_key(key) {
            duplicate_keys += 1;
        } else {
            fields.insert(key.clone(), field);
        }
        index = end + 1;
    }

    (fields, duplicate_keys)
}

fn classify_value(value: &[Token], line: usize) -> FieldValue {
    let shape = match &value[0].kind {
        TokenKind::Delimiter(Delimiter::ProcedureOpen) => ValueShape::Procedure,
        TokenKind::Delimiter(Delimiter::DictionaryOpen) => ValueShape::Dictionary,
        TokenKind::Delimiter(Delimiter::ArrayOpen) => ValueShape::Array,
        TokenKind::Number(_) if value.len() == 1 => ValueShape::Number,
        TokenKind::StringLiteral(_) if value.len() == 1 => ValueShape::Text,
        TokenKind::ExecutableName(_) | TokenKind::LiteralName(_) if value.len() == 1 => {
            ValueShape::Name
        }
        _ => ValueShape::Expression,
    };

    // Aggregate values are recorded as their shape and size, never their contents. That keeps
    // script text out of the generated reports and keeps a row a row.
    let text = match shape {
        ValueShape::Procedure | ValueShape::Dictionary | ValueShape::Array => {
            format!("<{} {} tokens>", shape.label(), value.len())
        }
        _ => value
            .iter()
            .map(token_text)
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(120)
            .collect(),
    };

    // Finite values only. The shared lexer calls a token a number when `f64::from_str` accepts it,
    // and Rust accepts `inf`, `infinity` and `nan` case-insensitively -- so the corpus's infantry
    // unit code, the literal `INF`, arrives here as floating-point infinity. Eight units per
    // profile carry it, and admitting them reported the `code` field's range as `inf..inf`.
    //
    // This does not fix the lexer, which is outside this module and whose published token counts
    // other work depends on; it stops a non-finite value being summarised as if it were a
    // measurement. Such a field keeps its `number` shape and its text, and is simply not counted.
    let number = match (&shape, &value[0].kind) {
        (ValueShape::Number, TokenKind::Number(text)) => {
            text.parse::<f64>().ok().filter(|value| value.is_finite())
        }
        _ => None,
    };

    FieldValue {
        shape,
        text,
        number,
        line,
    }
}

fn token_text(token: &Token) -> String {
    match &token.kind {
        TokenKind::ExecutableName(name) => name.clone(),
        TokenKind::LiteralName(name) => format!("/{name}"),
        TokenKind::Number(text) => text.clone(),
        TokenKind::StringLiteral(text) => text.clone(),
        TokenKind::Delimiter(delimiter) => match delimiter {
            Delimiter::ProcedureOpen => "{".to_owned(),
            Delimiter::ProcedureClose => "}".to_owned(),
            Delimiter::ArrayOpen => "[".to_owned(),
            Delimiter::ArrayClose => "]".to_owned(),
            Delimiter::DictionaryOpen => "<<".to_owned(),
            Delimiter::DictionaryClose => ">>".to_owned(),
        },
    }
}

/// One unit record: the name the member binds the block's result to, where the block starts, and
/// what it declares.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitRecord {
    pub name: String,
    pub line: usize,
    pub offset: usize,
    pub fields: BTreeMap<String, FieldValue>,
    /// Keys the block writes more than once. See [`record_fields`].
    pub duplicate_keys: usize,
}

/// Every unit record in `tokens`.
///
/// A unit is a `begin_unit_definition` ... `end_unit_definition` block. The block leaves a value on
/// the stack and the member binds it immediately afterwards with `/name exch def`; that binding is
/// the symbol's name, and the unit catalog refers to units by it.
///
/// **A member may hold more than one.** `units\gate.gs` ships three -- `gate`, `rgate` and
/// `lgate` -- so a reader that returned the first block silently lost two units per profile. That
/// is why this returns a vector: the shape is rare, which is exactly what made it easy to miss.
///
pub fn unit_records(tokens: &[Token]) -> Vec<UnitRecord> {
    let mut records = Vec::new();
    let mut cursor = 0_usize;
    while cursor < tokens.len() {
        let Some(open) = tokens[cursor..].iter().position(|token| {
            matches!(&token.kind, TokenKind::ExecutableName(name) if name == UNIT_BLOCK_OPEN)
        }) else {
            break;
        };
        let open = cursor + open;
        let Some(close) = tokens[open..].iter().position(|token| {
            matches!(&token.kind, TokenKind::ExecutableName(name) if name == UNIT_BLOCK_CLOSE)
        }) else {
            break;
        };
        let close = open + close;

        // `/name exch def` after the block closes, but before the next block opens -- otherwise a
        // member whose first block lacked a binding would steal the second block's name.
        let limit = tokens[close + 1..]
            .iter()
            .position(|token| {
                matches!(&token.kind, TokenKind::ExecutableName(name) if name == UNIT_BLOCK_OPEN)
            })
            .map_or(tokens.len(), |offset| close + 1 + offset);
        let bound = tokens[close + 1..limit].windows(3).find_map(|window| {
            let TokenKind::LiteralName(name) = &window[0].kind else {
                return None;
            };
            let TokenKind::ExecutableName(exch) = &window[1].kind else {
                return None;
            };
            let TokenKind::ExecutableName(def) = &window[2].kind else {
                return None;
            };
            (exch == "exch" && def == "def").then(|| name.clone())
        });

        if let Some(bound) = bound {
            let (fields, duplicates) = record_fields(&tokens[open + 1..close]);
            records.push(UnitRecord {
                name: bound,
                line: tokens[open].line,
                offset: tokens[open].offset,
                fields,
                duplicate_keys: duplicates,
            });
        }
        cursor = close + 1;
    }
    records
}

/// Every occurrence of any name in `wanted` within `tokens`, as references from `member`.
pub fn references_in(member: &str, tokens: &[Token], wanted: &BTreeSet<String>) -> Vec<Reference> {
    let mut found = Vec::new();
    for token in tokens {
        let Some((name, executable)) = name_of(token) else {
            continue;
        };
        if !wanted.contains(name) {
            continue;
        }
        found.push(Reference {
            symbol: name.to_owned(),
            member: member.to_owned(),
            line: token.line,
            form: if executable { "executable" } else { "literal" },
        });
    }
    found
}

/// A token-level fingerprint of a member, for the formatting-versus-content distinction.
///
/// Two members with the same fingerprint differ only in whitespace, comments and line endings.
/// This is the same distinction `tools/compare_trees.py` draws, computed from this crate's lexer
/// instead of that tool's own tokenizer -- which matters, because the Python tokenizer terminates a
/// `;` comment at `\n` only, and 25 GS5R3 members use bare CR as their sole line ending.
pub fn token_fingerprint(document: &GameScriptDocument) -> u64 {
    // FNV-1a over the normalised token texts. A hash, not a digest: the only question asked of it
    // is equality between two members of the same corpus.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for token in &document.tokens {
        for byte in token_text(token).as_bytes().iter().chain(b"\0") {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// How a symbol's definition compares between two profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionDiff {
    /// Present in both, byte-identical.
    Identical,
    /// Present in both, same tokens, different bytes: whitespace, comments or line endings only.
    FormattingOnly,
    /// Present in both, different tokens.
    TokenLevel,
    /// Present in one profile only.
    OnlyInLeft,
    OnlyInRight,
}

impl DefinitionDiff {
    pub fn label(self) -> &'static str {
        match self {
            Self::Identical => "identical",
            Self::FormattingOnly => "formatting-only",
            Self::TokenLevel => "token-level",
            Self::OnlyInLeft => "only-in-left",
            Self::OnlyInRight => "only-in-right",
        }
    }
}

/// Compare one symbol's defining member across two profiles.
pub fn compare_definitions(left: Option<(u64, u64)>, right: Option<(u64, u64)>) -> DefinitionDiff {
    match (left, right) {
        (Some((left_bytes, left_tokens)), Some((right_bytes, right_tokens))) => {
            if left_bytes == right_bytes {
                DefinitionDiff::Identical
            } else if left_tokens == right_tokens {
                DefinitionDiff::FormattingOnly
            } else {
                DefinitionDiff::TokenLevel
            }
        }
        (Some(_), None) => DefinitionDiff::OnlyInLeft,
        (None, Some(_)) => DefinitionDiff::OnlyInRight,
        (None, None) => DefinitionDiff::Identical,
    }
}

/// Summary statistics for one numeric field of one kind.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldStatistics {
    pub kind: SymbolKind,
    pub field: String,
    /// Symbols of this kind that write the field at all.
    pub present: usize,
    /// Symbols of this kind that do not.
    pub absent: usize,
    /// Of the present ones, how many wrote a plain number. The rest wrote a procedure or an
    /// expression and have no single value.
    pub numeric: usize,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    /// The most common numeric value, and how many symbols wrote it. This is the observed mode,
    /// which is a *default* only if the engine also treats it as one -- nothing here establishes
    /// that.
    pub modal_value: Option<f64>,
    pub modal_count: usize,
    pub distinct_numeric: usize,
    pub shapes: BTreeMap<ValueShape, usize>,
}

/// Per-field statistics over the symbols of each kind.
pub fn field_statistics(symbols: &BTreeMap<String, Symbol>) -> Vec<FieldStatistics> {
    let mut per_kind: BTreeMap<SymbolKind, Vec<&Symbol>> = BTreeMap::new();
    for symbol in symbols.values() {
        per_kind.entry(symbol.kind).or_default().push(symbol);
    }

    let mut result = Vec::new();
    for (kind, members) in per_kind {
        let mut keys: BTreeSet<&str> = BTreeSet::new();
        for symbol in &members {
            for key in symbol.fields.keys() {
                keys.insert(key.as_str());
            }
        }
        for key in keys {
            let values: Vec<&FieldValue> = members
                .iter()
                .filter_map(|symbol| symbol.fields.get(key))
                .collect();
            let mut shapes: BTreeMap<ValueShape, usize> = BTreeMap::new();
            for value in &values {
                *shapes.entry(value.shape).or_default() += 1;
            }
            let numbers: Vec<f64> = values.iter().filter_map(|value| value.number).collect();
            let mut counts: BTreeMap<String, (f64, usize)> = BTreeMap::new();
            for number in &numbers {
                let entry = counts.entry(format!("{number}")).or_insert((*number, 0));
                entry.1 += 1;
            }
            let modal = counts.values().max_by_key(|(_, count)| *count).copied();
            result.push(FieldStatistics {
                kind,
                field: key.to_owned(),
                present: values.len(),
                absent: members.len() - values.len(),
                numeric: numbers.len(),
                minimum: numbers
                    .iter()
                    .copied()
                    .fold(None, |best: Option<f64>, value| {
                        Some(best.map_or(value, |best| best.min(value)))
                    }),
                maximum: numbers
                    .iter()
                    .copied()
                    .fold(None, |best: Option<f64>, value| {
                        Some(best.map_or(value, |best| best.max(value)))
                    }),
                modal_value: modal.map(|(value, _)| value),
                modal_count: modal.map_or(0, |(_, count)| count),
                distinct_numeric: counts.len(),
                shapes,
            });
        }
    }
    result
}

/// A stable anchor for a symbol in the generated reference.
///
/// Markdown anchors are derived from heading text by lowercasing and replacing runs of
/// non-alphanumerics with a hyphen. Computing it here rather than trusting a renderer keeps the CLI
/// and the document agreeing on one string.
pub fn anchor_for(kind: SymbolKind, name: &str) -> String {
    let mut anchor = String::from(kind.label());
    anchor.push('-');
    let mut previous_hyphen = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            anchor.push(character.to_ascii_lowercase());
            previous_hyphen = false;
        } else if !previous_hyphen {
            anchor.push('-');
            previous_hyphen = true;
        }
    }
    anchor.trim_end_matches('-').to_owned()
}

/// One row of the generated `symbols.tsv`, as the query verbs read it back.
///
/// The committed TSV is the query path's whole input. No archive is opened and no script is read,
/// which is what lets `--gameplay-symbol` answer on a checkout with no game installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRow {
    pub name: String,
    pub kind: String,
    pub evidence: String,
    pub profiles: Vec<String>,
    pub member: String,
    pub line: String,
    pub byte_offset: String,
    pub registered_in: String,
    pub display_name: String,
    pub fields: String,
    pub references: String,
    pub anchor: String,
}

/// The column order `symbols.tsv` is written in. Parsing checks the header against this rather
/// than assuming it, so a regenerated file with reordered columns fails loudly instead of
/// silently reporting one column's values under another column's name.
pub const INDEX_COLUMNS: [&str; 12] = [
    "name",
    "kind",
    "evidence",
    "profiles",
    "member",
    "line",
    "byte-offset",
    "registered-in",
    "display-name",
    "fields",
    "references",
    "anchor",
];

/// Read `symbols.tsv`.
pub fn parse_index(text: &str) -> Result<Vec<IndexRow>, String> {
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split('\t').collect();
    if header != INDEX_COLUMNS {
        return Err(format!(
            "unexpected columns {header:?}; expected {INDEX_COLUMNS:?}. Regenerate with \
             `cargo run --release --example gameplay_symbols`."
        ));
    }
    let mut rows = Vec::new();
    for (number, line) in lines.enumerate() {
        if line.is_empty() {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').collect();
        if cells.len() != INDEX_COLUMNS.len() {
            return Err(format!(
                "row {} has {} columns, expected {}",
                number + 2,
                cells.len(),
                INDEX_COLUMNS.len()
            ));
        }
        rows.push(IndexRow {
            name: cells[0].to_owned(),
            kind: cells[1].to_owned(),
            evidence: cells[2].to_owned(),
            profiles: cells[3].split(',').map(str::to_owned).collect(),
            member: cells[4].to_owned(),
            line: cells[5].to_owned(),
            byte_offset: cells[6].to_owned(),
            registered_in: cells[7].to_owned(),
            display_name: cells[8].to_owned(),
            fields: cells[9].to_owned(),
            references: cells[10].to_owned(),
            anchor: cells[11].to_owned(),
        });
    }
    Ok(rows)
}

/// Case-insensitive glob match supporting `*` and `?`, for `--gameplay-symbols-like`.
pub fn matches_pattern(pattern: &str, candidate: &str) -> bool {
    let pattern: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let candidate: Vec<char> = candidate.to_ascii_lowercase().chars().collect();
    let mut table = vec![vec![false; candidate.len() + 1]; pattern.len() + 1];
    table[0][0] = true;
    for (index, character) in pattern.iter().enumerate() {
        if *character == '*' {
            table[index + 1][0] = table[index][0];
        }
    }
    for (row, character) in pattern.iter().enumerate() {
        for column in 0..candidate.len() {
            table[row + 1][column + 1] = match character {
                '*' => table[row][column + 1] || table[row + 1][column],
                '?' => table[row][column],
                _ => table[row][column] && *character == candidate[column],
            };
        }
    }
    table[pattern.len()][candidate.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokenize a fixture. Fixtures here are written to probe shapes the *corpus does not contain*
    /// as well as ones it does: a suite built only from corpus-shaped input cannot fail on what the
    /// corpus happens never to do.
    fn parse(source: &str) -> Vec<Token> {
        GameScriptDocument::parse(source.as_bytes())
            .expect("fixture tokenizes")
            .tokens
    }

    fn field_text<'a>(fields: &'a BTreeMap<String, FieldValue>, key: &str) -> &'a str {
        fields
            .get(key)
            .unwrap_or_else(|| panic!("expected a field {key}; got {:?}", fields.keys()))
            .text
            .as_str()
    }

    #[test]
    fn a_multi_token_value_is_one_field_not_several() {
        // The corpus's `/flags UNITTYPELAND CAN_ATTACK or ... def`. A reader that took the first
        // token as the value would report `or` and `CAN_DEFEND` as fields of their own.
        let (fields, _) = record_fields(&parse("/flags A B or C or def /armor 6 def"));
        assert_eq!(fields.len(), 2, "got {:?}", fields.keys());
        assert_eq!(fields["flags"].shape, ValueShape::Expression);
        assert_eq!(field_text(&fields, "flags"), "A B or C or");
        assert_eq!(fields["armor"].number, Some(6.0));
    }

    #[test]
    fn keys_inside_a_dictionary_literal_are_not_fields_of_the_record() {
        // `/description_table << /articon_luck "E" ... >> def` is ONE field. Counting the inner
        // keys would inflate every artifact's field list with presentation slots.
        let (fields, _) = record_fields(&parse(
            r#"/description_table << /articon_luck "E" /articon_barter "" >> def /image 2 def"#,
        ));
        assert_eq!(
            fields.keys().collect::<Vec<_>>(),
            vec!["description_table", "image"],
            "inner dictionary keys leaked into the record"
        );
        assert_eq!(fields["description_table"].shape, ValueShape::Dictionary);
    }

    #[test]
    fn keys_inside_a_procedure_value_are_not_fields_of_the_record() {
        let (fields, _) = record_fields(&parse(
            "/targeting_procedure { /army_id exch def /unit_num exch def } def /mana 2 def",
        ));
        assert_eq!(
            fields.keys().collect::<Vec<_>>(),
            vec!["mana", "targeting_procedure"],
            "a procedure body's locals leaked into the record"
        );
    }

    #[test]
    fn a_deferred_native_name_is_not_a_field() {
        // `/invoke_spell cvx` pushes a name for later execution. It has no `def`, so it is not a
        // field -- and crucially the scan must not swallow the rest of the member looking for one.
        let (fields, _) = record_fields(&parse("/invoke_spell cvx /mana 3 def"));
        assert_eq!(fields.keys().collect::<Vec<_>>(), vec!["mana"]);
    }

    #[test]
    fn a_repeated_key_is_counted_rather_than_silently_overwritten() {
        // The corpus may never write a key twice at top level. That is exactly why it is tested:
        // if a mod does, a reader that silently took the last write would report a value the
        // generator never saw a first write for.
        let (fields, duplicates) = record_fields(&parse("/mana 2 def /mana 9 def"));
        assert_eq!(duplicates, 1);
        assert_eq!(
            fields["mana"].number,
            Some(2.0),
            "the first write should be the recorded one"
        );
    }

    #[test]
    fn a_non_finite_number_token_is_never_summarised_as_a_value() {
        // `INF` is the corpus's infantry unit code. Rust's `f64::from_str` accepts it, so the
        // shared lexer hands it over as a number token -- and a range computed over it reads
        // `inf..inf`. Both spellings the parser accepts are checked, and a real number alongside
        // them, so the guard cannot pass by rejecting everything.
        let (fields, _) = record_fields(&parse("/code INF def /other NaN def /armor 6 def"));
        assert_eq!(fields["code"].number, None, "INF was summarised as a value");
        assert_eq!(
            fields["other"].number, None,
            "NaN was summarised as a value"
        );
        assert_eq!(fields["armor"].number, Some(6.0));
        // The text is still reported, so the record does not lose what the corpus actually says.
        assert_eq!(field_text(&fields, "code"), "INF");

        // And such a field contributes to neither the range nor the mode.
        let mut symbols = BTreeMap::new();
        symbols.insert(
            "a".to_owned(),
            Symbol {
                name: "a".to_owned(),
                kind: SymbolKind::Unit,
                evidence: EvidenceClass::DelimitedBlock,
                member: String::new(),
                line: 1,
                offset: 0,
                registered_in: String::new(),
                fields,
            },
        );
        let statistics = field_statistics(&symbols);
        let code = statistics
            .iter()
            .find(|entry| entry.field == "code")
            .expect("code");
        assert_eq!(code.present, 1);
        assert_eq!(code.numeric, 0);
        assert_eq!(code.minimum, None);
        assert_eq!(code.maximum, None);
    }

    #[test]
    fn value_shapes_are_distinguished() {
        let (fields, _) = record_fields(&parse(
            r#"/a 5 def /b "t" def /c NAME def /d { 1 } def /e << >> def /f [ 1 ] def"#,
        ));
        let shapes: Vec<ValueShape> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|key| fields[*key].shape)
            .collect();
        assert_eq!(
            shapes,
            vec![
                ValueShape::Number,
                ValueShape::Text,
                ValueShape::Name,
                ValueShape::Procedure,
                ValueShape::Dictionary,
                ValueShape::Array,
            ]
        );
        // An aggregate value is summarised, never reproduced: no script text reaches a report.
        assert!(field_text(&fields, "d").starts_with("<procedure"));
    }

    #[test]
    fn a_registration_needs_its_registrar_and_its_def() {
        let found = registered_symbols(&parse(
            r#"/bolt "gs/spells/FIRE/bolt.gs" define_spell def
               /relic "gs/artifact/FIRE/relic.gs" define_artifact def
               /catalog "gs/other.gs" run def
               /halfway "gs/spells/x.gs" define_spell bind"#,
        ));
        let names: Vec<&str> = found.iter().map(|entry| entry.0.as_str()).collect();
        assert_eq!(
            names,
            vec!["bolt", "relic"],
            "`run` is not a registrar and a registration without `def` is not one"
        );
        assert_eq!(found[0].2, SymbolKind::Spell);
        assert_eq!(found[1].2, SymbolKind::Artifact);
        assert_eq!(found[0].1, "gs/spells/FIRE/bolt.gs");
    }

    #[test]
    fn a_unit_record_needs_both_delimiters_and_the_binding() {
        let records = unit_records(&parse(
            "begin_unit_definition /attack 6 def /armor 4 def end_unit_definition /aicav exch def",
        ));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "aicav");
        assert_eq!(records[0].line, 1);
        assert_eq!(records[0].fields["attack"].number, Some(6.0));

        // Without the trailing bind there is no symbol name, so there is no symbol. Reporting one
        // under a made-up name is the failure this guards.
        assert!(
            unit_records(&parse(
                "begin_unit_definition /attack 6 def end_unit_definition"
            ))
            .is_empty()
        );
        assert!(unit_records(&parse("/attack 6 def /x exch def")).is_empty());
    }

    #[test]
    fn every_unit_block_in_a_member_is_recovered_not_just_the_first() {
        // `units\\gate.gs` ships three blocks in one member. Reading the first lost two units per
        // profile, and nothing in a one-block fixture could ever have caught it.
        let records = unit_records(&parse(
            "begin_unit_definition /attack 1 def end_unit_definition /gate exch def \
             begin_unit_definition /attack 2 def end_unit_definition /rgate exch def \
             begin_unit_definition /attack 3 def end_unit_definition /lgate exch def",
        ));
        let names: Vec<&str> = records.iter().map(|record| record.name.as_str()).collect();
        assert_eq!(names, vec!["gate", "rgate", "lgate"]);
        let attacks: Vec<Option<f64>> = records
            .iter()
            .map(|record| record.fields["attack"].number)
            .collect();
        assert_eq!(attacks, vec![Some(1.0), Some(2.0), Some(3.0)]);
    }

    #[test]
    fn an_unbound_block_does_not_steal_the_next_blocks_name() {
        // If the search for `/name exch def` ran past the next block opener, an unbound first
        // block would be reported under the second block's name -- with the first block's fields.
        let records = unit_records(&parse(
            "begin_unit_definition /attack 1 def end_unit_definition \
             begin_unit_definition /attack 2 def end_unit_definition /rgate exch def",
        ));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "rgate");
        assert_eq!(records[0].fields["attack"].number, Some(2.0));
    }

    #[test]
    fn fields_outside_the_unit_block_are_not_part_of_the_unit() {
        // The sound bindings after `end_unit_definition` are real tokens in every unit member.
        let records = unit_records(&parse(
            "/preamble 1 def begin_unit_definition /attack 6 def end_unit_definition \
             /aicav exch def /trailing 9 def",
        ));
        assert_eq!(records[0].fields.keys().collect::<Vec<_>>(), vec!["attack"]);
    }

    #[test]
    fn a_comment_ending_at_a_bare_carriage_return_does_not_swallow_the_code_after_it() {
        // The corpus's sole line ending in 25 GS5R3 members is a bare CR. A tokenizer that ends a
        // `;` comment at LF only reads the whole member as one comment -- `tools/gs_syntax.py`
        // does exactly that, and reduces a 5,347-byte encounter to six tokens.
        let with_comment = parse("/mana 2 def ; a note\r/level 3 def");
        let without = parse("/mana 2 def\r/level 3 def");
        let (fields, _) = record_fields(&with_comment);
        assert_eq!(
            fields.keys().collect::<Vec<_>>(),
            vec!["level", "mana"],
            "the comment swallowed the statement after the bare CR"
        );
        assert_eq!(fields["level"].number, Some(3.0));
        // And the fingerprint must see the two as the same code, since they differ only by a
        // comment. That is the formatting-versus-content distinction the profile diff rests on.
        assert_eq!(
            token_fingerprint(
                &GameScriptDocument::parse("/mana 2 def ; a note\r/level 3 def".as_bytes())
                    .unwrap()
            ),
            token_fingerprint(
                &GameScriptDocument::parse("/mana 2 def\r/level 3 def".as_bytes()).unwrap()
            )
        );
        assert_eq!(without.len(), 6);
    }

    #[test]
    fn the_fingerprint_ignores_layout_and_notices_any_token_change() {
        let base = GameScriptDocument::parse(b"/mana 2 def").unwrap();
        let spaced = GameScriptDocument::parse(b"  /mana\t2\r\n\r\ndef  ").unwrap();
        let commented = GameScriptDocument::parse(b"/mana 2 def ; why\n").unwrap();
        assert_eq!(token_fingerprint(&base), token_fingerprint(&spaced));
        assert_eq!(token_fingerprint(&base), token_fingerprint(&commented));

        // Both directions: a value raised and a value lowered must each be seen.
        for changed in [b"/mana 3 def".as_slice(), b"/mana 1 def".as_slice()] {
            let other = GameScriptDocument::parse(changed).unwrap();
            assert_ne!(
                token_fingerprint(&base),
                token_fingerprint(&other),
                "a changed value was read as formatting"
            );
        }
        // A renamed key, and a reordering, must also register.
        for changed in [b"/manna 2 def".as_slice(), b"2 /mana def".as_slice()] {
            let other = GameScriptDocument::parse(changed).unwrap();
            assert_ne!(token_fingerprint(&base), token_fingerprint(&other));
        }
    }

    #[test]
    fn the_fingerprint_separates_tokens_so_a_regrouping_is_not_invisible() {
        // Without a separator between token texts, `/ab c def` and `/a bc def` hash identically:
        // the concatenation is the same and only the token boundaries differ. Two different
        // programs would then be reported as a formatting-only difference.
        let left = GameScriptDocument::parse(b"/ab c def").unwrap();
        let right = GameScriptDocument::parse(b"/a bc def").unwrap();
        assert_ne!(
            token_fingerprint(&left),
            token_fingerprint(&right),
            "a regrouping of the same characters hashed the same"
        );
    }

    #[test]
    fn a_literal_and_an_executable_name_are_different_tokens_to_the_fingerprint() {
        // `/def` and `def` are not the same program. A fingerprint over bare name text would
        // conflate them and call a real edit formatting-only.
        let literal = GameScriptDocument::parse(b"/mana /2 def").unwrap();
        let executable = GameScriptDocument::parse(b"/mana 2 def").unwrap();
        assert_ne!(token_fingerprint(&literal), token_fingerprint(&executable));
    }

    #[test]
    fn definition_differences_separate_formatting_from_content() {
        use DefinitionDiff::*;
        assert_eq!(compare_definitions(Some((1, 9)), Some((1, 9))), Identical);
        assert_eq!(
            compare_definitions(Some((1, 9)), Some((2, 9))),
            FormattingOnly
        );
        assert_eq!(compare_definitions(Some((1, 9)), Some((2, 8))), TokenLevel);
        assert_eq!(compare_definitions(Some((1, 9)), None), OnlyInLeft);
        assert_eq!(compare_definitions(None, Some((1, 9))), OnlyInRight);
    }

    #[test]
    fn statistics_report_the_range_and_the_mode_over_the_symbols_that_have_the_field() {
        let mut symbols = BTreeMap::new();
        for (name, attack) in [("a", 3.0), ("b", 7.0), ("c", 3.0), ("d", 1.0)] {
            symbols.insert(
                name.to_owned(),
                Symbol {
                    name: name.to_owned(),
                    kind: SymbolKind::Unit,
                    evidence: EvidenceClass::DelimitedBlock,
                    member: String::new(),
                    line: 1,
                    offset: 0,
                    registered_in: String::new(),
                    fields: BTreeMap::from([(
                        "attack".to_owned(),
                        FieldValue {
                            shape: ValueShape::Number,
                            text: attack.to_string(),
                            number: Some(attack),
                            line: 1,
                        },
                    )]),
                },
            );
        }
        // One symbol writes the field as a procedure: present, but with no value to average.
        symbols.insert(
            "e".to_owned(),
            Symbol {
                name: "e".to_owned(),
                kind: SymbolKind::Unit,
                evidence: EvidenceClass::DelimitedBlock,
                member: String::new(),
                line: 1,
                offset: 0,
                registered_in: String::new(),
                fields: BTreeMap::from([(
                    "attack".to_owned(),
                    FieldValue {
                        shape: ValueShape::Procedure,
                        text: "<procedure 5 tokens>".to_owned(),
                        number: None,
                        line: 1,
                    },
                )]),
            },
        );
        // And one writes nothing at all, so `absent` has to be non-zero.
        symbols.insert(
            "f".to_owned(),
            Symbol {
                name: "f".to_owned(),
                kind: SymbolKind::Unit,
                evidence: EvidenceClass::DelimitedBlock,
                member: String::new(),
                line: 1,
                offset: 0,
                registered_in: String::new(),
                fields: BTreeMap::new(),
            },
        );

        let statistics = field_statistics(&symbols);
        let attack = statistics
            .iter()
            .find(|entry| entry.field == "attack")
            .expect("attack");
        assert_eq!(attack.present, 5);
        assert_eq!(attack.absent, 1);
        assert_eq!(attack.numeric, 4);
        assert_eq!(attack.minimum, Some(1.0));
        assert_eq!(attack.maximum, Some(7.0));
        assert_eq!(attack.modal_value, Some(3.0));
        assert_eq!(attack.modal_count, 2);
        assert_eq!(attack.distinct_numeric, 3);
        assert_eq!(attack.shapes[&ValueShape::Procedure], 1);
    }

    #[test]
    fn references_record_the_form_and_skip_names_nobody_asked_for() {
        let wanted: BTreeSet<String> = ["aicav"].iter().map(|name| (*name).to_owned()).collect();
        let found = references_in(
            "gs\\x.gs",
            &parse("aicav MOVE acmov setunittypesound\r/aicav exch def\rdecav pop"),
            &wanted,
        );
        assert_eq!(found.len(), 2, "got {found:?}");
        assert_eq!(found[0].form, "executable");
        assert_eq!(found[0].line, 1);
        assert_eq!(found[1].form, "literal");
        // Line 2 only if the bare CR counted as a line ending.
        assert_eq!(found[1].line, 2);
    }

    #[test]
    fn anchors_are_stable_and_distinguish_kinds() {
        assert_eq!(
            anchor_for(SymbolKind::Spell, "bolt_fire"),
            "spell-bolt-fire"
        );
        assert_eq!(anchor_for(SymbolKind::Unit, "bolt_fire"), "unit-bolt-fire");
        assert_eq!(
            anchor_for(SymbolKind::Spell, "normal_merc?"),
            "spell-normal-merc"
        );
        // Faction names are SCREAMING_CASE in the corpus, and a Markdown anchor is lowercase.
        assert_eq!(anchor_for(SymbolKind::Faction, "FIRE"), "faction-fire");
        assert_eq!(
            anchor_for(SymbolKind::Building, "THIEVES_GUILD"),
            "building-thieves-guild"
        );
        // Names differing only by punctuation collide by design; the generator resolves that by
        // reporting the collision, not by silently emitting two identical anchors.
        assert_eq!(
            anchor_for(SymbolKind::Spell, "a_b"),
            anchor_for(SymbolKind::Spell, "a-b")
        );
    }

    #[test]
    fn an_unbalanced_delimiter_does_not_turn_nested_keys_into_record_fields() {
        // Four shipped members have genuinely unmatched `{`, which the lexer records as an anomaly
        // and still hands over. With the braces unbalanced the depth counter never returns to
        // zero, and the depth guard is the only thing stopping a procedure's locals being filed as
        // fields of the record. A balanced fixture cannot exercise it, because the field scan
        // skips past a well-formed value in one jump.
        let (fields, _) = record_fields(&parse(
            "/proc { /inner_local exch def /another 1 def /mana 99 def",
        ));
        assert!(
            fields.is_empty(),
            "an unclosed procedure leaked its locals into the record: {:?}",
            fields.keys()
        );
    }

    #[test]
    fn a_value_of_several_numbers_is_an_expression_not_a_number() {
        // `value.len() == 1` is what separates a scalar from an expression. A corpus-shaped
        // fixture never tests it, because the corpus writes `/armor 6 def` and not `/armor 5 6
        // def` -- so a reader that dropped the length check would look correct on real input and
        // report the first of several operands as the value.
        let (fields, _) = record_fields(&parse("/pair 5 6 def /single 7 def"));
        assert_eq!(fields["pair"].shape, ValueShape::Expression);
        assert_eq!(fields["pair"].number, None);
        assert_eq!(field_text(&fields, "pair"), "5 6");
        assert_eq!(fields["single"].shape, ValueShape::Number);
        assert_eq!(fields["single"].number, Some(7.0));
    }

    #[test]
    fn the_unit_binding_is_the_exch_one_not_merely_the_first_definition_after_the_block() {
        // Every shipped unit member writes `/name exch def` and then more definitions. If the
        // search accepted any `/x ... def`, a member whose next statement happened to come first
        // would name the unit after it. The fixture puts the decoy first, which the corpus does
        // not -- and that is the point. The decoy is `/decoy bind def` rather than `/decoy 9 def`,
        // because the middle token has to be an executable name for a reader that checked only the
        // `def` to match it at all; with a number there, the wrong reader still gets it right.
        let records = unit_records(&parse(
            "begin_unit_definition /attack 6 def end_unit_definition \
             /decoy bind def /aicav exch def",
        ));
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].name, "aicav",
            "the decoy definition was taken as the unit name"
        );
    }

    #[test]
    fn a_name_field_that_is_not_text_is_not_a_display_name() {
        // Encounters really do write `/name { ... } def` -- `gs\\dungeons\\fire\\ficave.gs`
        // computes its name from the terrain sprite under it. Reporting `<procedure 41 tokens>` as
        // a human-facing name would be worse than reporting none.
        let (fields, _) = record_fields(&parse("/name { T_Cave } def"));
        let symbol = Symbol {
            name: "ficave".to_owned(),
            kind: SymbolKind::Encounter,
            evidence: EvidenceClass::CatalogEntry,
            member: String::new(),
            line: 1,
            offset: 0,
            registered_in: String::new(),
            fields,
        };
        assert_eq!(symbol.display_name(), None);

        // Nor is a bare number or a name token. Only text is a display name, so a guard written
        // as "anything but a procedure" is not equivalent.
        for source in ["/name 5 def", "/name CAV def", "/name [ 1 ] def"] {
            let (fields, _) = record_fields(&parse(source));
            let symbol = Symbol {
                fields,
                ..symbol.clone()
            };
            assert_eq!(
                symbol.display_name(),
                None,
                "{source} was read as a display name"
            );
        }

        // And a text one still is, so the guard cannot pass by rejecting everything.
        let (fields, _) = record_fields(&parse(r#"/name "Windriders" def"#));
        let symbol = Symbol { fields, ..symbol };
        assert_eq!(symbol.display_name(), Some("Windriders"));
    }

    #[test]
    fn the_raw_marker_search_is_an_exact_byte_search() {
        // The cross-check instrument. If it answered `true` for everything it would report every
        // excluded entry as a missed record; if it answered `false` for everything it would report
        // the `.gs` filter as costing nothing. Both directions are asserted.
        assert!(contains_marker(
            b"xx begin_unit_definition yy",
            UNIT_BLOCK_OPEN
        ));
        assert!(!contains_marker(
            b"xx begin_unit_definitio yy",
            UNIT_BLOCK_OPEN
        ));
        assert!(!contains_marker(b"", UNIT_BLOCK_OPEN));
        // No decoding: a marker preceded by bytes that are not valid UTF-8 is still found.
        assert!(contains_marker(b"\xff\xfedefine_spell", "define_spell"));
        // And it is case sensitive, as the corpus's own operator names are.
        assert!(!contains_marker(b"DEFINE_SPELL", "define_spell"));
    }

    #[test]
    fn the_index_is_parsed_against_its_declared_columns() {
        let header = INDEX_COLUMNS.join("\t");
        let good = format!(
            "{header}\nbolt_fire\tspell\tregistered-by-operator\tvanilla,gs5r3\t\
             gs\\spells\\FIRE\\bolt_fire.gs\t1\t0\tgs\\spells.gs\tBolt of Fire\t12\t4\tspell-bolt-fire\n"
        );
        let rows = parse_index(&good).expect("parses");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "bolt_fire");
        assert_eq!(rows[0].profiles, vec!["vanilla", "gs5r3"]);
        assert_eq!(rows[0].display_name, "Bolt of Fire");
        assert_eq!(rows[0].byte_offset, "0");
        assert_eq!(rows[0].anchor, "spell-bolt-fire");

        // A reordered header must be refused, not silently misread. This is the failure a reader
        // would never notice: every value present, every one under the wrong name.
        let swapped: Vec<&str> = {
            let mut columns = INDEX_COLUMNS.to_vec();
            columns.swap(0, 1);
            columns
        };
        assert!(parse_index(&format!("{}\n", swapped.join("\t"))).is_err());
        // And so must a short row.
        assert!(parse_index(&format!("{header}\nonly\ttwo\n")).is_err());
        // A header alone is an empty index, not an error.
        assert_eq!(
            parse_index(&format!("{header}\n")).expect("parses").len(),
            0
        );
    }

    #[test]
    fn the_glob_matches_the_way_a_shell_user_expects() {
        assert!(matches_pattern("bolt*", "bolt_fire"));
        assert!(matches_pattern("*fire", "bolt_fire"));
        assert!(matches_pattern("*_*", "bolt_fire"));
        assert!(matches_pattern("BOLT_FIRE", "bolt_fire"));
        assert!(matches_pattern("bolt_fir?", "bolt_fire"));
        assert!(!matches_pattern("bolt", "bolt_fire"));
        assert!(!matches_pattern("bolt_fir?", "bolt_fir"));
        assert!(!matches_pattern("*water*", "bolt_fire"));
        assert!(matches_pattern("*", "anything"));
        assert!(matches_pattern("", ""));
        assert!(!matches_pattern("", "x"));
    }
}
