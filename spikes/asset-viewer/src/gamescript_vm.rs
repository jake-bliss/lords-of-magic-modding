use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use crate::gamescript::{Delimiter, GameScriptDocument, Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Boolean(bool),
    String(String),
    LiteralName(String),
    ExecutableName(String),
    Procedure(Rc<RefCell<ProcedureValue>>),
    Array(Rc<RefCell<Vec<Value>>>),
    Dictionary(Rc<RefCell<BTreeMap<String, Value>>>),
    Mark(CollectionKind),
}

impl Value {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Number(_) => "number",
            Self::Boolean(_) => "boolean",
            Self::String(_) => "string",
            Self::LiteralName(_) => "literal-name",
            Self::ExecutableName(_) => "executable-name",
            Self::Procedure(_) => "procedure",
            Self::Array(_) => "array",
            Self::Dictionary(_) => "dictionary",
            Self::Mark(CollectionKind::Array) => "array-mark",
            Self::Mark(CollectionKind::Dictionary) => "dictionary-mark",
        }
    }

    pub fn scalar_summary(&self) -> Option<String> {
        match self {
            Self::Number(value) => Some(value.to_string()),
            Self::Boolean(value) => Some(value.to_string()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionKind {
    Array,
    Dictionary,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcedureValue {
    body: Vec<Value>,
    metadata: BTreeMap<usize, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameScriptVmError {
    pub message: String,
    pub step: usize,
    pub call_stack: Vec<String>,
}

impl GameScriptVmError {
    /// The structured trace for an unknown-name failure, for host-call classification.
    ///
    /// Read from the error rather than the VM: the call stack unwinds as the failure
    /// propagates, so only the error still holds the frames at the point of failure.
    pub fn unknown_name(&self) -> Option<UnknownNameTrace> {
        self.message
            .strip_prefix("unknown executable name ")
            .map(|name| UnknownNameTrace {
                name: name.to_owned(),
                steps: self.step,
                call_stack: self.call_stack.clone(),
            })
    }
}

impl fmt::Display for GameScriptVmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at VM step {}", self.message, self.step)?;
        if !self.call_stack.is_empty() {
            write!(formatter, " [{}]", self.call_stack.join(" -> "))?;
        }
        Ok(())
    }
}

impl std::error::Error for GameScriptVmError {}

#[derive(Debug)]
pub struct GameScriptVm {
    operand_stack: Vec<Value>,
    dictionaries: Vec<Rc<RefCell<BTreeMap<String, Value>>>>,
    call_stack: Vec<String>,
    steps: usize,
    maximum_steps: usize,
    native_stubs: BTreeMap<String, Value>,
    native_calls: BTreeMap<String, usize>,
}

/// One observation of an executable name the VM could not resolve.
///
/// Unknown names stop execution rather than being guessed. This record is what makes the
/// failure inspectable: which name, how deep in the call stack, and after how many steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownNameTrace {
    pub name: String,
    pub steps: usize,
    pub call_stack: Vec<String>,
}

impl GameScriptVm {
    pub fn new(maximum_steps: usize) -> Self {
        Self {
            operand_stack: Vec::new(),
            dictionaries: vec![Rc::new(RefCell::new(BTreeMap::new()))],
            call_stack: Vec::new(),
            steps: 0,
            maximum_steps,
            native_stubs: BTreeMap::new(),
            native_calls: BTreeMap::new(),
        }
    }

    /// Supply a value for a native host call the engine would otherwise provide.
    ///
    /// The host API is not implemented and is not being guessed at. A stub stands in for a
    /// *pure read of game state* so that script logic depending on it can be executed and
    /// observed under a known, declared input. Anything with side effects must not be
    /// stubbed this way.
    pub fn define_native_stub(&mut self, name: impl Into<String>, value: Value) {
        self.native_stubs.insert(name.into(), value);
    }

    /// Names satisfied from the stub table, with call counts. This is the evidence for
    /// classifying a candidate as a native host call rather than a script definition.
    pub fn native_calls(&self) -> &BTreeMap<String, usize> {
        &self.native_calls
    }

    pub fn execute_document(
        &mut self,
        document: &GameScriptDocument,
    ) -> Result<(), GameScriptVmError> {
        let values = compile_document(document)?;
        self.execute_values(&values)
    }

    pub fn operand_stack(&self) -> &[Value] {
        &self.operand_stack
    }

    pub fn steps(&self) -> usize {
        self.steps
    }

    pub fn defined_names(&self) -> Vec<String> {
        self.dictionaries
            .first()
            .map(|dictionary| dictionary.borrow().keys().cloned().collect())
            .unwrap_or_default()
    }

    fn execute_values(&mut self, values: &[Value]) -> Result<(), GameScriptVmError> {
        for value in values {
            self.steps = self.steps.saturating_add(1);
            if self.steps > self.maximum_steps {
                return Err(self.error(format!(
                    "execution exceeded the {}-step limit",
                    self.maximum_steps
                )));
            }
            match value {
                Value::ExecutableName(name) => self.execute_name(name)?,
                other => self.operand_stack.push(other.clone()),
            }
        }
        Ok(())
    }

    fn execute_name(&mut self, name: &str) -> Result<(), GameScriptVmError> {
        if name == "true" {
            self.operand_stack.push(Value::Boolean(true));
            return Ok(());
        }
        if name == "false" {
            self.operand_stack.push(Value::Boolean(false));
            return Ok(());
        }
        if let Some(value) = self.lookup(name) {
            return self.execute_resolved(name, value);
        }
        if self.execute_builtin(name)? {
            return Ok(());
        }
        if let Some(value) = self.native_stubs.get(name).cloned() {
            *self.native_calls.entry(name.to_owned()).or_default() += 1;
            self.operand_stack.push(value);
            return Ok(());
        }
        Err(self.error(format!("unknown executable name {name}")))
    }

    fn execute_resolved(&mut self, name: &str, value: Value) -> Result<(), GameScriptVmError> {
        match value {
            Value::Procedure(body) => {
                self.call_stack.push(name.to_owned());
                let body = body.borrow().body.clone();
                let result = self.execute_values(&body);
                self.call_stack.pop();
                result
            }
            Value::ExecutableName(alias) => self.execute_name(&alias),
            other => {
                self.operand_stack.push(other);
                Ok(())
            }
        }
    }

    fn execute_builtin(&mut self, name: &str) -> Result<bool, GameScriptVmError> {
        match name {
            "dup" => {
                let value = self.peek()?.clone();
                self.operand_stack.push(value);
            }
            "pop" => {
                self.pop()?;
            }
            "exch" => {
                self.require_stack(2)?;
                let length = self.operand_stack.len();
                self.operand_stack.swap(length - 1, length - 2);
            }
            "copy" => {
                let count = self.pop_nonnegative_integer("copy count")?;
                self.require_stack(count)?;
                let start = self.operand_stack.len() - count;
                let copies = self.operand_stack[start..].to_vec();
                self.operand_stack.extend(copies);
            }
            "dict" => {
                self.pop_nonnegative_integer("dict capacity")?;
                self.operand_stack
                    .push(Value::Dictionary(Rc::new(RefCell::new(BTreeMap::new()))));
            }
            "array" => {
                let length = self.pop_nonnegative_integer("array length")?;
                self.operand_stack
                    .push(Value::Array(Rc::new(RefCell::new(vec![
                        Value::Number(0.0);
                        length
                    ]))));
            }
            "string" => {
                let length = self.pop_nonnegative_integer("string length")?;
                self.operand_stack.push(Value::String("\0".repeat(length)));
            }
            "[" => self.operand_stack.push(Value::Mark(CollectionKind::Array)),
            "]" => self.close_collection(CollectionKind::Array)?,
            "<<" => self
                .operand_stack
                .push(Value::Mark(CollectionKind::Dictionary)),
            ">>" => self.close_collection(CollectionKind::Dictionary)?,
            "def" => {
                let value = self.pop()?;
                let name = self.pop_literal_name("def")?;
                self.current_dictionary()?.borrow_mut().insert(name, value);
            }
            "bind" => {
                self.peek()?;
            }
            "replace" => self.replace_metadata()?,
            "put" => self.put()?,
            "get" => self.get()?,
            "begin" => match self.pop()? {
                Value::Dictionary(dictionary) => self.dictionaries.push(dictionary),
                value => return Err(self.type_error("begin", "dictionary", &value)),
            },
            "end" => {
                if self.dictionaries.len() <= 1 {
                    return Err(self.error("end cannot pop the base dictionary"));
                }
                self.dictionaries.pop();
            }
            "currentdict" => {
                let dictionary = Rc::clone(self.current_dictionary()?);
                self.operand_stack.push(Value::Dictionary(dictionary));
            }
            "exec" => {
                let value = self.pop()?;
                self.execute_resolved("exec", value)?;
            }
            "add" => self.binary_number("add", |left, right| left + right)?,
            "sub" => self.binary_number("sub", |left, right| left - right)?,
            "mul" => self.binary_number("mul", |left, right| left * right)?,
            "div" => self.binary_number("div", |left, right| left / right)?,
            "idiv" => self.binary_number("idiv", |left, right| (left / right).trunc())?,
            "mod" => self.binary_number("mod", |left, right| left % right)?,
            "neg" => {
                let value = self.pop_number("neg")?;
                self.operand_stack.push(Value::Number(-value));
            }
            "eq" => {
                let right = self.pop()?;
                let left = self.pop()?;
                self.operand_stack.push(Value::Boolean(left == right));
            }
            "gt" => self.compare_numbers("gt", |left, right| left > right)?,
            "lt" => self.compare_numbers("lt", |left, right| left < right)?,
            "not" => {
                let value = self.pop_boolean("not")?;
                self.operand_stack.push(Value::Boolean(!value));
            }
            "if" => {
                let procedure = self.pop_procedure("if")?;
                if self.pop_boolean("if")? {
                    self.execute_values(&procedure)?;
                }
            }
            "ifelse" => {
                let otherwise = self.pop_procedure("ifelse")?;
                let then = self.pop_procedure("ifelse")?;
                let condition = self.pop_boolean("ifelse")?;
                self.execute_values(if condition { &then } else { &otherwise })?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn close_collection(&mut self, kind: CollectionKind) -> Result<(), GameScriptVmError> {
        let Some(mark_index) = self
            .operand_stack
            .iter()
            .rposition(|value| *value == Value::Mark(kind))
        else {
            return Err(self.error(format!("closing {:?} has no matching mark", kind)));
        };
        let values = self.operand_stack.split_off(mark_index + 1);
        self.operand_stack.pop();
        match kind {
            CollectionKind::Array => self
                .operand_stack
                .push(Value::Array(Rc::new(RefCell::new(values)))),
            CollectionKind::Dictionary => {
                if !values.len().is_multiple_of(2) {
                    return Err(self.error("dictionary literal has an odd value count"));
                }
                let mut dictionary = BTreeMap::new();
                for pair in values.chunks_exact(2) {
                    let key = match &pair[0] {
                        Value::LiteralName(name) | Value::String(name) => name.clone(),
                        value => {
                            return Err(self.type_error(
                                "dictionary literal",
                                "literal-name or string key",
                                value,
                            ));
                        }
                    };
                    dictionary.insert(key, pair[1].clone());
                }
                self.operand_stack
                    .push(Value::Dictionary(Rc::new(RefCell::new(dictionary))));
            }
        }
        Ok(())
    }

    fn put(&mut self) -> Result<(), GameScriptVmError> {
        let value = self.pop()?;
        let index = self.pop()?;
        let aggregate = self.pop()?;
        match (aggregate, index) {
            (Value::Array(values), Value::Number(index)) => {
                let index =
                    number_to_index(index, "put index").map_err(|message| self.error(message))?;
                let mut values = values.borrow_mut();
                if index >= values.len() {
                    return Err(self.error(format!(
                        "put index {index} is outside aggregate length {}",
                        values.len()
                    )));
                }
                values[index] = value;
            }
            (Value::Procedure(procedure), Value::Number(index)) => {
                let index =
                    number_to_index(index, "put index").map_err(|message| self.error(message))?;
                procedure.borrow_mut().metadata.insert(index, value);
            }
            (Value::Dictionary(values), Value::LiteralName(key))
            | (Value::Dictionary(values), Value::String(key)) => {
                values.borrow_mut().insert(key, value);
            }
            (aggregate, index) => {
                return Err(self.error(format!(
                    "put does not support {} with {} index",
                    aggregate.kind(),
                    index.kind()
                )));
            }
        }
        Ok(())
    }

    fn replace_metadata(&mut self) -> Result<(), GameScriptVmError> {
        self.peek()?;
        let length = self.operand_stack.len();
        let attaches_to_procedure = length >= 3
            && matches!(self.operand_stack[length - 3], Value::Procedure(_))
            && matches!(self.operand_stack[length - 2], Value::LiteralName(_));
        if attaches_to_procedure {
            self.operand_stack.pop();
            self.operand_stack.pop();
        }
        Ok(())
    }

    fn get(&mut self) -> Result<(), GameScriptVmError> {
        let index = self.pop()?;
        let aggregate = self.pop()?;
        let value = match (aggregate, index) {
            (Value::Array(values), Value::Number(index)) => {
                let index =
                    number_to_index(index, "get index").map_err(|message| self.error(message))?;
                values
                    .borrow()
                    .get(index)
                    .cloned()
                    .ok_or_else(|| self.error(format!("get index {index} is out of bounds")))?
            }
            (Value::Procedure(procedure), Value::Number(index)) => {
                let index =
                    number_to_index(index, "get index").map_err(|message| self.error(message))?;
                procedure
                    .borrow()
                    .metadata
                    .get(&index)
                    .cloned()
                    .ok_or_else(|| self.error(format!("procedure has no metadata slot {index}")))?
            }
            (Value::Dictionary(values), Value::LiteralName(key))
            | (Value::Dictionary(values), Value::String(key)) => values
                .borrow()
                .get(&key)
                .cloned()
                .ok_or_else(|| self.error(format!("dictionary has no key {key}")))?,
            (aggregate, index) => {
                return Err(self.error(format!(
                    "get does not support {} with {} index",
                    aggregate.kind(),
                    index.kind()
                )));
            }
        };
        self.operand_stack.push(value);
        Ok(())
    }

    fn binary_number(
        &mut self,
        operator: &str,
        function: impl FnOnce(f64, f64) -> f64,
    ) -> Result<(), GameScriptVmError> {
        let right = self.pop_number(operator)?;
        let left = self.pop_number(operator)?;
        self.operand_stack
            .push(Value::Number(function(left, right)));
        Ok(())
    }

    fn compare_numbers(
        &mut self,
        operator: &str,
        function: impl FnOnce(f64, f64) -> bool,
    ) -> Result<(), GameScriptVmError> {
        let right = self.pop_number(operator)?;
        let left = self.pop_number(operator)?;
        self.operand_stack
            .push(Value::Boolean(function(left, right)));
        Ok(())
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        self.dictionaries
            .iter()
            .rev()
            .find_map(|dictionary| dictionary.borrow().get(name).cloned())
    }

    fn current_dictionary(
        &self,
    ) -> Result<&Rc<RefCell<BTreeMap<String, Value>>>, GameScriptVmError> {
        self.dictionaries
            .last()
            .ok_or_else(|| self.error("dictionary stack is empty"))
    }

    fn peek(&self) -> Result<&Value, GameScriptVmError> {
        self.operand_stack
            .last()
            .ok_or_else(|| self.error("operand stack underflow"))
    }

    fn pop(&mut self) -> Result<Value, GameScriptVmError> {
        self.operand_stack
            .pop()
            .ok_or_else(|| self.error("operand stack underflow"))
    }

    fn require_stack(&self, count: usize) -> Result<(), GameScriptVmError> {
        if self.operand_stack.len() < count {
            Err(self.error(format!(
                "operand stack underflow: need {count}, have {}",
                self.operand_stack.len()
            )))
        } else {
            Ok(())
        }
    }

    fn pop_number(&mut self, operator: &str) -> Result<f64, GameScriptVmError> {
        match self.pop()? {
            Value::Number(value) => Ok(value),
            value => Err(self.type_error(operator, "number", &value)),
        }
    }

    fn pop_nonnegative_integer(&mut self, purpose: &str) -> Result<usize, GameScriptVmError> {
        let value = self.pop_number(purpose)?;
        number_to_index(value, purpose).map_err(|message| self.error(message))
    }

    fn pop_boolean(&mut self, operator: &str) -> Result<bool, GameScriptVmError> {
        match self.pop()? {
            Value::Boolean(value) => Ok(value),
            value => Err(self.type_error(operator, "boolean", &value)),
        }
    }

    fn pop_literal_name(&mut self, operator: &str) -> Result<String, GameScriptVmError> {
        match self.pop()? {
            Value::LiteralName(value) => Ok(value),
            value => Err(self.type_error(operator, "literal-name", &value)),
        }
    }

    fn pop_procedure(&mut self, operator: &str) -> Result<Vec<Value>, GameScriptVmError> {
        match self.pop()? {
            Value::Procedure(value) => Ok(value.borrow().body.clone()),
            value => Err(self.type_error(operator, "procedure", &value)),
        }
    }

    fn type_error(&self, operator: &str, expected: &str, actual: &Value) -> GameScriptVmError {
        self.error(format!(
            "{operator} expected {expected}, found {}",
            actual.kind()
        ))
    }

    fn error(&self, message: impl Into<String>) -> GameScriptVmError {
        GameScriptVmError {
            message: message.into(),
            step: self.steps,
            call_stack: self.call_stack.clone(),
        }
    }
}

fn number_to_index(number: f64, purpose: &str) -> Result<usize, String> {
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > usize::MAX as f64 {
        Err(format!(
            "{purpose} must be a nonnegative integer, found {number}"
        ))
    } else {
        Ok(number as usize)
    }
}

fn compile_document(document: &GameScriptDocument) -> Result<Vec<Value>, GameScriptVmError> {
    let mut index = 0;
    let values = compile_sequence(&document.tokens, &mut index, false)?;
    if index != document.tokens.len() {
        return Err(compile_error("unexpected trailing procedure delimiter"));
    }
    Ok(values)
}

fn compile_sequence(
    tokens: &[Token],
    index: &mut usize,
    stop_at_procedure_close: bool,
) -> Result<Vec<Value>, GameScriptVmError> {
    let mut values = Vec::new();
    while let Some(token) = tokens.get(*index) {
        *index += 1;
        match &token.kind {
            TokenKind::ExecutableName(name) => values.push(Value::ExecutableName(name.clone())),
            TokenKind::LiteralName(name) => values.push(Value::LiteralName(name.clone())),
            TokenKind::Number(number) => {
                let number = number.parse::<f64>().map_err(|_| {
                    compile_error(format!("could not parse numeric token {number}"))
                })?;
                values.push(Value::Number(number));
            }
            TokenKind::StringLiteral(string) => values.push(Value::String(string.clone())),
            TokenKind::Delimiter(Delimiter::ProcedureOpen) => {
                values.push(Value::Procedure(Rc::new(RefCell::new(ProcedureValue {
                    body: compile_sequence(tokens, index, true)?,
                    metadata: BTreeMap::new(),
                }))));
            }
            TokenKind::Delimiter(Delimiter::ProcedureClose) if stop_at_procedure_close => {
                return Ok(values);
            }
            TokenKind::Delimiter(Delimiter::ProcedureClose) => {
                return Err(compile_error("unexpected closing procedure delimiter }"));
            }
            TokenKind::Delimiter(Delimiter::ArrayOpen) => {
                values.push(Value::ExecutableName("[".to_owned()));
            }
            TokenKind::Delimiter(Delimiter::ArrayClose) => {
                values.push(Value::ExecutableName("]".to_owned()));
            }
            TokenKind::Delimiter(Delimiter::DictionaryOpen) => {
                values.push(Value::ExecutableName("<<".to_owned()));
            }
            TokenKind::Delimiter(Delimiter::DictionaryClose) => {
                values.push(Value::ExecutableName(">>".to_owned()));
            }
        }
    }
    if stop_at_procedure_close {
        Err(compile_error("unclosed procedure delimiter {"))
    } else {
        Ok(values)
    }
}

fn compile_error(message: impl Into<String>) -> GameScriptVmError {
    GameScriptVmError {
        message: message.into(),
        step: 0,
        call_stack: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{GameScriptVm, Value};
    use crate::gamescript::GameScriptDocument;

    fn run(source: &[u8]) -> Result<GameScriptVm, String> {
        let document = GameScriptDocument::parse(source).map_err(|error| error.to_string())?;
        let mut vm = GameScriptVm::new(10_000);
        vm.execute_document(&document)
            .map_err(|error| error.to_string())?;
        Ok(vm)
    }

    #[test]
    fn defines_and_executes_arithmetic_procedures() {
        let vm = run(b"/twice { 2 mul } bind def 21 twice").unwrap();
        assert_eq!(vm.operand_stack(), &[Value::Number(42.0)]);
        assert_eq!(vm.defined_names(), ["twice"]);
    }

    #[test]
    fn copies_and_compares_numbers() {
        let vm = run(b"3 5 2 copy gt { exch } if pop").unwrap();
        assert_eq!(vm.operand_stack(), &[Value::Number(3.0)]);
        assert_eq!(vm.operand_stack()[0].scalar_summary().as_deref(), Some("3"));
    }

    #[test]
    fn builds_runtime_arrays_and_dictionaries() {
        let vm = run(b"[1 2 3] 1 get << /answer 42 >> /answer get").unwrap();
        assert_eq!(
            vm.operand_stack(),
            &[Value::Number(2.0), Value::Number(42.0)]
        );
    }

    #[test]
    fn duplicate_aggregates_share_mutations() {
        let vm = run(b"[1 2] dup 0 9 put 0 get << >> dup /answer 42 put /answer get").unwrap();
        assert_eq!(
            vm.operand_stack(),
            &[Value::Number(9.0), Value::Number(42.0)]
        );
    }

    #[test]
    fn accepts_observed_procedure_metadata_replacement_pattern() {
        let vm = run(b"/utility { 1 } /dummy 5 dict replace bind def utility").unwrap();
        assert_eq!(vm.operand_stack(), &[Value::Number(1.0)]);
        assert_eq!(vm.defined_names(), ["utility"]);
    }

    #[test]
    fn procedure_metadata_does_not_replace_executable_body_tokens() {
        let vm = run(b"/utility { 1 } dup 0 5 dict put bind def utility").unwrap();
        assert_eq!(vm.operand_stack(), &[Value::Number(1.0)]);
    }

    #[test]
    fn native_stubs_supply_state_reads_and_are_counted() {
        // The real GS5R3 difficulty idiom, from gs\MAKEARMY5.gs: index a three-element
        // table by the native difficulty level.
        for (level, expected) in [(0.0, 25.0), (1.0, 50.0), (2.0, 75.0)] {
            let document =
                GameScriptDocument::parse(b"[25 50 75]getdifficultylevel get").unwrap();
            let mut vm = GameScriptVm::new(10_000);
            vm.define_native_stub("getdifficultylevel", Value::Number(level));
            vm.execute_document(&document).unwrap();

            assert_eq!(vm.operand_stack(), &[Value::Number(expected)]);
            assert_eq!(vm.native_calls()["getdifficultylevel"], 1);
        }
    }

    #[test]
    fn the_shipped_extra_strong_body_is_false_on_every_difficulty() {
        // gs\scenario\default.gs lines 147-152 as installed. The mod author described this
        // control as enabling AI stat bonuses on Hard; executing it shows it never does.
        let body = b"[false false false]getdifficultylevel get getmultiplayerflag{pop false}if";
        for level in [0.0, 1.0, 2.0] {
            let document = GameScriptDocument::parse(body).unwrap();
            let mut vm = GameScriptVm::new(10_000);
            vm.define_native_stub("getdifficultylevel", Value::Number(level));
            vm.define_native_stub("getmultiplayerflag", Value::Boolean(false));
            vm.execute_document(&document).unwrap();

            assert_eq!(
                vm.operand_stack(),
                &[Value::Boolean(false)],
                "shipped extra_strong? must be false at difficulty {level}"
            );
        }
    }

    #[test]
    fn an_unknown_name_yields_an_inspectable_trace() {
        let document = GameScriptDocument::parse(b"/probe{1 2 getarmydata}def probe").unwrap();
        let mut vm = GameScriptVm::new(10_000);
        let error = vm.execute_document(&document).unwrap_err();
        let trace = error.unknown_name().expect("a trace for an unknown name");

        assert_eq!(trace.name, "getarmydata");
        assert_eq!(trace.call_stack, vec!["probe".to_owned()]);
        assert!(trace.steps > 0);
    }

    #[test]
    fn executes_conditionals_and_reports_unknown_names_with_a_call_stack() {
        let vm = run(b"true { 7 } { 9 } ifelse").unwrap();
        assert_eq!(vm.operand_stack(), &[Value::Number(7.0)]);

        let document =
            GameScriptDocument::parse(b"/outer { missing_native_call } def outer").unwrap();
        let mut vm = GameScriptVm::new(100);
        let error = vm.execute_document(&document).unwrap_err();
        assert_eq!(error.message, "unknown executable name missing_native_call");
        assert_eq!(error.call_stack, ["outer"]);
    }

    #[test]
    fn enforces_a_step_limit() {
        let document = GameScriptDocument::parse(b"/loop { loop } def loop").unwrap();
        let mut vm = GameScriptVm::new(20);
        let error = vm.execute_document(&document).unwrap_err();
        assert!(error.message.contains("20-step limit"));
    }
}
