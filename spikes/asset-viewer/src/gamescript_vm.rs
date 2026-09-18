//! A bounded GameScript interpreter.
//!
//! GameScript is PostScript-derived: an operand stack, a dictionary stack, executable and literal
//! names, `/name {body} def`. This module implements the *language* half of that — values, stacks,
//! dictionaries, procedures, and the core operators — and deliberately implements none of the
//! engine half. Every name the engine provides natively (`getarmydata`, `rand`, `write`, ...)
//! stops execution with an inspectable trace instead of returning an invented value, because a VM
//! that guesses a host call produces plausible wrong results, which is worse than no result.
//!
//! The same rule binds the *language* half, and it did not at first. `1 0 div` returned `inf`,
//! `1 0 mod` returned `NaN`, and `1 32 bitshift` returned `1` by masking the shift count -- three
//! invented values reached by arithmetic rather than by a host call, one of which then made every
//! later `gt` and `lt` answer `false`. They now stop, as does a shift whose engine-side width
//! behaviour is not established. Execution is bounded in three dimensions, not one: steps,
//! call depth, and allocation size.
//!
//! Two GameScript features are not PostScript and were recovered from the corpus:
//!
//! - **Procedure locals.** `PROC /name VALUE replace` attaches `VALUE` to the procedure under
//!   `name`, and the older `PROC dup 0 N dict put` form attaches a dictionary in slot 0. While
//!   that procedure runs, `name` resolves to the attached value, so `/dummy begin` opens the
//!   procedure's private dictionary and `/char_array exch get` indexes its private array.
//!   Evidence class: Inferred, from `gs\standard.gs`, `gs\autochat.gs`, `gs\chess.gs` and
//!   `gs\citytest.gs` in a local `gs.mpq`, which use both forms interchangeably and always name
//!   the slot-0 dictionary `dummy`.
//! - **Numeric dictionary keys.** `gs\spells\weaken.gs` declares
//!   `/level_advantage_table << 3 1.0 0 0.5 -1 0 >>` and `gs\standard.gs`'s `interpolate` reads it
//!   with `forall`, comparing each key numerically. Keys are therefore typed, not strings.
//!   Evidence class: Observed in a local binary.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use crate::gamescript::{Delimiter, GameScriptDocument, Token, TokenKind};

/// A dictionary key. Names and numbers are distinct key types, as the corpus uses both.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DictKey {
    /// A name key, from `/name` or from a string used as a key.
    Name(String),
    /// A numeric key, held in a canonical decimal form so it can be ordered and compared.
    Number(String),
}

impl DictKey {
    pub fn display(&self) -> &str {
        match self {
            Self::Name(name) | Self::Number(name) => name,
        }
    }

    /// The value this key pushes when a dictionary is walked with `forall`.
    ///
    /// The `unwrap_or` is unreachable rather than a silent fallback: a `DictKey::Number` is only
    /// ever built by `number_key_text`, which formats an `f64`, so the text here is always one
    /// Rust wrote and can read back. No token text reaches this arm. Evidence class: Observed in
    /// a local binary (the sole construction site is `Value::as_dictionary_key`).
    pub fn to_value(&self) -> Value {
        match self {
            Self::Name(name) => Value::LiteralName(name.clone()),
            Self::Number(text) => Value::Number(text.parse::<f64>().unwrap_or(f64::NAN)),
        }
    }
}

/// Canonical decimal text for a number used as a dictionary key.
///
/// Round-trips through `f64`, so `3` and `3.0` are the same key — which is what the corpus needs,
/// since GameScript writes integral table keys without a fraction.
fn number_key_text(number: f64) -> String {
    if number == 0.0 {
        // Collapse -0.0 so it cannot become a second key for zero.
        "0".to_owned()
    } else {
        number.to_string()
    }
}

pub type Dictionary = Rc<RefCell<BTreeMap<DictKey, Value>>>;

fn new_dictionary() -> Dictionary {
    Rc::new(RefCell::new(BTreeMap::new()))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Boolean(bool),
    String(String),
    LiteralName(String),
    ExecutableName(String),
    Procedure(Rc<RefCell<ProcedureValue>>),
    Array(Rc<RefCell<Vec<Value>>>),
    Dictionary(Dictionary),
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

    /// The name GameScript's `type` operator reports, as a literal name.
    ///
    /// Numbers carry no integer/real distinction in this VM, so an integral real reports
    /// `integertype`. `gs\standard.gs`'s `free_stack_elements` branches on exactly that
    /// distinction, so a value it stored as `2.0` would be routed as an integer here.
    fn type_name(&self) -> &'static str {
        match self {
            Self::Number(number) if number.fract() == 0.0 => "integertype",
            Self::Number(_) => "realtype",
            Self::Boolean(_) => "booleantype",
            Self::String(_) => "stringtype",
            Self::LiteralName(_) | Self::ExecutableName(_) => "nametype",
            // PostScript reports a procedure as an array, and the corpus follows it.
            Self::Procedure(_) | Self::Array(_) => "arraytype",
            Self::Dictionary(_) => "dicttype",
            Self::Mark(_) => "marktype",
        }
    }

    pub fn scalar_summary(&self) -> Option<String> {
        match self {
            Self::Number(value) => Some(value.to_string()),
            Self::Boolean(value) => Some(value.to_string()),
            _ => None,
        }
    }

    /// A one-line rendering used by tests and by the driver to state an expected stack.
    pub fn render(&self) -> String {
        match self {
            Self::Number(value) => value.to_string(),
            Self::Boolean(value) => value.to_string(),
            Self::String(value) => format!("\"{value}\""),
            Self::LiteralName(name) => format!("/{name}"),
            Self::ExecutableName(name) => name.clone(),
            Self::Procedure(_) => "{...}".to_owned(),
            Self::Array(values) => {
                let rendered: Vec<String> = values.borrow().iter().map(Value::render).collect();
                format!("[{}]", rendered.join(" "))
            }
            Self::Dictionary(values) => format!("<<{} entries>>", values.borrow().len()),
            Self::Mark(kind) => format!("mark:{kind:?}"),
        }
    }

    /// The dictionary key this value is, if it can be one.
    ///
    /// A **string is not a key.** It used to map to the same variant as a name, which made
    /// `<< /a 1 "a" 2 >> /a get` answer `2`: the second entry silently overwrote the first. The
    /// corpus gives direct evidence for name keys and for numeric keys (`gs\spells\weaken.gs`
    /// keys a table by number) and none at all for string keys, so a string key stops here rather
    /// than aliasing onto a name. If a shipped member turns out to use one, it will stop with a
    /// message saying so, which is the outcome that gets the question answered instead of buried.
    fn as_dictionary_key(&self) -> Option<DictKey> {
        match self {
            Self::LiteralName(name) | Self::ExecutableName(name) => {
                Some(DictKey::Name(name.clone()))
            }
            Self::Number(number) => Some(DictKey::Number(number_key_text(*number))),
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
    /// Values attached to the procedure by `replace`, resolvable by name while it runs.
    locals: BTreeMap<String, Value>,
}

impl ProcedureValue {
    pub fn local_names(&self) -> Vec<String> {
        self.locals.keys().cloned().collect()
    }
}

/// The name the corpus gives the slot-0 dictionary attached by `PROC dup 0 N dict put`.
///
/// Every procedure observed using that form opens the result with `/dummy begin`, and the `replace`
/// form of the same procedures names it `dummy` explicitly. Evidence class: Inferred.
const SLOT_ZERO_LOCAL_NAME: &str = "dummy";

/// How deep script-to-script calls may nest before the VM refuses.
///
/// Recursion consumes the *host* stack, not the step budget, so `/f {f} def f` aborted the whole
/// process with a Rust stack overflow at roughly twenty thousand frames -- long before any step
/// ceiling. That matters most for the `--survey` driver, which runs every member in one process:
/// one recursive member took down the entire run.
///
/// The figure is measured, not chosen for roundness. A debug build overflows a 2 MB thread stack
/// -- which is what `cargo test` gives each test -- somewhere under a thousand of these frames, so
/// a limit set by "far below where the host stack gives out" on the main thread would still abort
/// the test suite. 256 clears the whole corpus (the `--survey` member counts are unchanged by it)
/// and holds on the smallest stack any caller here runs on.
const DEFAULT_MAXIMUM_CALL_DEPTH: usize = 256;

/// The largest `array` or `string` a script may ask this VM to allocate.
const MAXIMUM_ALLOCATION_LENGTH: usize = 1_000_000;

/// Whether execution of a value sequence ran to the end or was cut short by `exit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Normal,
    Exit,
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
    dictionaries: Vec<Dictionary>,
    call_stack: Vec<String>,
    /// One entry per active *named* procedure call, holding that procedure's attached locals.
    /// Anonymous procedure bodies run by `if`, `for`, `forall` and friends do not push a frame,
    /// so they keep seeing the locals of the procedure they are written inside.
    frames: Vec<BTreeMap<String, Value>>,
    steps: usize,
    maximum_steps: usize,
    maximum_call_depth: usize,
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
            dictionaries: vec![new_dictionary()],
            call_stack: Vec::new(),
            frames: Vec::new(),
            steps: 0,
            maximum_steps,
            maximum_call_depth: DEFAULT_MAXIMUM_CALL_DEPTH,
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
        match self.execute_values(&values)? {
            Flow::Normal => Ok(()),
            Flow::Exit => Err(self.error("exit ran outside any loop")),
        }
    }

    pub fn operand_stack(&self) -> &[Value] {
        &self.operand_stack
    }

    pub fn steps(&self) -> usize {
        self.steps
    }

    /// Return the machine to a clean run state while keeping every definition it has loaded.
    ///
    /// A driver that probes many procedures against one loaded module needs this: an exercise that
    /// stops on a native call leaves operands and an open `begin` behind, and carrying that debris
    /// into the next exercise would make its result depend on the previous one's failure.
    pub fn reset_stacks(&mut self) {
        self.operand_stack.clear();
        self.dictionaries.truncate(1);
        self.call_stack.clear();
        self.frames.clear();
    }

    pub fn defined_names(&self) -> Vec<String> {
        self.dictionaries
            .first()
            .map(|dictionary| {
                dictionary
                    .borrow()
                    .keys()
                    .map(|key| key.display().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn execute_values(&mut self, values: &[Value]) -> Result<Flow, GameScriptVmError> {
        for value in values {
            self.steps = self.steps.saturating_add(1);
            if self.steps > self.maximum_steps {
                return Err(self.error(format!(
                    "execution exceeded the {}-step limit",
                    self.maximum_steps
                )));
            }
            match value {
                Value::ExecutableName(name) => {
                    if self.execute_name(name)? == Flow::Exit {
                        return Ok(Flow::Exit);
                    }
                }
                other => self.operand_stack.push(other.clone()),
            }
        }
        Ok(Flow::Normal)
    }

    fn execute_name(&mut self, name: &str) -> Result<Flow, GameScriptVmError> {
        if name == "true" {
            self.operand_stack.push(Value::Boolean(true));
            return Ok(Flow::Normal);
        }
        if name == "false" {
            self.operand_stack.push(Value::Boolean(false));
            return Ok(Flow::Normal);
        }
        if let Some(value) = self.frame_local(name) {
            // A procedure's attached local is data, not something to call.
            self.operand_stack.push(value);
            return Ok(Flow::Normal);
        }
        if let Some(value) = self.lookup(name) {
            return self.execute_resolved(name, value);
        }
        if let Some(flow) = self.execute_builtin(name)? {
            return Ok(flow);
        }
        if let Some(value) = self.native_stubs.get(name).cloned() {
            *self.native_calls.entry(name.to_owned()).or_default() += 1;
            self.operand_stack.push(value);
            return Ok(Flow::Normal);
        }
        Err(self.error(format!("unknown executable name {name}")))
    }

    fn execute_resolved(&mut self, name: &str, value: Value) -> Result<Flow, GameScriptVmError> {
        match value {
            Value::Procedure(procedure) => {
                let (body, locals) = {
                    let procedure = procedure.borrow();
                    (procedure.body.clone(), procedure.locals.clone())
                };
                if self.call_stack.len() >= self.maximum_call_depth {
                    return Err(self.error(format!(
                        "procedure call depth exceeded the {}-frame limit at {name}",
                        self.maximum_call_depth
                    )));
                }
                self.call_stack.push(name.to_owned());
                self.frames.push(locals);
                let result = self.execute_values(&body);
                self.frames.pop();
                self.call_stack.pop();
                result
            }
            Value::ExecutableName(alias) => self.execute_name(&alias),
            other => {
                self.operand_stack.push(other);
                Ok(Flow::Normal)
            }
        }
    }

    /// Dispatch a language primitive. `Ok(None)` means the name is not a primitive.
    ///
    /// The quoted match arms below are the single source of [`PRIMITIVE_NAMES`]; the markers let a
    /// test read them back out of this file and refuse to let the two drift apart.
    fn execute_builtin(&mut self, name: &str) -> Result<Option<Flow>, GameScriptVmError> {
        match name {
            // PRIMITIVE-DISPATCH-BEGIN
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
            "index" => {
                let depth = self.pop_nonnegative_integer("index depth")?;
                self.require_stack(depth + 1)?;
                let value = self.operand_stack[self.operand_stack.len() - depth - 1].clone();
                self.operand_stack.push(value);
            }
            "roll" => self.roll()?,
            "count" => {
                let depth = self.operand_stack.len();
                self.operand_stack.push(Value::Number(depth as f64));
            }
            "clear" => self.operand_stack.clear(),
            "dict" => {
                self.pop_nonnegative_integer("dict capacity")?;
                self.operand_stack.push(Value::Dictionary(new_dictionary()));
            }
            "array" => {
                let length = self.pop_allocation_length("array length")?;
                self.operand_stack
                    .push(Value::Array(Rc::new(RefCell::new(vec![
                        Value::Number(0.0);
                        length
                    ]))));
            }
            "string" => {
                let length = self.pop_allocation_length("string length")?;
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
                let key = self.pop_key("def")?;
                self.current_dictionary()?.borrow_mut().insert(key, value);
            }
            "undef" => {
                let key = self.pop_key("undef")?;
                self.current_dictionary()?.borrow_mut().remove(&key);
            }
            "known" => {
                let key = self.pop_key("known")?;
                let dictionary = self.pop_dictionary("known")?;
                let known = dictionary.borrow().contains_key(&key);
                self.operand_stack.push(Value::Boolean(known));
            }
            "load" => {
                let key = self.pop_key("load")?;
                let value = self
                    .lookup_key(&key)
                    .ok_or_else(|| self.error(format!("load found no name {}", key.display())))?;
                self.operand_stack.push(value);
            }
            "bind" => {
                self.peek()?;
            }
            "replace" => self.replace_local()?,
            "put" => self.put()?,
            "get" => self.get()?,
            "length" => self.length()?,
            "begin" => {
                let dictionary = self.pop_dictionary("begin")?;
                self.dictionaries.push(dictionary);
            }
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
                return self.execute_resolved("exec", value).map(Some);
            }
            "cvx" => {
                let value = self.pop()?;
                match value {
                    Value::LiteralName(name) | Value::String(name) => {
                        self.operand_stack.push(Value::ExecutableName(name));
                    }
                    other => self.operand_stack.push(other),
                }
            }
            "cvlit" => {
                let value = self.pop()?;
                match value {
                    Value::ExecutableName(name) => {
                        self.operand_stack.push(Value::LiteralName(name));
                    }
                    other => self.operand_stack.push(other),
                }
            }
            "cvi" => {
                let value = self.pop_number("cvi")?;
                self.operand_stack.push(Value::Number(value.trunc()));
            }
            "cvr" => {
                let value = self.pop_number("cvr")?;
                self.operand_stack.push(Value::Number(value));
            }
            "cvs" => {
                // `value buffer cvs` renders into the buffer and returns it. This VM has no
                // mutable string buffers, so the buffer is consumed and the rendering returned.
                let buffer = self.pop()?;
                if !matches!(buffer, Value::String(_)) {
                    return Err(self.type_error("cvs", "string buffer", &buffer));
                }
                let value = self.pop()?;
                let rendered = match value {
                    Value::Number(number) => number.to_string(),
                    Value::Boolean(flag) => flag.to_string(),
                    Value::String(text) => text,
                    Value::LiteralName(name) | Value::ExecutableName(name) => name,
                    other => return Err(self.type_error("cvs", "a printable value", &other)),
                };
                self.operand_stack.push(Value::String(rendered));
            }
            "type" => {
                let value = self.pop()?;
                self.operand_stack
                    .push(Value::LiteralName(value.type_name().to_owned()));
            }
            "add" => self.binary_number("add", |left, right| left + right)?,
            "sub" => self.binary_number("sub", |left, right| left - right)?,
            "mul" => self.binary_number("mul", |left, right| left * right)?,
            "div" => self.binary_number("div", |left, right| left / right)?,
            "idiv" => self.binary_number("idiv", |left, right| (left / right).trunc())?,
            "mod" => self.binary_number("mod", |left, right| left % right)?,
            "neg" => self.unary_number("neg", |value| -value)?,
            "abs" => self.unary_number("abs", f64::abs)?,
            "sqrt" => self.unary_number("sqrt", f64::sqrt)?,
            // `gs\standard.gs` defines `/radians {180 div 3.141596 mul}` and applies it before
            // every `sin`/`cos`, so these take radians, unlike PostScript's degree-taking pair.
            // Evidence class: Inferred, from the corpus's own conversion.
            "sin" => self.unary_number("sin", f64::sin)?,
            "cos" => self.unary_number("cos", f64::cos)?,
            "round" => self.unary_number("round", f64::round)?,
            "truncate" => self.unary_number("truncate", f64::trunc)?,
            "floor" => self.unary_number("floor", f64::floor)?,
            "ceiling" => self.unary_number("ceiling", f64::ceil)?,
            "bitshift" => self.bitshift()?,
            "eq" => {
                let right = self.pop()?;
                let left = self.pop()?;
                self.operand_stack.push(Value::Boolean(left == right));
            }
            "ne" => {
                let right = self.pop()?;
                let left = self.pop()?;
                self.operand_stack.push(Value::Boolean(left != right));
            }
            "gt" => self.compare_numbers("gt", |left, right| left > right)?,
            "lt" => self.compare_numbers("lt", |left, right| left < right)?,
            "ge" => self.compare_numbers("ge", |left, right| left >= right)?,
            "le" => self.compare_numbers("le", |left, right| left <= right)?,
            "and" => self.logical(
                "and",
                |left, right| left & right,
                |left, right| left && right,
            )?,
            "or" => self.logical(
                "or",
                |left, right| left | right,
                |left, right| left || right,
            )?,
            "xor" => self.logical(
                "xor",
                |left, right| left ^ right,
                |left, right| left != right,
            )?,
            "not" => match self.pop()? {
                Value::Boolean(value) => self.operand_stack.push(Value::Boolean(!value)),
                Value::Number(value) => {
                    self.operand_stack
                        .push(Value::Number(!(value as i64) as f64));
                }
                value => return Err(self.type_error("not", "boolean or number", &value)),
            },
            "if" => {
                let procedure = self.pop_procedure("if")?;
                if self.pop_boolean("if")? {
                    return self.execute_values(&procedure).map(Some);
                }
            }
            "ifelse" => {
                let otherwise = self.pop_procedure("ifelse")?;
                let then = self.pop_procedure("ifelse")?;
                let condition = self.pop_boolean("ifelse")?;
                return self
                    .execute_values(if condition { &then } else { &otherwise })
                    .map(Some);
            }
            "repeat" => {
                let procedure = self.pop_procedure("repeat")?;
                let count = self.pop_nonnegative_integer("repeat count")?;
                for _ in 0..count {
                    // An empty body executes no values, so without charging the iteration itself
                    // `100000000000 {} repeat` would never reach the step ceiling. `for` and
                    // `loop` both charge; `repeat` did not.
                    self.charge_step()?;
                    if self.execute_values(&procedure)? == Flow::Exit {
                        break;
                    }
                }
            }
            "for" => return self.for_loop().map(Some),
            "loop" => {
                let procedure = self.pop_procedure("loop")?;
                loop {
                    self.steps = self.steps.saturating_add(1);
                    if self.steps > self.maximum_steps {
                        return Err(self.error(format!(
                            "execution exceeded the {}-step limit",
                            self.maximum_steps
                        )));
                    }
                    if self.execute_values(&procedure)? == Flow::Exit {
                        break;
                    }
                }
            }
            "exit" => return Ok(Some(Flow::Exit)),
            "forall" => return self.forall().map(Some),
            // PRIMITIVE-DISPATCH-END
            _ => return Ok(None),
        }
        Ok(Some(Flow::Normal))
    }

    fn roll(&mut self) -> Result<(), GameScriptVmError> {
        let shift = self.pop_number("roll shift")? as i64;
        let count = self.pop_nonnegative_integer("roll count")?;
        self.require_stack(count)?;
        if count == 0 {
            return Ok(());
        }
        let start = self.operand_stack.len() - count;
        let shift = shift.rem_euclid(count as i64) as usize;
        self.operand_stack[start..].rotate_right(shift);
        Ok(())
    }

    fn for_loop(&mut self) -> Result<Flow, GameScriptVmError> {
        let procedure = self.pop_procedure("for")?;
        let limit = self.pop_number("for limit")?;
        let increment = self.pop_number("for increment")?;
        let initial = self.pop_number("for initial")?;
        if increment == 0.0 {
            return Err(self.error("for increment must not be zero"));
        }
        let mut control = initial;
        while (increment > 0.0 && control <= limit) || (increment < 0.0 && control >= limit) {
            self.steps = self.steps.saturating_add(1);
            if self.steps > self.maximum_steps {
                return Err(self.error(format!(
                    "execution exceeded the {}-step limit",
                    self.maximum_steps
                )));
            }
            self.operand_stack.push(Value::Number(control));
            if self.execute_values(&procedure)? == Flow::Exit {
                break;
            }
            control += increment;
        }
        Ok(Flow::Normal)
    }

    fn forall(&mut self) -> Result<Flow, GameScriptVmError> {
        let procedure = self.pop_procedure("forall")?;
        let aggregate = self.pop_aggregate("forall")?;
        match aggregate {
            Value::Array(values) => {
                let values = values.borrow().clone();
                for value in values {
                    self.operand_stack.push(value);
                    if self.execute_values(&procedure)? == Flow::Exit {
                        break;
                    }
                }
            }
            Value::String(text) => {
                for byte in text.into_bytes() {
                    self.operand_stack.push(Value::Number(f64::from(byte)));
                    if self.execute_values(&procedure)? == Flow::Exit {
                        break;
                    }
                }
            }
            Value::Dictionary(entries) => {
                let entries: Vec<(DictKey, Value)> = entries
                    .borrow()
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                for (key, value) in entries {
                    self.operand_stack.push(key.to_value());
                    self.operand_stack.push(value);
                    if self.execute_values(&procedure)? == Flow::Exit {
                        break;
                    }
                }
            }
            other => return Err(self.type_error("forall", "array, string or dictionary", &other)),
        }
        Ok(Flow::Normal)
    }

    fn length(&mut self) -> Result<(), GameScriptVmError> {
        let aggregate = self.pop_aggregate("length")?;
        let length = match aggregate {
            Value::Array(values) => values.borrow().len(),
            Value::String(text) => text.len(),
            Value::Dictionary(entries) => entries.borrow().len(),
            Value::Procedure(procedure) => procedure.borrow().body.len(),
            other => {
                return Err(self.type_error(
                    "length",
                    "array, string, dictionary or procedure",
                    &other,
                ));
            }
        };
        self.operand_stack.push(Value::Number(length as f64));
        Ok(())
    }

    fn close_collection(&mut self, kind: CollectionKind) -> Result<(), GameScriptVmError> {
        let Some(mark_index) = self
            .operand_stack
            .iter()
            .rposition(|value| *value == Value::Mark(kind))
        else {
            return Err(self.error(format!("closing {kind:?} has no matching mark")));
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
                    let key = pair[0].as_dictionary_key().ok_or_else(|| {
                        self.type_error("dictionary literal", "name or number key", &pair[0])
                    })?;
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
        let aggregate = self.pop_aggregate("put")?;
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
                let mut procedure = procedure.borrow_mut();
                procedure.metadata.insert(index, value.clone());
                if index == 0 {
                    // The older attachment form. The corpus always opens the result with
                    // `/dummy begin`, so slot zero is that name. Evidence class: Inferred.
                    procedure
                        .locals
                        .insert(SLOT_ZERO_LOCAL_NAME.to_owned(), value);
                }
            }
            (Value::Dictionary(values), index) => {
                let key = index
                    .as_dictionary_key()
                    .ok_or_else(|| self.type_error("put", "name or number key", &index))?;
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

    /// `PROC /name VALUE replace` attaches `VALUE` to `PROC` under `name` and leaves `PROC`.
    ///
    /// Evidence class: Observed in a local binary for the shape (`gs\standard.gs`,
    /// `gs\autochat.gs`, `gs\chess.gs`, `gs\citytest.gs`); Inferred for the meaning, which is what
    /// makes every one of those procedures' bodies resolve.
    fn replace_local(&mut self) -> Result<(), GameScriptVmError> {
        self.require_stack(3)?;
        let length = self.operand_stack.len();
        let Value::LiteralName(name) = self.operand_stack[length - 2].clone() else {
            return Err(self.error(format!(
                "replace expects PROCEDURE /name VALUE, found {} where the name belongs",
                self.operand_stack[length - 2].kind()
            )));
        };
        let Value::Procedure(procedure) = self.operand_stack[length - 3].clone() else {
            return Err(self.error(format!(
                "replace expects PROCEDURE /name VALUE, found {} where the procedure belongs",
                self.operand_stack[length - 3].kind()
            )));
        };
        let value = self.pop()?;
        self.pop()?;
        procedure.borrow_mut().locals.insert(name, value);
        Ok(())
    }

    fn get(&mut self) -> Result<(), GameScriptVmError> {
        let index = self.pop()?;
        let aggregate = self.pop_aggregate("get")?;
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
            (Value::String(text), Value::Number(index)) => {
                let index =
                    number_to_index(index, "get index").map_err(|message| self.error(message))?;
                text.as_bytes()
                    .get(index)
                    .map(|byte| Value::Number(f64::from(*byte)))
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
            (Value::Dictionary(values), index) => {
                let key = index
                    .as_dictionary_key()
                    .ok_or_else(|| self.type_error("get", "name or number key", &index))?;
                values
                    .borrow()
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| self.error(format!("dictionary has no key {}", key.display())))?
            }
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
        let result = function(left, right);
        self.push_defined_result(operator, result)
    }

    fn unary_number(
        &mut self,
        operator: &str,
        function: impl FnOnce(f64) -> f64,
    ) -> Result<(), GameScriptVmError> {
        let value = self.pop_number(operator)?;
        let result = function(value);
        self.push_defined_result(operator, result)
    }

    /// Push an arithmetic result, or stop if it is not a number.
    ///
    /// `1 0 div` is `inf` in IEEE arithmetic and `undefinedresult` in PostScript. Returning `inf`
    /// or `NaN` is the same failure as inventing a host call: `1 0 div 1000000 gt` answers `true`,
    /// and a `NaN` then makes every later `gt` *and* `lt` answer `false`, so a wrong result
    /// propagates silently and looks plausible. Every operand here was finite, so a non-finite
    /// result is this operator's doing and it stops.
    fn push_defined_result(
        &mut self,
        operator: &str,
        result: f64,
    ) -> Result<(), GameScriptVmError> {
        if !result.is_finite() {
            return Err(self.error(format!(
                "{operator} has no defined result for its operands (it computed {result})"
            )));
        }
        self.operand_stack.push(Value::Number(result));
        Ok(())
    }

    /// `bitshift` on the engine's 32-bit integers.
    ///
    /// Two behaviours were wrong here. `f64 as i64` saturates, so `1 -1e300 bitshift` reached
    /// `i64::MIN` and the negation panicked outright in a debug build. And `wrapping_shl` masks
    /// the shift count, so `1 32 bitshift` answered `1` and `1 64 bitshift` answered `1` -- a
    /// fabricated flag rather than a stop, which matters because `getflagvalue` is
    /// `1 exch bitshift and`.
    ///
    /// A shift of 32 or more is **refused** rather than modelled. A 32-bit x86 `shl` masks the
    /// count to five bits and would answer `1`, while C's `1 << 32` is undefined and a compiler
    /// may fold it to `0`; which one `lomse.exe` does is not established here, and guessing it is
    /// exactly what this VM must not do. Within range the shift is computed on the 32-bit pattern
    /// with vacated bits zero-filled, which is what PostScript documents. Evidence class:
    /// Documented for the in-range rule, Refused for the out-of-range case.
    fn bitshift(&mut self) -> Result<(), GameScriptVmError> {
        let shift = self.pop_number("bitshift")?;
        let value = self.pop_number("bitshift")?;
        if shift.fract() != 0.0 || !(-31.0..=31.0).contains(&shift) {
            return Err(self.error(format!(
                "bitshift needs a whole shift count in -31..=31, found {shift}; the engine's behaviour beyond the 32-bit width is not established"
            )));
        }
        if value.fract() != 0.0 || !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&value) {
            return Err(self.error(format!(
                "bitshift needs a 32-bit integer operand, found {value}"
            )));
        }
        let pattern = value as i32 as u32;
        let shifted = if shift >= 0.0 {
            pattern << (shift as u32)
        } else {
            pattern >> ((-shift) as u32)
        };
        self.operand_stack
            .push(Value::Number(f64::from(shifted as i32)));
        Ok(())
    }

    fn compare_numbers(
        &mut self,
        operator: &str,
        function: impl FnOnce(f64, f64) -> bool,
    ) -> Result<(), GameScriptVmError> {
        let right = self.pop_number(operator)?;
        let left = self.pop_number(operator)?;
        // A `NaN` answers `false` to `gt` *and* to `lt`, so an ordering comparison on one is not a
        // comparison at all. Arithmetic here can no longer produce one, but a native stub can.
        if left.is_nan() || right.is_nan() {
            return Err(self.error(format!("{operator} cannot order a not-a-number operand")));
        }
        self.operand_stack
            .push(Value::Boolean(function(left, right)));
        Ok(())
    }

    /// `and`, `or` and `xor` are bitwise on numbers and logical on booleans, as in PostScript.
    fn logical(
        &mut self,
        operator: &str,
        bitwise: impl FnOnce(i64, i64) -> i64,
        boolean: impl FnOnce(bool, bool) -> bool,
    ) -> Result<(), GameScriptVmError> {
        let right = self.pop()?;
        let left = self.pop()?;
        match (left, right) {
            (Value::Number(left), Value::Number(right)) => {
                self.operand_stack
                    .push(Value::Number(bitwise(left as i64, right as i64) as f64));
            }
            (Value::Boolean(left), Value::Boolean(right)) => {
                self.operand_stack
                    .push(Value::Boolean(boolean(left, right)));
            }
            (left, right) => {
                return Err(self.error(format!(
                    "{operator} expects two numbers or two booleans, found {} and {}",
                    left.kind(),
                    right.kind()
                )));
            }
        }
        Ok(())
    }

    /// A value attached to the procedure currently executing, if it carries this name.
    fn frame_local(&self, name: &str) -> Option<Value> {
        self.frames.last()?.get(name).cloned()
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        self.lookup_key(&DictKey::Name(name.to_owned()))
    }

    fn lookup_key(&self, key: &DictKey) -> Option<Value> {
        self.dictionaries
            .iter()
            .rev()
            .find_map(|dictionary| dictionary.borrow().get(key).cloned())
    }

    fn current_dictionary(&self) -> Result<&Dictionary, GameScriptVmError> {
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

    /// Pop a value that is meant to be an aggregate, resolving a procedure-local name.
    ///
    /// `gs\standard.gs`'s `char_cvs` reads its attached array as `/char_array exch get`, and
    /// `gs\autochat.gs` reads its attached array as `/dummy length`. A literal name is never a
    /// legal aggregate otherwise, so this resolution only fires where the alternative is a type
    /// error, and it never changes the meaning of a name that does resolve.
    fn pop_aggregate(&mut self, operator: &str) -> Result<Value, GameScriptVmError> {
        match self.pop()? {
            Value::LiteralName(name) => self.frame_local(&name).ok_or_else(|| {
                self.error(format!(
                    "{operator} received the name /{name}, which is not an aggregate and is not a local of the running procedure"
                ))
            }),
            other => Ok(other),
        }
    }

    fn pop_dictionary(&mut self, operator: &str) -> Result<Dictionary, GameScriptVmError> {
        match self.pop_aggregate(operator)? {
            Value::Dictionary(dictionary) => Ok(dictionary),
            value => Err(self.type_error(operator, "dictionary", &value)),
        }
    }

    /// Charge one step against the ceiling. Loop constructs call this per iteration so that a
    /// body executing no values still cannot run forever.
    fn charge_step(&mut self) -> Result<(), GameScriptVmError> {
        self.steps = self.steps.saturating_add(1);
        if self.steps > self.maximum_steps {
            return Err(self.error(format!(
                "execution exceeded the {}-step limit",
                self.maximum_steps
            )));
        }
        Ok(())
    }

    /// Pop a script-controlled allocation length, refusing one this process should not attempt.
    ///
    /// A step ceiling bounds time, not memory: `100000000000 array` asks for 1.6 TB of `Value`
    /// before a single step is charged. The limit is a refusal, not a silent clamp -- a shorter
    /// array than the script asked for would quietly change what the script computes.
    fn pop_allocation_length(&mut self, purpose: &str) -> Result<usize, GameScriptVmError> {
        let length = self.pop_nonnegative_integer(purpose)?;
        if length > MAXIMUM_ALLOCATION_LENGTH {
            return Err(self.error(format!(
                "{purpose} {length} exceeds the {MAXIMUM_ALLOCATION_LENGTH}-element allocation limit"
            )));
        }
        Ok(length)
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

    /// Pop a condition.
    ///
    /// A number is a condition too: `gs\standard.gs`'s `getflagvalue` is
    /// `1 exch bitshift and {true}{false} ifelse`, where `and` on two integers yields an integer
    /// that `ifelse` then consumes. A strict boolean-only `ifelse` cannot run the shipped flag
    /// helpers at all. Evidence class: Inferred, from that procedure executing correctly for every
    /// bit only under this rule.
    fn pop_boolean(&mut self, operator: &str) -> Result<bool, GameScriptVmError> {
        match self.pop()? {
            Value::Boolean(value) => Ok(value),
            Value::Number(value) => Ok(value != 0.0),
            value => Err(self.type_error(operator, "boolean or number", &value)),
        }
    }

    fn pop_key(&mut self, operator: &str) -> Result<DictKey, GameScriptVmError> {
        let value = self.pop()?;
        value
            .as_dictionary_key()
            .ok_or_else(|| self.type_error(operator, "name or number key", &value))
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

/// Every language primitive this VM implements.
///
/// This is the boundary that makes the vocabulary classification mechanical: a name the engine's
/// operator table lists is a *native host call* exactly when this VM does not implement it. The
/// list is checked against the dispatch itself by `primitive_names_match_the_dispatch`, so it
/// cannot quietly drift into claiming a primitive that does not exist.
pub const PRIMITIVE_NAMES: &[&str] = &[
    "<<",
    ">>",
    "[",
    "]",
    "abs",
    "add",
    "and",
    "array",
    "begin",
    "bind",
    "bitshift",
    "ceiling",
    "clear",
    "copy",
    "cos",
    "count",
    "currentdict",
    "cvi",
    "cvlit",
    "cvr",
    "cvs",
    "cvx",
    "def",
    "dict",
    "div",
    "dup",
    "end",
    "eq",
    "exch",
    "exec",
    "exit",
    "floor",
    "for",
    "forall",
    "ge",
    "get",
    "gt",
    "idiv",
    "if",
    "ifelse",
    "index",
    "known",
    "le",
    "length",
    "load",
    "loop",
    "lt",
    "mod",
    "mul",
    "ne",
    "neg",
    "not",
    "or",
    "pop",
    "put",
    "repeat",
    "replace",
    "roll",
    "round",
    "sin",
    "sqrt",
    "string",
    "sub",
    "truncate",
    "type",
    "undef",
    "xor",
];

/// The two names the lexer turns into pushed values before dispatch ever sees them.
pub const PRIMITIVE_LITERAL_NAMES: &[&str] = &["true", "false"];

/// Whether the VM implements `name` as a language primitive.
pub fn is_primitive(name: &str) -> bool {
    PRIMITIVE_NAMES.contains(&name) || PRIMITIVE_LITERAL_NAMES.contains(&name)
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
                // Every form `gamescript::is_number_token` admits is also accepted by
                // `f64::from_str`, so this error is unreachable for tokens from that lexer; it
                // stays as the honest failure for a `Token` assembled by hand.
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
                    locals: BTreeMap::new(),
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
    use super::{GameScriptVm, PRIMITIVE_NAMES, Value};
    use crate::gamescript::GameScriptDocument;
    use std::collections::BTreeSet;

    fn run(source: &[u8]) -> Result<GameScriptVm, String> {
        let document = GameScriptDocument::parse(source).map_err(|error| error.to_string())?;
        let mut vm = GameScriptVm::new(100_000);
        vm.execute_document(&document)
            .map_err(|error| error.to_string())?;
        Ok(vm)
    }

    fn rendered_stack(source: &[u8]) -> String {
        let vm = run(source)
            .unwrap_or_else(|error| panic!("{}: {error}", String::from_utf8_lossy(source)));
        vm.operand_stack()
            .iter()
            .map(Value::render)
            .collect::<Vec<_>>()
            .join(" ")
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
    fn stack_operators_reorder_deterministically() {
        // `roll` and `index` are what `gs\standard.gs`'s `between` and its own `/index` are built
        // from; getting the direction wrong is the classic silent error.
        assert_eq!(rendered_stack(b"1 2 3 3 1 roll"), "3 1 2");
        assert_eq!(rendered_stack(b"1 2 3 3 -1 roll"), "2 3 1");
        assert_eq!(rendered_stack(b"1 2 3 0 index"), "1 2 3 3");
        assert_eq!(rendered_stack(b"1 2 3 2 index"), "1 2 3 1");
        assert_eq!(rendered_stack(b"1 2 3 count"), "1 2 3 3");
        assert_eq!(rendered_stack(b"1 2 3 clear"), "");
    }

    #[test]
    fn control_flow_loops_and_exits() {
        assert_eq!(rendered_stack(b"0 5 {1 add} repeat"), "5");
        assert_eq!(rendered_stack(b"0 1 1 10 {add} for"), "55");
        assert_eq!(rendered_stack(b"0 10 -2 0 {add} for"), "30");
        // `exit` must break the innermost loop and leave the enclosing sequence running.
        // Sums 1..4, then the control value 5 trips the guard, is dropped, and the loop exits.
        assert_eq!(
            rendered_stack(b"0 1 1 10 {dup 4 gt {pop exit} if add} for 99"),
            "10 99"
        );
        assert_eq!(rendered_stack(b"0 {1 add dup 3 ge {exit} if} loop"), "3");
    }

    #[test]
    fn forall_walks_arrays_strings_and_dictionaries() {
        assert_eq!(rendered_stack(b"0 [1 2 3] {add} forall"), "6");
        // A string yields character codes: this is what `string_cvi` is built on.
        assert_eq!(rendered_stack(b"[ \"AB\" {} forall ]"), "[65 66]");
        // A dictionary yields key then value, and a numeric key comes back as a number.
        assert_eq!(rendered_stack(b"<< 3 1.5 >> {} forall"), "3 1.5");
    }

    /// Two key types that looked like one. A string key aliasing onto a name key is a *silent
    /// overwrite*, which is the failure mode this VM exists to avoid.
    #[test]
    fn a_string_is_not_a_dictionary_key() {
        // The aliasing this prevents: `"a"` used to land on the same key as `/a`, so the literal
        // below answered 2 for `/a get` and the first entry vanished.
        let error = run(b"<< /a 1 \"a\" 2 >>").unwrap_err();
        assert!(
            error.contains("name or number key"),
            "a string key must stop, got {error}"
        );
        // Access refuses it for the same reason, rather than reading a neighbouring entry.
        let error = run(b"<< /a 1 >> \"a\" get").unwrap_err();
        assert!(error.contains("name or number key"), "{error}");
        let error = run(b"<< >> dup \"a\" 1 put").unwrap_err();
        assert!(error.contains("name or number key"), "{error}");
        // Names and numbers still work, and remain distinct from each other.
        assert_eq!(rendered_stack(b"<< /a 1 >> /a get"), "1");
        assert_eq!(
            rendered_stack(b"<< 3 1 >> dup 3 known exch /3 known"),
            "true false"
        );
    }

    #[test]
    fn numeric_dictionary_keys_are_not_names() {
        // `gs\spells\weaken.gs` keys its level-advantage table by number.
        assert_eq!(rendered_stack(b"<< 3 1 0 0.5 -1 0 >> 0 get"), "0.5");
        assert_eq!(
            rendered_stack(b"<< 3 1 >> dup 3 known exch /3 known"),
            "true false"
        );
    }

    #[test]
    fn a_procedure_local_resolves_by_name_while_that_procedure_runs() {
        // `PROC /name VALUE replace` is how the corpus gives a procedure private storage.
        let vm =
            run(b"/pick { /table exch get } /table [10 20 30] replace bind def 2 pick").unwrap();
        assert_eq!(vm.operand_stack(), &[Value::Number(30.0)]);

        // ... and it is private: the same name outside the procedure is not defined.
        let error = run(b"/pick { 1 } /table [10] replace bind def table").unwrap_err();
        assert!(error.contains("unknown executable name table"), "{error}");
    }

    #[test]
    fn a_local_dictionary_opens_with_begin_and_keeps_definitions_private() {
        // The `/dummy begin ... end` idiom, in both attachment forms.
        let vm = run(b"/scale { /dummy begin /factor exch def factor factor mul end } /dummy 2 dict replace bind def 7 scale").unwrap();
        assert_eq!(vm.operand_stack(), &[Value::Number(49.0)]);

        let vm = run(b"/scale { /dummy begin /factor exch def factor 3 mul end } dup 0 2 dict put bind def 7 scale").unwrap();
        assert_eq!(vm.operand_stack(), &[Value::Number(21.0)]);

        let error = run(b"/scale { /dummy begin /factor exch def end } /dummy 2 dict replace bind def 7 scale factor").unwrap_err();
        assert!(error.contains("unknown executable name factor"), "{error}");
    }

    #[test]
    fn known_and_undef_operate_on_the_open_local_dictionary() {
        let source = b"/probe { /dummy begin /seen 1 def /dummy /seen known /seen undef /dummy /seen known end } /dummy 2 dict replace bind def probe";
        assert_eq!(rendered_stack(source), "true false");
    }

    #[test]
    fn native_stubs_supply_state_reads_and_are_counted() {
        // The real GS5R3 difficulty idiom, from gs\MAKEARMY5.gs: index a three-element
        // table by the native difficulty level.
        for (level, expected) in [(0.0, 25.0), (1.0, 50.0), (2.0, 75.0)] {
            let document = GameScriptDocument::parse(b"[25 50 75]getdifficultylevel get").unwrap();
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

        // A `loop` with no `exit` must also be bounded, or a survey run never returns.
        let document = GameScriptDocument::parse(b"{1 pop} loop").unwrap();
        let mut vm = GameScriptVm::new(50);
        let error = vm.execute_document(&document).unwrap_err();
        assert!(error.message.contains("50-step limit"), "{}", error.message);
    }

    /// Issue #5 forbids inventing a value for something the VM cannot compute. IEEE arithmetic
    /// invents them cheerfully, and a released `inf` or `NaN` is indistinguishable from a real
    /// answer two operators later.
    #[test]
    fn arithmetic_with_no_defined_result_stops_instead_of_answering() {
        for source in [
            &b"1 0 div"[..],
            b"1 0 idiv",
            b"1 0 mod",
            b"-4 sqrt",
            b"1e308 1e308 mul",
        ] {
            let error = run(source).unwrap_err();
            assert!(
                error.contains("has no defined result"),
                "{} should have stopped, got {error}",
                String::from_utf8_lossy(source)
            );
        }

        // The specific poisoning this prevents: a released infinity compares as a real number.
        assert!(run(b"1 0 div 1000000 gt").is_err());
        // A stub is the remaining way to get a NaN onto the stack; ordering it is refused.
        let document = GameScriptDocument::parse(b"1 nan_source gt").unwrap();
        let mut vm = GameScriptVm::new(1_000);
        vm.define_native_stub("nan_source", Value::Number(f64::NAN));
        let error = vm.execute_document(&document).unwrap_err();
        assert_eq!(error.message, "gt cannot order a not-a-number operand");
    }

    /// Catch a primitive wired to the wrong function, which the corpus battery would catch but
    /// which only runs against a local archive.
    ///
    /// Asserted as *identities*, not as tabulated values: `sin` is odd and `cos` is even, so
    /// `sin(-x) + sin(x)` is zero and `cos(-x) - cos(x)` is zero. Swapping the two makes both
    /// sums non-zero, and the orderings below disagree as well. These are facts about the
    /// functions rather than a restatement of the dispatch, so the test can fail on the dispatch
    /// being wrong. Verified by mutation: pointing `sin` at `f64::cos` fails it.
    #[test]
    fn trigonometry_is_not_self_consistent_under_a_swap() {
        // sin is odd.
        assert_eq!(rendered_stack(b"-1 sin 1 sin add abs 1e-12 lt"), "true");
        // cos is even.
        assert_eq!(rendered_stack(b"-1 cos 1 cos sub abs 1e-12 lt"), "true");
        // And they are ordered oppositely on either side of their crossing near 0.785 radians.
        assert_eq!(rendered_stack(b"0 sin 0 cos lt"), "true");
        assert_eq!(rendered_stack(b"1 sin 1 cos gt"), "true");
    }

    /// The remaining arithmetic and comparison primitives, pinned by properties that a
    /// wrong-operand-order or wrong-operator wiring breaks.
    #[test]
    fn arithmetic_and_comparison_primitives_are_wired_to_the_right_operations() {
        // Subtraction and division are not commutative, so these catch a swapped operand order
        // that `add` and `mul` cannot.
        assert_eq!(rendered_stack(b"5 3 sub 3 5 sub"), "2 -2");
        assert_eq!(rendered_stack(b"6 3 div 3 6 div"), "2 0.5");
        assert_eq!(rendered_stack(b"7 2 idiv 7 2 mod"), "3 1");
        // add and mul, checked against each other rather than against a literal: doubling and
        // squaring agree only at 0 and 2, so a mul wired to add would show up at 3.
        assert_eq!(rendered_stack(b"3 3 add 3 3 mul eq"), "false");
        assert_eq!(rendered_stack(b"2 2 add 2 2 mul eq"), "true");
        // The ordering family, each against its own negation.
        assert_eq!(
            rendered_stack(b"1 2 lt 2 1 lt 1 2 gt 2 1 gt"),
            "true false false true"
        );
        assert_eq!(
            rendered_stack(b"2 2 le 2 2 ge 2 2 lt 2 2 gt"),
            "true true false false"
        );
        assert_eq!(rendered_stack(b"2 2 eq 2 2 ne"), "true false");
        // Bitwise on numbers, logical on booleans, from the same three operators.
        assert_eq!(rendered_stack(b"6 3 and 6 3 or 6 3 xor"), "2 7 5");
        assert_eq!(
            rendered_stack(b"true false and true false or true false xor"),
            "false true true"
        );
        // Rounding directions are distinct: a single wrong wiring collapses two of these.
        assert_eq!(
            rendered_stack(b"-1.5 floor -1.5 ceiling -1.5 truncate -1.5 abs"),
            "-2 -1 -1 1.5"
        );
        assert_eq!(rendered_stack(b"9 sqrt 3 neg"), "3 -3");
        // `type` distinguishes what the corpus branches on.
        assert_eq!(
            rendered_stack(b"2 type 2.5 type true type \"a\" type"),
            "/integertype /realtype /booleantype /stringtype"
        );
    }

    #[test]
    fn bitshift_refuses_what_it_cannot_model_and_never_panics() {
        // In range, on the engine's 32-bit width, vacated bits zero-filled.
        assert_eq!(rendered_stack(b"1 4 bitshift"), "16");
        assert_eq!(rendered_stack(b"-1 31 bitshift"), "-2147483648");
        assert_eq!(rendered_stack(b"256 -4 bitshift"), "16");

        // Out of range: refused, not masked. `1 32 bitshift` used to answer `1`, which
        // `getflagvalue` would have reported as a set flag.
        for source in [
            &b"1 32 bitshift"[..],
            b"1 64 bitshift",
            b"1 -1e300 bitshift",
        ] {
            let error = run(source).unwrap_err();
            assert!(
                error.contains("shift count in -31..=31"),
                "{} should have stopped, got {error}",
                String::from_utf8_lossy(source)
            );
        }
        // And the flag helper's own idiom still works across the whole modelled width.
        assert_eq!(rendered_stack(b"5 1 0 bitshift and"), "1");
        assert_eq!(rendered_stack(b"5 1 1 bitshift and"), "0");
    }

    /// A step ceiling alone bounds none of these. Each of the three used to run the process out of
    /// time, host stack, or memory with no error to show for it.
    ///
    /// **Read this before trusting a green suite here.** Only part of what this test covers is an
    /// assertion. Each case below asserts that the *guard fires* -- that the error arrives and says
    /// what it should -- which catches the guard being weakened or its threshold moved. What it
    /// cannot do is fail cleanly when a guard is deleted outright, because then the code under test
    /// never returns:
    ///
    /// - deleting `repeat`'s `charge_step` makes this test **hang forever** (measured: still
    ///   running at 20 s, `timeout` exit 124), not fail;
    /// - deleting the call-depth check makes it **abort the whole process** with
    ///   `fatal runtime error: stack overflow`, SIGABRT, taking every other test in the binary
    ///   with it;
    /// - deleting the allocation check makes it ask the allocator for 1.6 TB.
    ///
    /// Those are detections by the process dying, which is not the same thing as a failing
    /// assertion, and a reader who sees "229 passed" should not conclude these three bounds are
    /// asserted in the ordinary way. The allocation case shows the shape that *is* assertable and
    /// is worth copying: pin the threshold from both sides, so the constant and the comparison are
    /// covered by something that fails cleanly. The step and depth limits have no equivalent --
    /// there is no value of the step budget at which an uncharged `repeat` terminates.
    #[test]
    fn execution_is_bounded_in_steps_call_depth_and_allocation() {
        // `repeat` with an empty body executes no values, so it charged no steps. This asserts
        // the ceiling is reached and reported; with the charge deleted there is no error to
        // assert on, only a process that never returns.
        let document = GameScriptDocument::parse(b"100000000000 {} repeat").unwrap();
        let mut vm = GameScriptVm::new(500);
        let error = vm.execute_document(&document).unwrap_err();
        assert!(
            error.message.contains("500-step limit"),
            "{}",
            error.message
        );

        // Recursion consumes the host stack, which no step budget protects. Same caveat: this
        // asserts the refusal, and cannot observe the alternative, which is a SIGABRT.
        let document = GameScriptDocument::parse(b"/f {f} def f").unwrap();
        let mut vm = GameScriptVm::new(100_000_000);
        let error = vm.execute_document(&document).unwrap_err();
        assert!(
            error.message.contains("call depth exceeded"),
            "{}",
            error.message
        );

        // A script-controlled allocation is refused rather than attempted or silently shortened.
        for source in [&b"100000000000 array"[..], b"100000000000 string"] {
            let error = run(source).unwrap_err();
            assert!(
                error.contains("allocation limit"),
                "{} should have stopped, got {error}",
                String::from_utf8_lossy(source)
            );
        }
        // Pin the threshold from both sides. This is the only one of the three bounds whose
        // constant and comparison are covered by an assertion that fails cleanly: moving
        // MAXIMUM_ALLOCATION_LENGTH to 2_000_000 fails here (verified), whereas deleting the check
        // outright is "caught" only by the allocator.
        assert!(run(b"1000001 array").is_err());
        assert!(run(b"1000000 array").is_ok());
        assert!(run(b"1000001 string").is_err());
    }

    /// A procedure local, and a dictionary definition, shadow a language primitive. That is
    /// deliberate: in PostScript the primitives live at the bottom of the dictionary stack, so any
    /// nearer binding wins, and 15 of 3.02's script definitions rely on it by overriding an
    /// operator the engine also implements.
    #[test]
    fn a_nearer_binding_shadows_a_primitive() {
        let vm = run(b"/probe { add 0 get } /add [7] replace bind def probe").unwrap();
        assert_eq!(vm.operand_stack(), &[Value::Number(7.0)]);

        let vm = run(b"/add {99} def 1 2 add").unwrap();
        assert_eq!(
            vm.operand_stack(),
            &[Value::Number(1.0), Value::Number(2.0), Value::Number(99.0)]
        );
    }

    #[test]
    fn reset_stacks_keeps_definitions_and_drops_debris() {
        let document = GameScriptDocument::parse(b"/twice{2 mul}def 1 2 3 << >> begin").unwrap();
        let mut vm = GameScriptVm::new(10_000);
        vm.execute_document(&document).unwrap();
        vm.reset_stacks();

        assert!(vm.operand_stack().is_empty());
        let follow_on = GameScriptDocument::parse(b"5 twice end").unwrap();
        // `end` must now fail, because `reset_stacks` closed the dictionary the run left open.
        let error = vm.execute_document(&follow_on).unwrap_err();
        assert_eq!(error.message, "end cannot pop the base dictionary");
        assert_eq!(vm.operand_stack(), &[Value::Number(10.0)]);
    }

    /// The classification in `examples/gamescript_vocabulary.rs` calls a name a native host call
    /// precisely when the engine lists it and this VM does not implement it, so a stale
    /// `PRIMITIVE_NAMES` would silently move names between classes. Read the dispatch back out of
    /// this file and require the two to agree.
    #[test]
    fn primitive_names_match_the_dispatch() {
        let source = include_str!("gamescript_vm.rs");
        let (_, rest) = source
            .split_once("// PRIMITIVE-DISPATCH-BEGIN")
            .expect("the dispatch begin marker");
        let (dispatch, _) = rest
            .split_once("// PRIMITIVE-DISPATCH-END")
            .expect("the dispatch end marker");

        let mut dispatched = BTreeSet::new();
        for line in dispatch.lines() {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed.strip_prefix('"') else {
                continue;
            };
            let Some((name, tail)) = rest.split_once('"') else {
                continue;
            };
            if tail.trim_start().starts_with("=>") {
                dispatched.insert(name.to_owned());
            }
        }

        let listed: BTreeSet<String> = PRIMITIVE_NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        assert_eq!(
            listed, dispatched,
            "PRIMITIVE_NAMES and the operator dispatch have drifted apart"
        );
    }
}
