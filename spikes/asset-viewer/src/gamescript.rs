use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delimiter {
    ProcedureOpen,
    ProcedureClose,
    ArrayOpen,
    ArrayClose,
    DictionaryOpen,
    DictionaryClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    ExecutableName(String),
    LiteralName(String),
    Number(String),
    StringLiteral(String),
    Delimiter(Delimiter),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameScriptDocument {
    pub tokens: Vec<Token>,
    pub comment_count: usize,
    pub maximum_procedure_depth: usize,
    pub procedure_anomalies: Vec<ProcedureAnomaly>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureAnomaly {
    pub message: String,
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameScriptAnalysis {
    pub token_count: usize,
    pub comment_count: usize,
    pub string_count: usize,
    pub number_count: usize,
    pub maximum_procedure_depth: usize,
    pub procedure_anomaly_count: usize,
    pub executable_names: BTreeMap<String, usize>,
    pub literal_names: BTreeMap<String, usize>,
    /// Literal names that appear in a *definition* position, as opposed to merely
    /// occurring as a literal somewhere.
    ///
    /// A literal name is not evidence that a script defines it. The corpus defers native
    /// calls by pushing the name and converting it, as in `/invoke_spell cvx`, so treating
    /// every literal as script-defined hides real host calls. Only these shapes count:
    ///
    /// - `/name <value-or-procedure> ... def` within a short window at the same nesting
    ///   depth, which covers `/NAME{...}def`, `/INSANE_LEVEL 3 def`, and `/a exch def`;
    /// - `/name <value>` directly inside a `<< >>` dictionary literal, which is how
    ///   scenario tables such as `gs\scenario\default.gs` declare their entries.
    pub definition_names: BTreeMap<String, usize>,
    pub static_run_dependencies: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameScriptError {
    message: String,
    offset: usize,
    line: usize,
    column: usize,
}

impl GameScriptError {
    fn new(message: impl Into<String>, offset: usize, line: usize, column: usize) -> Self {
        Self {
            message: message.into(),
            offset,
            line,
            column,
        }
    }
}

impl fmt::Display for GameScriptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at byte {}, line {}, column {}",
            self.message, self.offset, self.line, self.column
        )
    }
}

impl std::error::Error for GameScriptError {}

impl GameScriptDocument {
    pub fn parse(source: &[u8]) -> Result<Self, GameScriptError> {
        let mut lexer = Lexer::new(source);
        let mut tokens = Vec::new();
        let mut procedures = Vec::<(usize, usize, usize)>::new();
        let mut maximum_procedure_depth = 0;
        let mut procedure_anomalies = Vec::new();

        while let Some(token) = lexer.next_token()? {
            if let TokenKind::Delimiter(delimiter) = &token.kind {
                match *delimiter {
                    Delimiter::ProcedureOpen => {
                        procedures.push((token.offset, token.line, token.column));
                        maximum_procedure_depth = maximum_procedure_depth.max(procedures.len());
                    }
                    Delimiter::ProcedureClose => {
                        if procedures.pop().is_none() {
                            procedure_anomalies.push(ProcedureAnomaly {
                                message: "unexpected closing procedure delimiter }".to_owned(),
                                offset: token.offset,
                                line: token.line,
                                column: token.column,
                            });
                        }
                    }
                    Delimiter::ArrayOpen
                    | Delimiter::ArrayClose
                    | Delimiter::DictionaryOpen
                    | Delimiter::DictionaryClose => {}
                }
            }
            tokens.push(token);
        }

        for (offset, line, column) in procedures {
            procedure_anomalies.push(ProcedureAnomaly {
                message: "unclosed procedure delimiter {".to_owned(),
                offset,
                line,
                column,
            });
        }

        Ok(Self {
            tokens,
            comment_count: lexer.comment_count,
            maximum_procedure_depth,
            procedure_anomalies,
        })
    }

    pub fn analyze(&self) -> GameScriptAnalysis {
        let mut executable_names = BTreeMap::new();
        let mut literal_names = BTreeMap::new();
        let mut definition_names = BTreeMap::new();
        let mut static_run_dependencies = BTreeSet::new();
        let mut string_count = 0;
        let mut number_count = 0;

        for (index, token) in self.tokens.iter().enumerate() {
            if let TokenKind::LiteralName(name) = &token.kind
                && self.is_definition_site(index)
            {
                *definition_names.entry(name.clone()).or_default() += 1;
            }
        }

        for token in &self.tokens {
            match &token.kind {
                TokenKind::ExecutableName(name) => {
                    *executable_names.entry(name.clone()).or_default() += 1;
                }
                TokenKind::LiteralName(name) => {
                    *literal_names.entry(name.clone()).or_default() += 1;
                }
                TokenKind::Number(_) => number_count += 1,
                TokenKind::StringLiteral(_) => string_count += 1,
                TokenKind::Delimiter(_) => {}
            }
        }
        for pair in self.tokens.windows(2) {
            if let [
                Token {
                    kind: TokenKind::StringLiteral(path),
                    ..
                },
                Token {
                    kind: TokenKind::ExecutableName(operator),
                    ..
                },
            ] = pair
                && operator.eq_ignore_ascii_case("run")
            {
                static_run_dependencies.insert(path.clone());
            }
        }

        GameScriptAnalysis {
            token_count: self.tokens.len(),
            comment_count: self.comment_count,
            string_count,
            number_count,
            maximum_procedure_depth: self.maximum_procedure_depth,
            procedure_anomaly_count: self.procedure_anomalies.len(),
            executable_names,
            literal_names,
            definition_names,
            static_run_dependencies,
        }
    }

    /// How many tokens after a literal name may be *anything at all* and still leave a following
    /// `def` reading as that name's definition. A nested `{...}`, `[...]` or `<<...>>` group
    /// counts as one. Three covers `/NAME{...}def`, `/NAME 3 def` and `/a exch def`.
    const DEFINITION_WINDOW: usize = 3;

    /// The hard cap once only [`DEFINITION_MODIFIERS`] are allowed through.
    ///
    /// Three tokens alone was too few: it missed both of the corpus's procedure-local attachment
    /// forms, so `gs\standard.gs`'s own `writestring`, `pushonstack`, `popoffstack` and `onstack?`
    /// were filed as names nothing in the corpus defines. Executing the module is what exposed
    /// that. This cap is wide enough for the longest observed form,
    /// `/NAME {...} dup 0 N dict put bind def`. Evidence class: Corrected.
    const DEFINITION_ATTACHMENT_WINDOW: usize = 10;

    /// Decide whether the literal name at `index` occupies a definition position.
    fn is_definition_site(&self, index: usize) -> bool {
        // A name the very next operator throws away is not defined by any later `def`.
        // `fonts\balloon.gs` opens with `/CopperplateGothicBT-BoldCond pop /gridsize[16 14]def`,
        // where the font name is pushed as a label and discarded; the scanner used to read the
        // `def` two statements later as that label's. `gs\diplo.gs` has the same shape with
        // `/i undef /majorrace?{4 lt}bind def`. Evidence class: Observed in a local binary.
        if let Some(TokenKind::ExecutableName(next)) =
            self.tokens.get(index + 1).map(|token| &token.kind)
            && NAME_CONSUMING_OPERATORS
                .iter()
                .any(|operator| next.eq_ignore_ascii_case(operator))
        {
            return false;
        }

        if let Some(open) = self.enclosing_dictionary(index) {
            // Inside `<< >>` a literal name is a key, and the following token is its value -- but
            // only at an even offset from the `<<`. `<< /a /value /b 1 >>` used to mark `/value`,
            // the *value* of `/a`, as a definition. Evidence class: Corrected.
            if !self.dictionary_entry_offset(open, index).is_multiple_of(2) {
                return false;
            }
            return matches!(
                self.tokens.get(index + 1).map(|token| &token.kind),
                Some(
                    TokenKind::Number(_)
                        | TokenKind::StringLiteral(_)
                        | TokenKind::ExecutableName(_)
                        | TokenKind::LiteralName(_)
                        | TokenKind::Delimiter(Delimiter::ProcedureOpen | Delimiter::ArrayOpen)
                )
            );
        }

        let mut depth = 0_isize;
        let mut seen = 0_usize;
        for token in self.tokens.iter().skip(index + 1) {
            match &token.kind {
                TokenKind::Delimiter(
                    Delimiter::ProcedureOpen | Delimiter::ArrayOpen | Delimiter::DictionaryOpen,
                ) => depth += 1,
                TokenKind::Delimiter(
                    Delimiter::ProcedureClose | Delimiter::ArrayClose | Delimiter::DictionaryClose,
                ) => {
                    depth -= 1;
                    if depth < 0 {
                        return false;
                    }
                }
                TokenKind::ExecutableName(name) if depth == 0 => {
                    if name.eq_ignore_ascii_case("def") {
                        return true;
                    }
                    if seen >= Self::DEFINITION_WINDOW
                        && !DEFINITION_MODIFIERS
                            .iter()
                            .any(|modifier| name.eq_ignore_ascii_case(modifier))
                    {
                        // Past the short window, only the operators that finish a definition may
                        // stand between the value and its `def`. Any other operator is doing
                        // something else, so the `def` further on belongs to a later statement.
                        return false;
                    }
                }
                _ => {}
            }
            // A nested group counts as one token, matching `/NAME{...}def`.
            if depth == 0 {
                seen += 1;
                if seen > Self::DEFINITION_ATTACHMENT_WINDOW {
                    return false;
                }
            }
        }
        false
    }

    /// The index of the `<<` that *directly* encloses `index`, if a dictionary literal does.
    ///
    /// The old test was "is the `<<` depth above zero", which ignored every other kind of bracket.
    /// `gs\actvrect.gs` ships `/xdict << /left{/x parentrect /x get def} ... >>`, so the literals
    /// inside that procedure were being read as keys of the surrounding dictionary. What matters
    /// is the *innermost* enclosing group, not whether a dictionary is open somewhere outside.
    /// Evidence class: Corrected.
    fn enclosing_dictionary(&self, index: usize) -> Option<usize> {
        let mut open_groups: Vec<(Delimiter, usize)> = Vec::new();
        for (position, token) in self.tokens.iter().take(index).enumerate() {
            if let TokenKind::Delimiter(delimiter) = &token.kind {
                match delimiter {
                    Delimiter::ProcedureOpen | Delimiter::ArrayOpen | Delimiter::DictionaryOpen => {
                        open_groups.push((*delimiter, position));
                    }
                    Delimiter::ProcedureClose
                    | Delimiter::ArrayClose
                    | Delimiter::DictionaryClose => {
                        open_groups.pop();
                    }
                }
            }
        }
        match open_groups.last() {
            Some((Delimiter::DictionaryOpen, position)) => Some(*position),
            _ => None,
        }
    }

    /// How many entries deep into a `<< >>` literal the token at `index` sits, counting a nested
    /// group as one token. Even means a key position, odd means a value position.
    fn dictionary_entry_offset(&self, open: usize, index: usize) -> usize {
        let mut offset = 0_usize;
        let mut depth = 0_isize;
        for token in &self.tokens[open + 1..index] {
            if let TokenKind::Delimiter(delimiter) = &token.kind {
                match delimiter {
                    Delimiter::ProcedureOpen | Delimiter::ArrayOpen | Delimiter::DictionaryOpen => {
                        if depth == 0 {
                            offset += 1;
                        }
                        depth += 1;
                    }
                    Delimiter::ProcedureClose
                    | Delimiter::ArrayClose
                    | Delimiter::DictionaryClose => depth -= 1,
                }
                continue;
            }
            if depth == 0 {
                offset += 1;
            }
        }
        offset
    }
}

/// Operators that may stand between a name's value and its `def` without ending the statement.
///
/// These are exactly the operators the corpus uses to finish a definition: `bind`, and the two
/// forms that attach a procedure's private storage, `dup 0 N dict put` and `/name VALUE replace`.
/// Anything else at the same nesting depth means the `def` further on belongs to a different
/// statement -- which is what keeps `/invoke_spell cvx ... def` out of the definition set.
/// Operators that discard the literal name immediately before them.
///
/// Neither defines anything: `pop` throws the name away and `undef` removes a binding. A literal
/// followed by one of these is therefore never the subject of a later `def`, whatever the tokens
/// in between look like. The list is short and semantic on purpose -- it is not a general fix for
/// statement bleed, which needs operand-arity modelling this scanner does not have.
const NAME_CONSUMING_OPERATORS: &[&str] = &["pop", "undef"];

const DEFINITION_MODIFIERS: &[&str] = &[
    "bind",
    "dup",
    "put",
    "dict",
    "array",
    "string",
    "replace",
    "currentdict",
    "begin",
    "end",
];

struct Lexer<'a> {
    source: &'a [u8],
    offset: usize,
    line: usize,
    column: usize,
    comment_count: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            offset: 0,
            line: 1,
            column: 1,
            comment_count: 0,
        }
    }

    fn next_token(&mut self) -> Result<Option<Token>, GameScriptError> {
        self.skip_layout();
        if self.offset >= self.source.len() {
            return Ok(None);
        }

        let offset = self.offset;
        let line = self.line;
        let column = self.column;
        let byte = self.source[self.offset];
        let kind = match byte {
            b'"' => TokenKind::StringLiteral(self.read_string(offset, line, column)?),
            b'/' => {
                self.advance();
                TokenKind::LiteralName(self.read_name(offset, line, column)?)
            }
            b'{' => {
                self.advance();
                TokenKind::Delimiter(Delimiter::ProcedureOpen)
            }
            b'}' => {
                self.advance();
                TokenKind::Delimiter(Delimiter::ProcedureClose)
            }
            b'[' => {
                self.advance();
                TokenKind::Delimiter(Delimiter::ArrayOpen)
            }
            b']' => {
                self.advance();
                TokenKind::Delimiter(Delimiter::ArrayClose)
            }
            b'<' if self.peek(1) == Some(b'<') => {
                self.advance();
                self.advance();
                TokenKind::Delimiter(Delimiter::DictionaryOpen)
            }
            b'>' if self.peek(1) == Some(b'>') => {
                self.advance();
                self.advance();
                TokenKind::Delimiter(Delimiter::DictionaryClose)
            }
            _ => {
                let name = self.read_name(offset, line, column)?;
                if name.parse::<f64>().is_ok() {
                    TokenKind::Number(name)
                } else {
                    TokenKind::ExecutableName(name)
                }
            }
        };
        Ok(Some(Token {
            kind,
            offset,
            line,
            column,
        }))
    }

    fn skip_layout(&mut self) {
        loop {
            while self
                .source
                .get(self.offset)
                .is_some_and(|byte| byte.is_ascii_whitespace())
            {
                self.advance();
            }
            if self.source.get(self.offset) != Some(&b';') {
                return;
            }
            self.comment_count += 1;
            while self
                .source
                .get(self.offset)
                .is_some_and(|byte| !matches!(byte, b'\r' | b'\n'))
            {
                self.advance();
            }
        }
    }

    fn read_string(
        &mut self,
        start_offset: usize,
        start_line: usize,
        start_column: usize,
    ) -> Result<String, GameScriptError> {
        self.advance();
        let mut bytes = Vec::new();
        while let Some(byte) = self.source.get(self.offset).copied() {
            match byte {
                b'"' => {
                    self.advance();
                    return Ok(String::from_utf8_lossy(&bytes).into_owned());
                }
                _ => {
                    bytes.push(byte);
                    self.advance();
                }
            }
        }
        Err(GameScriptError::new(
            "unterminated string",
            start_offset,
            start_line,
            start_column,
        ))
    }

    fn read_name(
        &mut self,
        start_offset: usize,
        start_line: usize,
        start_column: usize,
    ) -> Result<String, GameScriptError> {
        let name_start = self.offset;
        while self
            .source
            .get(self.offset)
            .is_some_and(|byte| !is_separator(*byte))
        {
            self.advance();
        }
        if self.offset == name_start {
            return Err(GameScriptError::new(
                "empty or unsupported name",
                start_offset,
                start_line,
                start_column,
            ));
        }
        Ok(String::from_utf8_lossy(&self.source[name_start..self.offset]).into_owned())
    }

    fn peek(&self, relative: usize) -> Option<u8> {
        self.source.get(self.offset + relative).copied()
    }

    fn advance(&mut self) {
        let Some(byte) = self.source.get(self.offset).copied() else {
            return;
        };
        self.offset += 1;
        // GameScript members use every line ending: `gs.mpq` in a local GS5R3 install holds 1,123
        // CRLF members, 242 with **bare CR**, and 63 with bare LF. Evidence class: Observed in a
        // local binary. A bare CR is a line ending, so it has to advance the line counter, and a
        // CRLF pair has to advance it once rather than twice -- otherwise every position this
        // lexer reports in a Mac-line-ended member is line 1, which is exactly the sort of report
        // that sends a reader to the wrong statement.
        let ends_line =
            byte == b'\n' || (byte == b'\r' && self.source.get(self.offset) != Some(&b'\n'));
        if ends_line {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
    }
}

fn is_separator(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b';' | b'"' | b'/' | b'{' | b'}' | b'[' | b']' | b'<' | b'>'
        )
}

#[cfg(test)]
mod tests {
    use super::{Delimiter, GameScriptDocument, TokenKind};

    #[test]
    fn tokenizes_comments_strings_names_and_runtime_containers() {
        let source = br#"; leading comment
/loader { "gs/a;file.gs" run [1 /key << /inner true >>] } bind def
"path\to\file" pop
"#;
        let document = GameScriptDocument::parse(source).unwrap();
        let analysis = document.analyze();

        assert_eq!(document.comment_count, 1);
        assert_eq!(document.maximum_procedure_depth, 1);
        assert!(document.procedure_anomalies.is_empty());
        assert_eq!(analysis.static_run_dependencies.len(), 1);
        assert!(analysis.static_run_dependencies.contains("gs/a;file.gs"));
        assert_eq!(analysis.literal_names["loader"], 1);
        assert_eq!(analysis.literal_names["key"], 1);
        assert_eq!(analysis.executable_names["def"], 1);
        assert!(
            document.tokens.iter().any(|token| {
                token.kind == TokenKind::StringLiteral("path\\to\\file".to_owned())
            })
        );
        assert!(
            document
                .tokens
                .iter()
                .any(|token| { token.kind == TokenKind::Delimiter(Delimiter::DictionaryOpen) })
        );
    }

    #[test]
    fn reports_procedure_anomalies_without_rejecting_shipped_fragments() {
        let document = GameScriptDocument::parse(b"} [ << { >> ]").unwrap();
        assert_eq!(document.procedure_anomalies.len(), 2);
        assert_eq!(
            document.procedure_anomalies[0].message,
            "unexpected closing procedure delimiter }"
        );
        assert_eq!(document.procedure_anomalies[0].offset, 0);
        assert_eq!(
            document.procedure_anomalies[1].message,
            "unclosed procedure delimiter {"
        );
        assert_eq!(document.procedure_anomalies[1].offset, 7);
    }

    #[test]
    fn counts_only_definition_shaped_literals_as_definitions() {
        // `/min { ... } def` is a definition; `/invoke_spell cvx` is a deferred native call
        // pushed as a literal, which must NOT be treated as script-defined.
        let source = b"/min{2 copy gt{exch}if pop}def /invoke_spell cvx tickalarm";
        let analysis = GameScriptDocument::parse(source).unwrap().analyze();

        assert_eq!(analysis.literal_names["min"], 1);
        assert_eq!(analysis.literal_names["invoke_spell"], 1);
        assert_eq!(analysis.definition_names["min"], 1);
        assert!(
            !analysis.definition_names.contains_key("invoke_spell"),
            "a literal followed by cvx defers a native call and is not a definition"
        );
    }

    #[test]
    fn counts_dictionary_literal_entries_as_definitions() {
        // Scenario tables declare entries as `/key value` inside `<< >>` with no `def`.
        let source = b"<< /legendary_refresh? 140 /extra_strong?{false}>>";
        let analysis = GameScriptDocument::parse(source).unwrap().analyze();

        assert_eq!(analysis.definition_names["legendary_refresh?"], 1);
        assert_eq!(analysis.definition_names["extra_strong?"], 1);
    }

    /// Both attachment forms end in `def` and both define the name they open with. Missing them
    /// filed four of `gs\standard.gs`'s own procedures as names nothing in the corpus defines.
    #[test]
    fn counts_the_procedure_local_attachment_forms_as_definitions() {
        let source = b"/writestring{1}dup 0 3 dict put bind def /onstack?{2}/dummy 2 dict replace bind def /char_cvs{3}/char_array[0 1 2]replace bind def";
        let analysis = GameScriptDocument::parse(source).unwrap().analyze();

        assert_eq!(analysis.definition_names["writestring"], 1);
        assert_eq!(analysis.definition_names["onstack?"], 1);
        assert_eq!(analysis.definition_names["char_cvs"], 1);
        // The attached local is script-bound data too, not a call into the engine.
        assert_eq!(analysis.definition_names["char_array"], 1);
    }

    /// Three false-positive shapes a cross-model review found, each confirmed against a local
    /// `gs.mpq` before being fixed here. Removing any one of the three guards fails this test.
    #[test]
    fn rejects_the_three_shapes_that_are_not_definitions() {
        // (A) The name is discarded by the very next operator, and the `def` belongs to the next
        // statement. `fonts\balloon.gs` and `gs\diplo.gs` both ship this.
        let analysis =
            GameScriptDocument::parse(b"/CopperplateGothicBT-BoldCond pop /gridsize[16 14]def")
                .unwrap()
                .analyze();
        assert!(
            !analysis
                .definition_names
                .contains_key("CopperplateGothicBT-BoldCond")
        );
        assert_eq!(analysis.definition_names["gridsize"], 1);

        let analysis = GameScriptDocument::parse(b"/i undef /majorrace?{4 lt}bind def")
            .unwrap()
            .analyze();
        assert!(!analysis.definition_names.contains_key("i"));
        assert_eq!(analysis.definition_names["majorrace?"], 1);

        // (B) A value inside a `<< >>` literal is not a key. `/value` is `/a`'s value.
        let analysis = GameScriptDocument::parse(b"<< /a /value /b 1 >>")
            .unwrap()
            .analyze();
        assert_eq!(analysis.definition_names["a"], 1);
        assert_eq!(analysis.definition_names["b"], 1);
        assert!(!analysis.definition_names.contains_key("value"));

        // (C) A literal inside a procedure inside a dictionary is not a key of that dictionary.
        // `/invoke_spell cvx` is the corpus's way of deferring a *native* call.
        let analysis = GameScriptDocument::parse(b"<< /handler { /invoke_spell cvx } >>")
            .unwrap()
            .analyze();
        assert_eq!(analysis.definition_names["handler"], 1);
        assert!(!analysis.definition_names.contains_key("invoke_spell"));

        // ... and the shape (C) came from: a procedure value whose body reads a key off another
        // dictionary, as `gs\actvrect.gs` does. The literals inside the procedure are no longer
        // read as keys of the enclosing dictionary.
        let analysis = GameScriptDocument::parse(b"<< /left{/x parentrect /x get def} >>")
            .unwrap()
            .analyze();
        assert_eq!(analysis.definition_names["left"], 1);
        // **A recorded limitation, not a target.** `x` counts 2: the leading `/x` is genuinely
        // defined, and the second is a `get` operand that the scanner still cannot tell apart,
        // because separating them needs the operand arity of `parentrect` -- a native whose arity
        // this project does not have. That residual is measured at 14 names in 3.02 (0.11% of
        // 12,979). If this ever reads 1, the residual has been closed and this expectation should
        // be tightened rather than deleted.
        assert_eq!(analysis.definition_names["x"], 2);
    }

    #[test]
    fn does_not_treat_a_distant_def_as_a_definition() {
        // `def` beyond the window belongs to a later statement, not to `/first`.
        let source = b"/first pop pop pop pop /second 1 def";
        let analysis = GameScriptDocument::parse(source).unwrap().analyze();

        assert!(!analysis.definition_names.contains_key("first"));
        assert_eq!(analysis.definition_names["second"], 1);
    }

    /// Bare CR is a line ending in this corpus, and treating it as ordinary text is the
    /// project's known way to harvest commented-out code as if it were live: a `;` comment then
    /// appears to run to the end of the member and every following statement disappears.
    ///
    /// 242 members of a local GS5R3 `gs.mpq` use bare CR. Evidence class: Observed in a local
    /// binary. This fixture reproduces the shape rather than shipping one of them.
    #[test]
    fn a_comment_ends_at_a_bare_carriage_return() {
        let source = b"; a Mac-line-ended header comment\r/kept{1}def\r; another comment\r/also_kept{2}def\r";
        let document = GameScriptDocument::parse(source).unwrap();
        let analysis = document.analyze();

        assert_eq!(document.comment_count, 2);
        assert_eq!(analysis.definition_names["kept"], 1);
        assert_eq!(analysis.definition_names["also_kept"], 1);
        assert_eq!(analysis.executable_names["def"], 2);

        // The same bytes with LF line endings must tokenize identically; if they do not, the
        // lexer is treating one of the two as text.
        let with_line_feeds: Vec<u8> = source
            .iter()
            .map(|byte| if *byte == b'\r' { b'\n' } else { *byte })
            .collect();
        let converted = GameScriptDocument::parse(&with_line_feeds).unwrap();
        let kinds: Vec<_> = document.tokens.iter().map(|token| &token.kind).collect();
        let converted_kinds: Vec<_> = converted.tokens.iter().map(|token| &token.kind).collect();
        assert_eq!(kinds, converted_kinds);
    }

    /// A position report is only useful if the line is the line a reader would count.
    #[test]
    fn line_numbers_count_bare_carriage_returns_and_pair_crlf() {
        // Bare CR: the stray `}` is on the fourth line.
        let document = GameScriptDocument::parse(b"/a{1}def\r/b{2}def\r/c{3}def\r}").unwrap();
        assert_eq!(document.procedure_anomalies.len(), 1);
        assert_eq!(document.procedure_anomalies[0].line, 4);

        // CRLF: the same four lines, and the pair must advance the counter once, not twice.
        let document = GameScriptDocument::parse(b"/a{1}def\r\n/b{2}def\r\n/c{3}def\r\n}").unwrap();
        assert_eq!(document.procedure_anomalies[0].line, 4);
    }

    #[test]
    fn rejects_empty_literal_names_and_unterminated_strings() {
        assert_eq!(
            GameScriptDocument::parse(b"/ }").unwrap_err().to_string(),
            "empty or unsupported name at byte 0, line 1, column 1"
        );
        assert_eq!(
            GameScriptDocument::parse(b"\"unfinished")
                .unwrap_err()
                .to_string(),
            "unterminated string at byte 0, line 1, column 1"
        );
    }
}
