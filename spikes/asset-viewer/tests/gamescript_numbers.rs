//! Number lexing, asserted against a real archive.
//!
//! **Why these are `#[ignore]`d.** They read a shipped `gs.mpq`, which is not in Git. They are
//! marked ignored so `cargo test` lists them rather than leaving them silently absent.
//!
//! ```text
//! LOM_GS_MPQ='/path/to/English/gs.mpq' cargo test --test gamescript_numbers -- --ignored
//! ```
//!
//! **What these assert.** Not a count copied out of a run. Each test states a property the corpus
//! itself supplies the expectation for: the unit-code alphabet is read out of the corpus's own
//! `/unit_code_strings` array rather than listed here, and the classification test compares the
//! predicate against an independent instrument (`f64::from_str` plus finiteness) over every bare
//! word in the archive. Both carry a guard that fails if the corpus stops exercising the case
//! they exist for, so neither can quietly become vacuous.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use lom_asset_viewer::gamescript::{GameScriptDocument, TokenKind, is_number_token};
use lom_asset_viewer::mpq::Archive;

/// Every bare word in the archive -- the tokens the number predicate decides between -- with the
/// kind the lexer gave it and how many times it occurred.
fn bare_words() -> BTreeMap<String, (BTreeSet<&'static str>, usize)> {
    let path = PathBuf::from(std::env::var("LOM_GS_MPQ").expect(
        "set LOM_GS_MPQ to a local English/gs.mpq; these tests read shipped script that is not in Git",
    ));
    let archive = Archive::open(&path).expect("gs.mpq opens");
    if let Ok(listfile) = std::env::var("LOM_LISTFILE")
        && let Ok(contents) = std::fs::read(listfile)
    {
        let _ = archive.add_listfile_contents(&contents);
    }
    let mut words: BTreeMap<String, (BTreeSet<&'static str>, usize)> = BTreeMap::new();
    for entry in archive.entries().expect("gs.mpq lists") {
        let Ok(content) = archive.read(&entry.name) else {
            continue;
        };
        let Ok(document) = GameScriptDocument::parse(&content) else {
            continue;
        };
        for token in &document.tokens {
            let (kind, text) = match &token.kind {
                TokenKind::ExecutableName(text) => ("executable-name", text),
                TokenKind::Number(text) => ("number", text),
                _ => continue,
            };
            let record = words.entry(text.clone()).or_default();
            record.0.insert(kind);
            record.1 += 1;
        }
    }
    assert!(!words.is_empty(), "the archive yielded no bare words");
    words
}

/// The unit-code alphabet as the corpus declares it: the string array `/unit_code_strings`.
fn unit_codes() -> BTreeSet<String> {
    let path = PathBuf::from(std::env::var("LOM_GS_MPQ").expect("set LOM_GS_MPQ"));
    let archive = Archive::open(&path).expect("gs.mpq opens");
    let mut codes = BTreeSet::new();
    for entry in archive.entries().expect("gs.mpq lists") {
        let Ok(content) = archive.read(&entry.name) else {
            continue;
        };
        let Ok(document) = GameScriptDocument::parse(&content) else {
            continue;
        };
        for (index, token) in document.tokens.iter().enumerate() {
            let TokenKind::LiteralName(name) = &token.kind else {
                continue;
            };
            if name != "unit_code_strings" {
                continue;
            }
            for following in document.tokens.iter().skip(index + 1) {
                match &following.kind {
                    TokenKind::StringLiteral(code) => {
                        codes.insert(code.clone());
                    }
                    TokenKind::Delimiter(_) => continue,
                    _ => break,
                }
            }
        }
    }
    assert!(
        !codes.is_empty(),
        "no /unit_code_strings array found; this test derives its alphabet from the corpus"
    );
    codes
}

#[test]
#[ignore = "reads a shipped gs.mpq"]
fn unit_codes_all_lex_as_names() {
    let codes = unit_codes();
    let words = bare_words();

    // Guard: the test only means something while the corpus still contains a code that
    // `f64::from_str` would swallow. If the alphabet ever stops containing one, say so rather
    // than pass on nothing.
    let swallowed: Vec<&String> = codes
        .iter()
        .filter(|code| code.parse::<f64>().is_ok())
        .collect();
    assert!(
        !swallowed.is_empty(),
        "no unit code is accepted by f64::from_str, so this test no longer exercises anything"
    );

    let mut seen_as_word = 0_usize;
    for code in &codes {
        let Some((kinds, uses)) = words.get(code) else {
            continue;
        };
        seen_as_word += 1;
        assert_eq!(
            kinds.iter().copied().collect::<Vec<_>>(),
            vec!["executable-name"],
            "unit code {code} lexes as {kinds:?} in {uses} uses; every code is a name"
        );
    }
    assert!(
        seen_as_word > 1,
        "only {seen_as_word} unit codes occur as bare words; expected the codes the scripts compare against"
    );
}

#[test]
#[ignore = "reads a shipped gs.mpq"]
fn number_classification_matches_an_independent_reading() {
    let words = bare_words();
    let mut numbers = 0_usize;
    let mut disagreements = Vec::new();
    for (word, (kinds, _)) in &words {
        let independent = word.parse::<f64>().is_ok_and(f64::is_finite);
        let predicate = is_number_token(word);
        if predicate {
            numbers += 1;
            assert!(
                kinds.contains("number"),
                "{word} satisfies the predicate but the lexer called it {kinds:?}"
            );
        }
        if predicate != independent {
            disagreements.push(format!(
                "{word}: predicate={predicate} finite-parse={independent}"
            ));
        }
    }
    assert!(
        disagreements.is_empty(),
        "predicate disagrees with a finite f64 parse on: {disagreements:?}"
    );
    assert!(
        numbers > 100,
        "only {numbers} distinct numbers in the corpus; the survey found over a thousand"
    );
}

#[test]
#[ignore = "reads a shipped gs.mpq"]
fn no_bare_word_lexes_as_an_infinity_or_a_not_a_number() {
    let words = bare_words();
    let mut family = Vec::new();
    for (word, (kinds, uses)) in &words {
        let folded = word.to_ascii_lowercase();
        if !matches!(
            folded.trim_start_matches(['+', '-']),
            "inf" | "infinity" | "nan"
        ) {
            continue;
        }
        family.push(format!("{word} ({uses} uses)"));
        assert!(
            !kinds.contains("number"),
            "{word} lexes as a number in this corpus: {kinds:?}"
        );
    }
    assert!(
        !family.is_empty(),
        "the corpus contains no word in the inf/nan family, so this test proves nothing"
    );
}
