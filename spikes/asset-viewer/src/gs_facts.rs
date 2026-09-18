//! Machine-readable lexical facts about one `.gs` member, for the build pipeline.
//!
//! The pipeline's validation logic lives in Python, where it can be unit-tested against archives
//! and profiles that do not exist. But there is exactly one GameScript lexer worth trusting, and it
//! is [`crate::gamescript`]: it reports `line`/`column`, and it treats a bare CR as a line ending,
//! which `tools/gs_syntax.py` does not (see the latent-defect list in `docs/roadmap.md`). Rather
//! than grow a second lexer in Python, this module hands the Rust lexer's results across as JSON
//! Lines.
//!
//! Nothing here rewrites GameScript. Every fact is derived from the bytes as they are; the bytes
//! themselves are never normalised, re-encoded or re-emitted. The lexer is for *checking*.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::gamescript::{GameScriptDocument, TokenKind};

/// Line-ending classes, counted as *occurrences* of each terminator.
///
/// **Observed in a local binary, 2026-09-18**, over all 4,692 `.gs` members of the three installed
/// profiles' `gs.mpq`: 3,050 members contain **no line ending at all**, 1,337 are pure CRLF, 193
/// mix CRLF with bare CR, 49 are pure bare CR, 40 mix CRLF with bare LF, and 23 are pure bare LF.
/// So "GameScript uses bare CR" is **Refuted** as a general rule -- it is the exclusive style of 49
/// members out of 4,692 -- and the majority style is "one single line, no terminator". The Phase 4
/// target `units\orinf.gs` is in that majority: 1,798 bytes, zero CR, zero LF.
///
/// This is why the pipeline compares a replacement's census against its *base member's* census
/// instead of asserting any one style. A reflow of `orinf.gs` from 1 line to 35 changes `lf` from 0
/// to 34 and would sail through any CR-versus-LF rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineEndings {
    pub crlf: usize,
    pub bare_cr: usize,
    pub bare_lf: usize,
}

impl LineEndings {
    pub fn measure(source: &[u8]) -> Self {
        let mut endings = Self::default();
        let mut index = 0;
        while index < source.len() {
            match source[index] {
                b'\r' => {
                    if source.get(index + 1) == Some(&b'\n') {
                        endings.crlf += 1;
                        index += 2;
                        continue;
                    }
                    endings.bare_cr += 1;
                }
                b'\n' => endings.bare_lf += 1,
                _ => {}
            }
            index += 1;
        }
        endings
    }

    pub fn total(&self) -> usize {
        self.crlf + self.bare_cr + self.bare_lf
    }
}

/// The byte alphabet a `.gs` member actually uses.
///
/// **Observed in a local binary, 2026-09-18**: across all 4,692 `.gs` members of the three
/// profiles, the only control bytes present are TAB, CR and LF -- there is not one occurrence of
/// any other byte below 0x20, and none of 0x7f. 971 members contain a TAB. Bytes above 0x7e occur
/// in **17 members**, and **not one of those 17 is valid UTF-8**. GameScript is therefore a byte
/// format with a single-byte high half, and a validator that demanded UTF-8 would reject shipped
/// members. The pipeline treats `.gs` as bytes throughout and only *reports* the alphabet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ByteAlphabet {
    /// Bytes below 0x20 other than TAB, CR and LF, with their counts. Empty in the whole corpus.
    pub unexpected_control: BTreeMap<u8, usize>,
    /// Bytes above 0x7e, with their counts.
    pub high: BTreeMap<u8, usize>,
    pub tabs: usize,
    pub valid_utf8: bool,
}

impl ByteAlphabet {
    pub fn measure(source: &[u8]) -> Self {
        let mut alphabet = Self {
            valid_utf8: std::str::from_utf8(source).is_ok(),
            ..Self::default()
        };
        for &byte in source {
            match byte {
                b'\t' => alphabet.tabs += 1,
                b'\r' | b'\n' => {}
                0x00..=0x1f | 0x7f => *alphabet.unexpected_control.entry(byte).or_default() += 1,
                0x80..=0xff => *alphabet.high.entry(byte).or_default() += 1,
                _ => {}
            }
        }
        alphabet
    }
}

/// A string literal worth carrying into validation.
///
/// Every string literal is *counted*, but only path-shaped ones are carried, because
/// `gs\textdict.gs` alone is 385,003 bytes of display text that no path check can use. The count
/// of the ones left behind travels with the record so a caller can report what it did not examine
/// rather than implying it examined everything.
fn looks_like_a_path(value: &str) -> bool {
    if value.contains('\\') || value.contains('/') {
        return true;
    }
    let lowered = value.to_ascii_lowercase();
    [
        ".gs", ".lbm", ".imp", ".mpq", ".til", ".scn", ".smp", ".lgd", ".map", ".wav", ".smk",
        ".pal",
    ]
    .iter()
    .any(|extension| lowered.ends_with(extension))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GsFacts {
    /// The member name as the archive spells it, or the path as the caller gave it.
    pub name: String,
    pub bytes: usize,
    pub sha256: String,
    /// `None` when the file lexed. `Some` carries message, line, column, offset.
    pub parse_error: Option<ParseError>,
    pub token_count: usize,
    pub comment_count: usize,
    pub string_count: usize,
    pub number_count: usize,
    pub maximum_procedure_depth: usize,
    pub procedure_anomalies: Vec<ParseError>,
    pub definition_names: BTreeMap<String, usize>,
    /// Definitions of the exact shape `/name VALUE def`, where VALUE is a single number or string
    /// token, mapped to that value's source text.
    ///
    /// This is deliberately the narrowest possible extraction, not a general evaluator. It covers
    /// the whole of a unit, spell or artifact record -- `units\\orinf.gs` is 34 definitions and 33
    /// of them are this shape -- which is what lets a change report say `hit_points: 13 -> 18`
    /// rather than only that the bytes moved. Anything with a procedure, an array or a computed
    /// value on the right is absent from this map on purpose; `definition_names` still counts it.
    pub scalar_definitions: BTreeMap<String, String>,
    pub executable_names: BTreeMap<String, usize>,
    pub literal_names: BTreeMap<String, usize>,
    pub static_run_dependencies: BTreeSet<String>,
    /// Path-shaped string literals only; see [`looks_like_a_path`].
    pub path_strings: BTreeSet<String>,
    /// String literals that were *not* carried, so the caller can count what it did not check.
    pub non_path_string_count: usize,
    pub line_endings: LineEndings,
    pub alphabet: ByteAlphabet,
    /// A stable token-stream digest, CR-aware, for the change report's
    /// "reformatted versus modified" split.
    pub token_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

impl GsFacts {
    pub fn measure(name: &str, source: &[u8]) -> Self {
        let line_endings = LineEndings::measure(source);
        let alphabet = ByteAlphabet::measure(source);
        let sha256 = sha256_hex(source);

        let document = match GameScriptDocument::parse(source) {
            Ok(document) => document,
            Err(error) => {
                return Self {
                    name: name.to_owned(),
                    bytes: source.len(),
                    sha256,
                    parse_error: Some(ParseError {
                        message: error.message().to_owned(),
                        offset: error.offset(),
                        line: error.line(),
                        column: error.column(),
                    }),
                    token_count: 0,
                    comment_count: 0,
                    string_count: 0,
                    number_count: 0,
                    maximum_procedure_depth: 0,
                    procedure_anomalies: Vec::new(),
                    definition_names: BTreeMap::new(),
                    scalar_definitions: BTreeMap::new(),
                    executable_names: BTreeMap::new(),
                    literal_names: BTreeMap::new(),
                    static_run_dependencies: BTreeSet::new(),
                    path_strings: BTreeSet::new(),
                    non_path_string_count: 0,
                    line_endings,
                    alphabet,
                    token_sha256: String::new(),
                };
            }
        };

        let analysis = document.analyze();
        let mut path_strings = BTreeSet::new();
        let mut non_path_string_count = 0;
        for token in &document.tokens {
            if let TokenKind::StringLiteral(value) = &token.kind {
                if looks_like_a_path(value) {
                    path_strings.insert(value.clone());
                } else {
                    non_path_string_count += 1;
                }
            }
        }

        Self {
            name: name.to_owned(),
            bytes: source.len(),
            sha256,
            parse_error: None,
            token_count: analysis.token_count,
            comment_count: analysis.comment_count,
            string_count: analysis.string_count,
            number_count: analysis.number_count,
            maximum_procedure_depth: analysis.maximum_procedure_depth,
            procedure_anomalies: document
                .procedure_anomalies
                .iter()
                .map(|anomaly| ParseError {
                    message: anomaly.message.clone(),
                    offset: anomaly.offset,
                    line: anomaly.line,
                    column: anomaly.column,
                })
                .collect(),
            scalar_definitions: scalar_definitions(&document, &analysis.definition_names),
            definition_names: analysis.definition_names,
            executable_names: analysis.executable_names,
            literal_names: analysis.literal_names,
            static_run_dependencies: analysis.static_run_dependencies,
            path_strings,
            non_path_string_count,
            token_sha256: token_digest(&document),
            line_endings,
            alphabet,
        }
    }

    pub fn to_json(&self) -> String {
        let mut out = String::from("{");
        write!(out, "\"name\":{}", json_string(&self.name)).unwrap();
        write!(out, ",\"bytes\":{}", self.bytes).unwrap();
        write!(out, ",\"sha256\":{}", json_string(&self.sha256)).unwrap();
        write!(out, ",\"token_sha256\":{}", json_string(&self.token_sha256)).unwrap();
        match &self.parse_error {
            Some(error) => write!(out, ",\"parse_error\":{}", error_json(error)).unwrap(),
            None => out.push_str(",\"parse_error\":null"),
        }
        write!(out, ",\"token_count\":{}", self.token_count).unwrap();
        write!(out, ",\"comment_count\":{}", self.comment_count).unwrap();
        write!(out, ",\"string_count\":{}", self.string_count).unwrap();
        write!(out, ",\"number_count\":{}", self.number_count).unwrap();
        write!(
            out,
            ",\"maximum_procedure_depth\":{}",
            self.maximum_procedure_depth
        )
        .unwrap();
        out.push_str(",\"procedure_anomalies\":[");
        for (index, anomaly) in self.procedure_anomalies.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&error_json(anomaly));
        }
        out.push(']');
        write!(
            out,
            ",\"definition_names\":{}",
            json_counts(&self.definition_names)
        )
        .unwrap();
        write!(
            out,
            ",\"scalar_definitions\":{}",
            json_strings(&self.scalar_definitions)
        )
        .unwrap();
        write!(
            out,
            ",\"executable_names\":{}",
            json_counts(&self.executable_names)
        )
        .unwrap();
        write!(out, ",\"literal_names\":{}", json_counts(&self.literal_names)).unwrap();
        write!(
            out,
            ",\"static_run_dependencies\":{}",
            json_string_array(&self.static_run_dependencies)
        )
        .unwrap();
        write!(
            out,
            ",\"path_strings\":{}",
            json_string_array(&self.path_strings)
        )
        .unwrap();
        write!(
            out,
            ",\"non_path_string_count\":{}",
            self.non_path_string_count
        )
        .unwrap();
        write!(
            out,
            ",\"line_endings\":{{\"crlf\":{},\"bare_cr\":{},\"bare_lf\":{}}}",
            self.line_endings.crlf, self.line_endings.bare_cr, self.line_endings.bare_lf
        )
        .unwrap();
        write!(
            out,
            ",\"alphabet\":{{\"tabs\":{},\"valid_utf8\":{},\"high\":{},\"unexpected_control\":{}}}",
            self.alphabet.tabs,
            self.alphabet.valid_utf8,
            json_byte_counts(&self.alphabet.high),
            json_byte_counts(&self.alphabet.unexpected_control)
        )
        .unwrap();
        out.push('}');
        out
    }
}

/// Pull out every `/name VALUE def` whose VALUE is one number or string token.
///
/// A name is only taken if `definition_names` already agreed it is a definition site, so this
/// inherits that scanner's corrections about `pop`, `undef` and dictionary key positions rather
/// than re-deriving them. A name defined more than once in the same member is dropped from the
/// map entirely: reporting one of two values as *the* value would be a quiet guess.
fn scalar_definitions(
    document: &GameScriptDocument,
    definition_names: &BTreeMap<String, usize>,
) -> BTreeMap<String, String> {
    let mut values: BTreeMap<String, String> = BTreeMap::new();
    let tokens = &document.tokens;
    for (index, token) in tokens.iter().enumerate() {
        let TokenKind::LiteralName(name) = &token.kind else {
            continue;
        };
        if definition_names.get(name).copied().unwrap_or(0) != 1 {
            continue;
        }
        let value = match tokens.get(index + 1).map(|token| &token.kind) {
            Some(TokenKind::Number(text)) => text.clone(),
            Some(TokenKind::StringLiteral(text)) => format!("\"{text}\""),
            _ => continue,
        };
        let followed_by_def = matches!(
            tokens.get(index + 2).map(|token| &token.kind),
            Some(TokenKind::ExecutableName(operator)) if operator.eq_ignore_ascii_case("def")
        );
        if followed_by_def {
            values.insert(name.clone(), value);
        }
    }
    values
}

fn json_strings(values: &BTreeMap<String, String>) -> String {
    let mut out = String::from("{");
    for (index, (name, value)) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write!(out, "{}:{}", json_string(name), json_string(value)).unwrap();
    }
    out.push('}');
    out
}

fn error_json(error: &ParseError) -> String {
    format!(
        "{{\"message\":{},\"offset\":{},\"line\":{},\"column\":{}}}",
        json_string(&error.message),
        error.offset,
        error.line,
        error.column
    )
}

fn token_digest(document: &GameScriptDocument) -> String {
    // NUL-joined token text, matching `gs_syntax.normalized_bytes`'s shape so the two are
    // comparable -- but produced by the CR-aware lexer, so it is correct for the 242 members
    // `gs_syntax.py` mis-lexes. The change report prints both and says when they disagree.
    let mut joined: Vec<u8> = Vec::new();
    for (index, token) in document.tokens.iter().enumerate() {
        if index > 0 {
            joined.push(0);
        }
        let text = match &token.kind {
            TokenKind::ExecutableName(name) => name.clone(),
            TokenKind::LiteralName(name) => format!("/{name}"),
            TokenKind::Number(value) => value.clone(),
            TokenKind::StringLiteral(value) => format!("\"{value}\""),
            TokenKind::Delimiter(delimiter) => delimiter_text(*delimiter).to_owned(),
        };
        joined.extend_from_slice(text.as_bytes());
    }
    sha256_hex(&joined)
}

fn delimiter_text(delimiter: crate::gamescript::Delimiter) -> &'static str {
    use crate::gamescript::Delimiter::*;
    match delimiter {
        ProcedureOpen => "{",
        ProcedureClose => "}",
        ArrayOpen => "[",
        ArrayClose => "]",
        DictionaryOpen => "<<",
        DictionaryClose => ">>",
    }
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                write!(out, "\\u{:04x}", control as u32).unwrap();
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn json_counts(counts: &BTreeMap<String, usize>) -> String {
    let mut out = String::from("{");
    for (index, (name, count)) in counts.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write!(out, "{}:{}", json_string(name), count).unwrap();
    }
    out.push('}');
    out
}

fn json_byte_counts(counts: &BTreeMap<u8, usize>) -> String {
    let mut out = String::from("{");
    for (index, (byte, count)) in counts.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write!(out, "\"{byte}\":{count}").unwrap();
    }
    out.push('}');
    out
}

fn json_string_array(values: &BTreeSet<String>) -> String {
    let mut out = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(value));
    }
    out.push(']');
    out
}

/// A dependency-free SHA-256, so the fact emitter does not add a crate to a tree that has none.
pub fn sha256_hex(message: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut padded = message.to_vec();
    let bit_length = (message.len() as u64).wrapping_mul(8);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0_u32; 64];
        for (index, word) in chunk.chunks_exact(4).enumerate() {
            w[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }

    state.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The digest is checked against a value produced by a *different* implementation
    /// (`shasum -a 256`), not against one this code emitted, so agreement is evidence.
    #[test]
    fn sha256_matches_an_independent_implementation() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Longer than one 64-byte block, which exercises the multi-chunk path the two short
        // vectors above cannot reach.
        assert_eq!(
            sha256_hex(&b"a".repeat(1000)),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }

    #[test]
    fn line_ending_census_separates_the_three_terminators() {
        assert_eq!(
            LineEndings::measure(b"a\r\nb\rc\nd"),
            LineEndings {
                crlf: 1,
                bare_cr: 1,
                bare_lf: 1
            }
        );
        // The Phase 4 target's shape: one line, no terminator at all.
        assert_eq!(LineEndings::measure(b"/a 1 def"), LineEndings::default());
        assert_eq!(LineEndings::measure(b"/a 1 def").total(), 0);
    }

    /// A CRLF must not be counted as both a CR and an LF. Measured in both directions: the pair
    /// counts once as CRLF and zero times as either bare form, and a lone CR immediately followed
    /// by a non-LF byte counts as bare.
    #[test]
    fn crlf_is_not_double_counted() {
        let pair = LineEndings::measure(b"\r\n");
        assert_eq!(pair.crlf, 1);
        assert_eq!(pair.bare_cr, 0);
        assert_eq!(pair.bare_lf, 0);
        let lone = LineEndings::measure(b"\ra");
        assert_eq!(lone.crlf, 0);
        assert_eq!(lone.bare_cr, 1);
        // A CR at the very end of the buffer has no following byte to inspect.
        assert_eq!(LineEndings::measure(b"a\r").bare_cr, 1);
    }

    #[test]
    fn alphabet_reports_high_bytes_without_demanding_utf8() {
        // 0xfc alone is Windows-1252 u-umlaut and is not valid UTF-8. Three shipped members
        // contain exactly this byte.
        let alphabet = ByteAlphabet::measure(b"/a\x09\xfc def");
        assert_eq!(alphabet.tabs, 1);
        assert!(!alphabet.valid_utf8);
        assert_eq!(alphabet.high.get(&0xfc), Some(&1));
        assert!(alphabet.unexpected_control.is_empty());
        // CR and LF are line endings, not unexpected controls; NUL is.
        let controls = ByteAlphabet::measure(b"\r\n\x00\x01");
        assert!(controls.unexpected_control.contains_key(&0));
        assert!(controls.unexpected_control.contains_key(&1));
        assert!(!controls.unexpected_control.contains_key(&b'\r'));
        assert!(!controls.unexpected_control.contains_key(&b'\n'));
    }

    #[test]
    fn facts_survive_a_member_with_no_whitespace_between_a_name_and_its_string() {
        // The exact shape of `units\orinf.gs`: `/name"Footmen"def`, no separating space.
        let facts = GsFacts::measure(
            "units\\orinf.gs",
            b"begin_unit_definition /name\"Footmen\"def /hit_points 13 def",
        );
        assert!(facts.parse_error.is_none());
        assert_eq!(facts.definition_names.get("name"), Some(&1));
        assert_eq!(facts.definition_names.get("hit_points"), Some(&1));
        assert_eq!(
            facts.scalar_definitions.get("hit_points"),
            Some(&"13".to_owned())
        );
        assert_eq!(
            facts.scalar_definitions.get("name"),
            Some(&"\"Footmen\"".to_owned())
        );
        assert_eq!(facts.string_count, 1);
        assert_eq!(facts.non_path_string_count, 1);
        assert!(facts.path_strings.is_empty());
        assert_eq!(facts.line_endings.total(), 0);
    }

    #[test]
    fn a_comment_ends_at_a_bare_cr() {
        // `tools/gs_syntax.py` ends a comment at `\n` only, so it would swallow the definition.
        // This is the reason the pipeline lexes in Rust.
        let facts = GsFacts::measure("t.gs", b"; a comment\r/kept 1 def");
        assert_eq!(facts.comment_count, 1);
        assert_eq!(facts.definition_names.get("kept"), Some(&1));
    }

    #[test]
    fn run_targets_and_path_strings_are_separated_from_display_text() {
        let facts = GsFacts::measure(
            "t.gs",
            b"\"gs\\\\sub\\\\other.gs\" run \"Just some words\" pop \"LBM\\\\ART.lbm\" pop",
        );
        assert!(
            facts
                .static_run_dependencies
                .contains("gs\\\\sub\\\\other.gs")
        );
        assert_eq!(facts.non_path_string_count, 1);
        assert_eq!(facts.path_strings.len(), 2);
    }

    #[test]
    fn a_parse_error_carries_a_line_and_column() {
        let facts = GsFacts::measure("t.gs", b"/ok 1 def\r/ \"unterminated");
        let error = facts.parse_error.expect("a bare slash cannot start a name");
        assert_eq!(error.line, 2);
        assert!(error.column >= 1);
        assert!(facts.token_sha256.is_empty());
        // The byte-level facts are still reported for a file that would not lex.
        assert_eq!(facts.bytes, 25);
        assert_eq!(facts.line_endings.bare_cr, 1);
    }

    #[test]
    fn json_escapes_the_backslashes_that_every_member_name_contains() {
        let facts = GsFacts::measure("gs\\hotkey.gs", b"/a 1 def");
        let json = facts.to_json();
        assert!(json.contains("\"name\":\"gs\\\\hotkey.gs\""));
        assert!(json.contains("\"parse_error\":null"));
        assert!(!json.contains('\n'));
    }

    /// Reformatting changes the bytes and keeps the token digest; changing a number changes both.
    /// Asserted in both directions so a digest that ignored its input would fail.
    #[test]
    fn token_digest_separates_reformatting_from_a_real_change() {
        let original = GsFacts::measure("t.gs", b"/hit_points 13 def");
        let reflowed = GsFacts::measure("t.gs", b"; note\r\n/hit_points\r\n  13\r\n  def\r\n");
        let changed = GsFacts::measure("t.gs", b"/hit_points 18 def");
        assert_ne!(original.sha256, reflowed.sha256);
        assert_eq!(original.token_sha256, reflowed.token_sha256);
        assert_ne!(original.token_sha256, changed.token_sha256);
    }
}
