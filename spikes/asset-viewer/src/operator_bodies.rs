//! Static analysis of what each native GameScript operator's *body* does.
//!
//! The operator tables give 1,906 names, entry points and a static operand count. That is the
//! outside of the host API. This module reads the inside: for one entry point it recovers the
//! function's extent, every absolute data address the body touches, every function it calls, which
//! of those calls reach PE imports, and an operand count derived from a control-flow dataflow
//! rather than from a linear site count.
//!
//! Three things make the result usable rather than decorative.
//!
//! **The boundary is reported, not assumed.** Recursive descent from the entry point ends a run at
//! `ret`, at `int3` padding, at an unresolvable indirect jump, or at a `jmp` into a function the
//! program calls elsewhere — a tail call. Every way a walk can be incomplete is recorded on the
//! result, so a truncated body is visible as truncated instead of being reported as a small one.
//!
//! **Data references come from the decoder's operand model, not from formatted text.** A
//! displacement that lands in a mapped non-executable section is a global whether or not a register
//! is added to it, which is what makes `mov eax,[eax+5A7D90h]` — an indexed table read — count as a
//! reference to the table at `0x5A7D90`. Immediates are checked the same way, because the engine
//! reaches its singleton objects as `mov ecx,<address>` and no memory operand exists at all.
//!
//! **Operand counts follow paths.** Nearly every operator fetches its operands by calling one
//! shared helper, so a count of call sites overcounts any operator that fetches different numbers
//! on different branches. Instead each instruction is labelled with the number of operands consumed
//! before it; two predecessors that disagree mark the operator as branching, and only an operator
//! whose `ret`s all agree is reported as having an arity at all.
//!
//! The helpers themselves are **found, not hardcoded**. An operand-fetch helper is any small
//! function that many operators call and whose own body performs exactly one operand pop and no
//! push. The search finds more than one: the engine has a `thiscall` fetch that hands back the raw
//! `(tag, value)` pair and a `cdecl` fetch that coerces on the way out, and an analysis that knew
//! only the first reports a third of the table as nullary. If the engine were rebuilt the search
//! would find whatever it has instead.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use iced_x86::{
    Decoder, DecoderOptions, Instruction, InstructionInfoFactory, Mnemonic, OpAccess, OpKind,
    Register,
};

use crate::native_table::{NativeTableError, PeImage};
use crate::operator_arity::{
    Adjustment, adjustment, branch_target, is_stack_index_load, is_stack_index_store,
    lea_adjustment,
};

/// Upper bound on instructions decoded for one body. Reaching it is reported as truncation.
const MAXIMUM_INSTRUCTIONS: usize = 8192;

/// Largest jump table followed. Switch tables in this image are far smaller; the cap only stops a
/// misidentified table from walking the whole section.
const MAXIMUM_JUMP_TABLE: usize = 512;

/// How far a resolved jump-table target may sit from the entry point before it is disbelieved.
const JUMP_TABLE_SPAN: u32 = 0x8000;

/// Stand-in source for the object pointer the caller passed in `ecx`.
///
/// The engine is C++ with `thiscall` methods on singletons, so an operator's mutation is routinely
/// `mov ecx,<singleton>` followed by a call, and the store happens one frame down through `this`.
/// Marking that store lets the caller's own reference to the singleton carry the mutation, without
/// pretending to have followed a value across a call boundary.
const THIS_POINTER: u32 = u32::MAX;

/// How many call levels the reachability search crosses. Zero is the body itself.
///
/// The number is not tuned to an expected answer: the search records the *first* depth at which
/// each import kind appears, so the whole curve is reported and a reader can see where it
/// saturates. The classifier reads the curve at `CLASSIFY_DEPTH`.
pub const MAXIMUM_SEARCH_DEPTH: usize = 6;

/// The depth the behaviour classifier reads. Chosen from the reported curve: the engine reaches its
/// singleton subsystem objects through one method call and its file and archive work through that
/// object, so nothing below three is visible, and past four the archive reach saturates.
pub const CLASSIFY_DEPTH: usize = 1;

// ---------------------------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------------------------

/// One entry of the PE import directory, named by library and by symbol or ordinal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub library: String,
    pub symbol: String,
}

impl std::fmt::Display for Import {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}!{}", self.library, self.symbol)
    }
}

/// What an imported symbol says about the caller.
///
/// Coarse on purpose. The categories exist to be counted across 1,906 operators, and a taxonomy
/// finer than the evidence would invite reading intent into `CloseHandle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImportKind {
    FileIo,
    Archive,
    Memory,
    Thread,
    Graphics,
    Video,
    Audio,
    Network,
    Registry,
    WindowInput,
    Time,
    Other,
}

impl ImportKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::FileIo => "file-io",
            Self::Archive => "archive",
            Self::Memory => "memory",
            Self::Thread => "thread",
            Self::Graphics => "graphics",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Network => "network",
            Self::Registry => "registry",
            Self::WindowInput => "window-input",
            Self::Time => "time",
            Self::Other => "other",
        }
    }

    pub const ALL: [Self; 12] = [
        Self::FileIo,
        Self::Archive,
        Self::Memory,
        Self::Thread,
        Self::Graphics,
        Self::Video,
        Self::Audio,
        Self::Network,
        Self::Registry,
        Self::WindowInput,
        Self::Time,
        Self::Other,
    ];
}

/// Symbols that move bytes between the process and the filesystem.
const FILE_SYMBOLS: [&str; 24] = [
    "CreateFileA",
    "CreateFileW",
    "ReadFile",
    "WriteFile",
    "SetFilePointer",
    "SetEndOfFile",
    "FlushFileBuffers",
    "GetFileSize",
    "GetFileType",
    "GetFileTime",
    "CompareFileTime",
    "DeleteFileA",
    "FindFirstFileA",
    "FindNextFileA",
    "CreateDirectoryA",
    "GetFullPathNameA",
    "GetTempPathA",
    "GetTempFileNameA",
    "CreateFileMappingA",
    "MapViewOfFile",
    "UnmapViewOfFile",
    "GetCurrentDirectoryA",
    "SetCurrentDirectoryA",
    "GetDiskFreeSpaceA",
];

fn classify_import(import: &Import) -> ImportKind {
    let library = import.library.to_ascii_lowercase();
    let symbol = import.symbol.as_str();
    match library.as_str() {
        // Storm is Blizzard's MPQ library and is imported entirely by ordinal, so the library
        // itself is the evidence; no ordinal is named here that the binary does not name.
        "storm.dll" => return ImportKind::Archive,
        "ddraw.dll" | "gdi32.dll" => return ImportKind::Graphics,
        "smackw32.dll" => return ImportKind::Video,
        "dsound.dll" => return ImportKind::Audio,
        "dplayx.dll" => return ImportKind::Network,
        "advapi32.dll" => return ImportKind::Registry,
        "user32.dll" => return ImportKind::WindowInput,
        "winmm.dll" => {
            return if symbol.starts_with("time") {
                ImportKind::Time
            } else {
                ImportKind::Audio
            };
        }
        _ => {}
    }
    if FILE_SYMBOLS.contains(&symbol) {
        return ImportKind::FileIo;
    }
    if symbol.starts_with("Heap")
        || symbol.starts_with("Global")
        || symbol.starts_with("Local")
        || symbol.starts_with("Virtual")
    {
        return ImportKind::Memory;
    }
    if symbol.contains("Thread")
        || symbol.contains("CriticalSection")
        || symbol.contains("Event")
        || symbol.starts_with("Interlocked")
        || symbol.starts_with("Tls")
        || symbol == "Sleep"
        || symbol == "WaitForSingleObject"
    {
        return ImportKind::Thread;
    }
    if symbol == "GetTickCount"
        || symbol == "GetSystemTime"
        || symbol == "GetLocalTime"
        || symbol.starts_with("FileTimeTo")
    {
        return ImportKind::Time;
    }
    ImportKind::Other
}

/// Read the import directory and return each import thunk's address with the symbol behind it.
///
/// The thunk address is what a body actually references: the compiler emits `call [thunk]`, or
/// `call stub` where the stub is `jmp [thunk]`. Both are resolved through this map.
pub fn imports(image: &PeImage<'_>) -> Result<BTreeMap<u32, Import>, NativeTableError> {
    let bytes = image.bytes();
    let pe_offset = read_u32(bytes, 0x3c)
        .ok_or_else(|| NativeTableError::new("executable is too short for a DOS header"))?
        as usize;
    let directory = pe_offset + 24 + 96 + 8;
    let table_rva = read_u32(bytes, directory)
        .ok_or_else(|| NativeTableError::new("truncated data directory"))?;
    if table_rva == 0 {
        return Ok(BTreeMap::new());
    }

    let base = image.image_base();
    let mut resolved = BTreeMap::new();
    let mut descriptor = image
        .file_offset(base + table_rva)
        .ok_or_else(|| NativeTableError::new("import directory is not inside raw section data"))?;
    loop {
        let lookup_rva = read_u32(bytes, descriptor)
            .ok_or_else(|| NativeTableError::new("truncated import descriptor"))?;
        let name_rva = read_u32(bytes, descriptor + 12)
            .ok_or_else(|| NativeTableError::new("truncated import descriptor"))?;
        let thunk_rva = read_u32(bytes, descriptor + 16)
            .ok_or_else(|| NativeTableError::new("truncated import descriptor"))?;
        if name_rva == 0 && thunk_rva == 0 {
            break;
        }
        let library = read_c_string(image, base + name_rva).unwrap_or_else(|| "?".to_owned());
        // The lookup table survives binding; the address table may have been overwritten with
        // resolved addresses. Prefer the lookup table and fall back when it is absent.
        let names_rva = if lookup_rva == 0 { thunk_rva } else { lookup_rva };
        let mut index = 0_usize;
        while let Some(offset) = image.file_offset(base + names_rva + (index as u32) * 4) {
            let Some(entry) = read_u32(bytes, offset) else {
                break;
            };
            if entry == 0 {
                break;
            }
            let symbol = if entry & 0x8000_0000 != 0 {
                format!("#{}", entry & 0xffff)
            } else {
                read_c_string(image, base + entry + 2).unwrap_or_else(|| "?".to_owned())
            };
            resolved.insert(
                base + thunk_rva + (index as u32) * 4,
                Import {
                    library: library.clone(),
                    symbol,
                },
            );
            index += 1;
        }
        descriptor += 20;
    }
    Ok(resolved)
}

// ---------------------------------------------------------------------------------------------
// Program-wide index
// ---------------------------------------------------------------------------------------------

/// Facts about the whole image that every body walk needs.
pub struct ProgramIndex {
    /// Addresses the program calls from somewhere. Used to tell a tail call from an internal jump.
    call_targets: BTreeSet<u32>,
    imports: BTreeMap<u32, Import>,
    /// One-instruction stubs of the form `jmp [thunk]`, mapped to the import behind them.
    import_stubs: BTreeMap<u32, u32>,
    /// The shared operand-fetch helpers, found structurally. An empty set means the search failed,
    /// which makes every operand count body-local and is reported rather than papered over.
    pop_helpers: BTreeSet<u32>,
    /// The matching result-push helpers.
    push_helpers: BTreeSet<u32>,
}

impl ProgramIndex {
    /// Build the index. `operator_entry_points` seeds the helper search with the functions whose
    /// callees are worth ranking.
    pub fn build(
        image: &PeImage<'_>,
        operator_entry_points: &[u32],
    ) -> Result<Self, NativeTableError> {
        let imports = imports(image)?;
        let call_targets = scan_call_targets(image);
        let import_stubs = scan_import_stubs(image, &call_targets, &imports);
        let mut index = Self {
            call_targets,
            imports,
            import_stubs,
            pop_helpers: BTreeSet::new(),
            push_helpers: BTreeSet::new(),
        };
        let (pops, pushes) = find_operand_helpers(image, &index, operator_entry_points);
        index.pop_helpers = pops;
        index.push_helpers = pushes;
        Ok(index)
    }

    pub fn pop_helpers(&self) -> &BTreeSet<u32> {
        &self.pop_helpers
    }

    pub fn push_helpers(&self) -> &BTreeSet<u32> {
        &self.push_helpers
    }

    pub fn import_count(&self) -> usize {
        self.imports.len()
    }

    pub fn function_entry_count(&self) -> usize {
        self.call_targets.len()
    }

    /// The import a call target names, whether reached directly through the thunk or through a
    /// `jmp [thunk]` stub.
    pub fn import_for(&self, address: u32) -> Option<&Import> {
        if let Some(import) = self.imports.get(&address) {
            return Some(import);
        }
        self.import_stubs
            .get(&address)
            .and_then(|thunk| self.imports.get(thunk))
    }

    fn is_function_entry(&self, address: u32) -> bool {
        self.call_targets.contains(&address) || self.import_stubs.contains_key(&address)
    }
}

/// Every `call rel32` target in the image, found by a byte scan.
///
/// A byte scan rather than a linear disassembly because the section interleaves code with jump
/// tables and string data, so a linear decode desynchronises and invents instructions. The scan's
/// own false positives — an `0xe8` byte inside an immediate whose following dword happens to point
/// into the code section — are harmless here: the set is consulted only to decide whether a `jmp`
/// leaves the function, and a spurious member would have to land exactly on a jump target to
/// matter.
fn scan_call_targets(image: &PeImage<'_>) -> BTreeSet<u32> {
    let bytes = image.bytes();
    let mut targets = BTreeSet::new();
    // Walk every address the image maps, translating back to file offsets, so the scan covers code
    // wherever the linker put it rather than assuming one section.
    let base = image.image_base();
    for address in (base..base + 0x0200_0000).step_by(1) {
        if !image.is_code_address(address) {
            continue;
        }
        let Some(offset) = image.file_offset(address) else {
            continue;
        };
        if bytes.get(offset) != Some(&0xe8) {
            continue;
        }
        let Some(displacement) = read_u32(bytes, offset + 1) else {
            continue;
        };
        let target = address
            .wrapping_add(5)
            .wrapping_add(displacement);
        if image.is_code_address(target) {
            targets.insert(target);
        }
    }
    targets
}

/// Find the `jmp [thunk]` stubs the linker emits in front of imports.
fn scan_import_stubs(
    image: &PeImage<'_>,
    call_targets: &BTreeSet<u32>,
    imports: &BTreeMap<u32, Import>,
) -> BTreeMap<u32, u32> {
    let mut stubs = BTreeMap::new();
    for target in call_targets {
        let Some(offset) = image.file_offset(*target) else {
            continue;
        };
        let Some(window) = image.bytes().get(offset..offset + 6) else {
            continue;
        };
        // ff 25 <abs32> is `jmp dword [abs32]`.
        if window[0] != 0xff || window[1] != 0x25 {
            continue;
        }
        let thunk = u32::from_le_bytes([window[2], window[3], window[4], window[5]]);
        if imports.contains_key(&thunk) {
            stubs.insert(*target, thunk);
        }
    }
    stubs
}

/// Identify the shared operand helpers by their shape and their popularity.
///
/// A helper is a small function that pops exactly one operand and pushes nothing (or the mirror
/// image), called by many *distinct operators*. Counting distinct operators rather than call sites
/// stops one operator with a loop from electing its own private helper, and taking a set rather
/// than a single winner is what keeps the second fetch helper from being missed — the engine has
/// two, and they do not share a calling convention.
const HELPER_MINIMUM_CALLERS: usize = 16;

/// Helpers are leaf-ish wrappers. A large function that happens to pop once is doing something
/// else, and counting a call to it as exactly one operand would be a guess.
const HELPER_MAXIMUM_INSTRUCTIONS: usize = 128;

fn find_operand_helpers(
    image: &PeImage<'_>,
    index: &ProgramIndex,
    operator_entry_points: &[u32],
) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut callers: HashMap<u32, usize> = HashMap::new();
    for entry in operator_entry_points {
        let Ok(body) = walk(image, index, *entry, None) else {
            continue;
        };
        for callee in body.calls.iter().chain(body.tail_calls.iter()) {
            *callers.entry(*callee).or_default() += 1;
        }
    }
    let mut pops = BTreeSet::new();
    let mut pushes = BTreeSet::new();
    for (address, count) in callers {
        if count < HELPER_MINIMUM_CALLERS {
            continue;
        }
        let Ok(body) = walk(image, index, address, None) else {
            continue;
        };
        if !body.boundary_complete() || body.instructions > HELPER_MAXIMUM_INSTRUCTIONS {
            continue;
        }
        if body.inline_pops == 1 && body.inline_pushes == 0 {
            pops.insert(address);
        } else if body.inline_pushes == 1 && body.inline_pops == 0 {
            pushes.insert(address);
        }
    }
    (pops, pushes)
}

// ---------------------------------------------------------------------------------------------
// One body
// ---------------------------------------------------------------------------------------------

/// How a value stored in a data global was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GlobalAccess {
    /// The body loaded from the address.
    Read,
    /// The body stored to the address.
    Write,
    /// The address itself was materialised — `mov ecx,<address>`, `push <address>`. The engine
    /// reaches its singletons this way, so this is a reference even though no memory operand
    /// exists.
    Taken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalRef {
    pub address: u32,
    pub access: GlobalAccess,
    /// Whether a register took part in the effective address, which is what distinguishes an
    /// element read `[eax+table]` from a scalar read `[variable]`.
    pub indexed: bool,
}

/// Everything the walk could and could not establish about one function body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyAnalysis {
    pub entry_point: u32,
    pub instructions: usize,
    /// Highest address the walk decoded, exclusive. With the entry point this is the body's span.
    pub span_end: u32,
    pub globals: Vec<GlobalRef>,
    pub calls: BTreeSet<u32>,
    /// A `jmp` that left the function into a function the program calls elsewhere.
    pub tail_calls: BTreeSet<u32>,
    pub direct_imports: BTreeSet<String>,
    /// Globals whose value was loaded into a register and then stored *through*.
    ///
    /// The engine keeps its large state — the map array, the unit roster — in heap allocations
    /// reached through a pointer in `.data`, so a mutation is `mov eax,[global]` followed by
    /// `mov [eax+n],value` and never touches an absolute address on the writing side. Without this
    /// the whole map-editing half of the API classifies as read-only, which is how the control
    /// operator `setterrain` first came back as `reads-state`.
    pub writes_through_pointer: BTreeSet<u32>,
    /// Whether the body stores through the object pointer its caller passed in `ecx`. On its own
    /// this says nothing about engine state — it is the caller's `mov ecx,<global>` that says which
    /// state — so it is only ever combined with a caller that named one.
    pub writes_through_this: bool,
    /// Addresses of NUL-terminated printable strings the body references. Addresses only; the text
    /// is game content and stays out of the repository.
    pub string_refs: BTreeSet<u32>,
    /// Operand pops performed inline, by the `[ctx+0x54]` increment idiom.
    pub inline_pops: usize,
    /// Operand pushes performed inline, by the matching decrement.
    pub inline_pushes: usize,
    /// Call sites that reach a shared operand-fetch helper.
    pub helper_pops: usize,
    /// Call sites that reach a shared result-push helper.
    pub helper_pushes: usize,
    /// Operands consumed on every path that returns, when all such paths agree. `None` means the
    /// paths disagree, which is what a variadic or branch-dependent operator looks like.
    pub arity: Option<usize>,
    /// Distinct operand counts observed at `ret`, in ascending order. Always populated, so a
    /// disagreement can be inspected rather than merely noted.
    pub arity_candidates: BTreeSet<usize>,
    /// Whether the operand count grows around a loop, so no finite count describes the body. This
    /// is what a genuinely variadic operator looks like from here.
    pub operand_count_unbounded: bool,
    /// Bytes of arguments the function releases on return — `ret 4` reports 4. Part of how the
    /// operand helper is recognised.
    pub returns_arguments: u32,
    pub indirect_calls: usize,
    pub unresolved_indirect_jumps: usize,
    pub resolved_jump_tables: usize,
    pub invalid_instructions: usize,
    pub truncated: bool,
    /// Whether any floating-point instruction was decoded.
    pub floating_point: bool,
    pub returns: usize,
}

impl BodyAnalysis {
    /// The operand count on the path that does the operator's work.
    ///
    /// Every operand fetch in this engine is "fetch or fail": the helper checks for underflow, and
    /// on underflow it raises a script error and the operator returns having consumed nothing. That
    /// error path is a real path through the body, which is why `arity` — strict agreement between
    /// all returning paths — is absent for half the table. The count a caller cares about is the
    /// one on the path that did not abort, and that is the largest.
    ///
    /// **Inferred**, not observed: the observation is the set of counts at the `ret`s; reading the
    /// largest as "the successful path" is an interpretation of the fetch idiom. It is the reading
    /// the idiom supports, and `arity_candidates` is kept so a reader can take the other one.
    pub fn nominal_arity(&self) -> Option<usize> {
        if self.operand_count_unbounded {
            return None;
        }
        self.arity_candidates.iter().copied().max()
    }

    /// Whether the walk reached the end of every path it started.
    ///
    /// This is the boundary claim and nothing else. A complete walk can still describe an operator
    /// whose real work is three calls away.
    pub fn boundary_complete(&self) -> bool {
        !self.truncated
            && self.invalid_instructions == 0
            && self.unresolved_indirect_jumps == 0
            && self.returns > 0
    }

    pub fn boundary_failure(&self) -> Option<&'static str> {
        if self.truncated {
            Some("truncated")
        } else if self.invalid_instructions > 0 {
            Some("invalid-instruction")
        } else if self.unresolved_indirect_jumps > 0 {
            Some("unresolved-indirect-jump")
        } else if self.returns == 0 {
            Some("no-return")
        } else {
            None
        }
    }

    /// Whether the body stores to engine state, directly or through a pointer it read out of a
    /// global.
    pub fn writes_globals(&self) -> bool {
        !self.writes_through_pointer.is_empty()
            || self
                .globals
                .iter()
                .any(|global| global.access == GlobalAccess::Write)
    }

    pub fn reads_globals(&self) -> bool {
        self.globals
            .iter()
            .any(|global| global.access != GlobalAccess::Write)
    }
}

/// What one instruction contributes to the walk.
struct Step {
    successors: Vec<u32>,
    operands_consumed: usize,
    returns: bool,
}

/// Walk one function body.
///
/// `pop_counts` supplies, for each callee already measured, how many operands it consumes, so a
/// body that fetches its operands inside a helper is not reported as nullary. Passing `None` walks
/// the body alone, which is what the helper search itself needs.
fn walk(
    image: &PeImage<'_>,
    index: &ProgramIndex,
    entry_point: u32,
    pop_counts: Option<&HashMap<u32, usize>>,
) -> Result<BodyAnalysis, NativeTableError> {
    if !image.is_code_address(entry_point) {
        return Err(NativeTableError::new(format!(
            "entry point {entry_point:#010x} is not inside a code section"
        )));
    }

    let mut analysis = BodyAnalysis {
        entry_point,
        instructions: 0,
        span_end: entry_point,
        globals: Vec::new(),
        calls: BTreeSet::new(),
        tail_calls: BTreeSet::new(),
        direct_imports: BTreeSet::new(),
        writes_through_pointer: BTreeSet::new(),
        writes_through_this: false,
        string_refs: BTreeSet::new(),
        inline_pops: 0,
        inline_pushes: 0,
        helper_pops: 0,
        helper_pushes: 0,
        arity: None,
        arity_candidates: BTreeSet::new(),
        operand_count_unbounded: false,
        returns_arguments: 0,
        indirect_calls: 0,
        unresolved_indirect_jumps: 0,
        resolved_jump_tables: 0,
        invalid_instructions: 0,
        truncated: false,
        floating_point: false,
        returns: 0,
    };

    let mut steps: BTreeMap<u32, Step> = BTreeMap::new();
    let mut globals: BTreeSet<(u32, GlobalAccess, bool)> = BTreeSet::new();
    let mut starts: VecDeque<u32> = VecDeque::from([entry_point]);
    let mut started: BTreeSet<u32> = BTreeSet::new();
    // The pointer taint each run begins with. Carried along control-flow edges, first writer
    // winning, so a `this` moved into a callee-saved register in the prologue is still recognised
    // in the block that stores through it. First-writer-wins is an approximation and errs towards
    // forgetting, which understates mutation rather than inventing it.
    let mut entry_pointers: BTreeMap<u32, BTreeMap<Register, u32>> =
        BTreeMap::from([(entry_point, BTreeMap::from([(Register::ECX, THIS_POINTER)]))]);
    let mut info_factory = InstructionInfoFactory::new();

    while let Some(start) = starts.pop_front() {
        if !started.insert(start) {
            continue;
        }
        let Some(offset) = image.file_offset(start) else {
            continue;
        };
        let Some(bytes) = image.bytes().get(offset..) else {
            continue;
        };
        let mut decoder = Decoder::with_ip(32, bytes, u64::from(start), DecoderOptions::NONE);
        // Register tracking is per run: the idiom that adjusts the operand-stack index lives
        // inside a basic block, so carrying it across a branch would invent pops.
        let mut tracked: BTreeMap<Register, Adjustment> = BTreeMap::new();
        // Registers currently holding a value read out of a data global, with the global they came
        // from. Per run for the same reason `tracked` is: a value does not survive a branch here.
        let mut pointers: BTreeMap<Register, u32> =
            entry_pointers.get(&start).cloned().unwrap_or_default();

        while decoder.can_decode() {
            if analysis.instructions >= MAXIMUM_INSTRUCTIONS {
                analysis.truncated = true;
                break;
            }
            let instruction = decoder.decode();
            let address = instruction.ip() as u32;
            if instruction.is_invalid() {
                analysis.invalid_instructions += 1;
                break;
            }
            // Falling into code another run already decoded: the edge is recorded, the work is not
            // repeated.
            if steps.contains_key(&address) {
                break;
            }
            // `int3` is the linker's inter-function padding; reaching it means the body ended.
            if instruction.mnemonic() == Mnemonic::Int3 {
                break;
            }
            analysis.instructions += 1;
            analysis.span_end = analysis.span_end.max(address + instruction.len() as u32);

            let mut operands_consumed = 0_usize;

            // -- operand-stack traffic ------------------------------------------------------
            if is_stack_index_store(&instruction) {
                match tracked.get(&instruction.op1_register()) {
                    Some(Adjustment::Increment) => {
                        analysis.inline_pops += 1;
                        operands_consumed += 1;
                    }
                    Some(Adjustment::Decrement) => analysis.inline_pushes += 1,
                    Some(Adjustment::Loaded) | None => {}
                }
            } else if is_stack_index_load(&instruction) {
                tracked.insert(instruction.op0_register(), Adjustment::Loaded);
            } else if let Some((destination, adjusted)) = lea_adjustment(&instruction, &tracked) {
                tracked.insert(destination, adjusted);
            } else if let Some(adjusted) = adjustment(&instruction) {
                let register = instruction.op0_register();
                if matches!(tracked.get(&register), Some(Adjustment::Loaded)) {
                    tracked.insert(register, adjusted);
                } else {
                    tracked.remove(&register);
                }
            } else {
                let info = info_factory.info(&instruction);
                for used in info.used_registers() {
                    if matches!(
                        used.access(),
                        OpAccess::Write
                            | OpAccess::CondWrite
                            | OpAccess::ReadWrite
                            | OpAccess::ReadCondWrite
                    ) {
                        tracked.remove(&used.register());
                    }
                }
            }

            // -- data references ------------------------------------------------------------
            let memory: Vec<iced_x86::UsedMemory> =
                info_factory.info(&instruction).used_memory().to_vec();
            for used in &memory {
                let displacement = used.displacement() as u32;
                if !image.is_data_address(displacement) {
                    continue;
                }
                // The import address table is code by intent, not engine state; it is read here as
                // a call target and reported as an import rather than as a global.
                if index.imports.contains_key(&displacement) {
                    continue;
                }
                let access = match used.access() {
                    OpAccess::Write | OpAccess::CondWrite => GlobalAccess::Write,
                    OpAccess::ReadWrite | OpAccess::ReadCondWrite => GlobalAccess::Write,
                    _ => GlobalAccess::Read,
                };
                let indexed = used.base() != Register::None || used.index() != Register::None;
                globals.insert((displacement, access, indexed));
                if access == GlobalAccess::Write {
                    globals.insert((displacement, GlobalAccess::Read, indexed));
                }
            }
            update_pointer_taint(
                image,
                &instruction,
                &memory,
                &mut pointers,
                &mut analysis.writes_through_pointer,
            );

            for operand in 0..instruction.op_count() {
                if instruction.op_kind(operand) != OpKind::Immediate32 {
                    continue;
                }
                let value = instruction.immediate32();
                if image.is_data_address(value) && !index.imports.contains_key(&value) {
                    globals.insert((value, GlobalAccess::Taken, false));
                    if let Some(text) = read_c_string(image, value)
                        && is_printable_string(&text)
                    {
                        analysis.string_refs.insert(value);
                    }
                }
            }
            // x87 mnemonics all begin with `F`; the debug spelling is the only name iced-x86
            // exposes without the formatter feature, and it is stable across the crate's API.
            if format!("{:?}", instruction.mnemonic()).starts_with('F') {
                analysis.floating_point = true;
            }

            // -- calls ----------------------------------------------------------------------
            if instruction.mnemonic() == Mnemonic::Call {
                match branch_target(&instruction) {
                    Some(target) => {
                        if let Some(import) = index.import_for(target) {
                            analysis.direct_imports.insert(import.to_string());
                        } else if index.pop_helpers.contains(&target) {
                            analysis.helper_pops += 1;
                            operands_consumed += 1;
                        } else if index.push_helpers.contains(&target) {
                            analysis.helper_pushes += 1;
                        } else if let Some(counts) = pop_counts {
                            operands_consumed += counts.get(&target).copied().unwrap_or(0);
                        }
                        analysis.calls.insert(target);
                    }
                    None => {
                        // `call [thunk]` against the import table is a resolved import, not an
                        // unknown indirection.
                        let absolute = instruction.memory_displacement64() as u32;
                        if instruction.op0_kind() == OpKind::Memory
                            && instruction.memory_base() == Register::None
                            && instruction.memory_index() == Register::None
                            && let Some(import) = index.imports.get(&absolute)
                        {
                            analysis.direct_imports.insert(import.to_string());
                        } else {
                            analysis.indirect_calls += 1;
                        }
                    }
                }
            }

            // -- control flow ---------------------------------------------------------------
            let next = address + instruction.len() as u32;
            let step = match instruction.mnemonic() {
                Mnemonic::Ret => {
                    analysis.returns += 1;
                    if instruction.op_count() > 0 {
                        analysis.returns_arguments = instruction.immediate32();
                    }
                    Step {
                        successors: Vec::new(),
                        operands_consumed,
                        returns: true,
                    }
                }
                Mnemonic::Jmp => match branch_target(&instruction) {
                    Some(target) => {
                        // A backward jump into code this walk has already decoded is a loop; a
                        // forward jump to something the program calls from elsewhere is a tail
                        // call. Treating the whole walked span as "inside" also swallowed the
                        // instruction immediately after the jump, which is where a tail call to an
                        // adjacent function lands.
                        let loops_back = target >= entry_point && target <= address;
                        let inside = loops_back || !index.is_function_entry(target);
                        if inside {
                            entry_pointers.entry(target).or_insert_with(|| pointers.clone());
                            starts.push_back(target);
                            Step {
                                successors: vec![target],
                                operands_consumed,
                                returns: false,
                            }
                        } else {
                            // A tail call leaves the function. The callee's own operand traffic is
                            // folded in the same way a `call` would be.
                            if let Some(import) = index.import_for(target) {
                                analysis.direct_imports.insert(import.to_string());
                            } else if index.pop_helpers.contains(&target) {
                                analysis.helper_pops += 1;
                                operands_consumed += 1;
                            } else if index.push_helpers.contains(&target) {
                                analysis.helper_pushes += 1;
                            } else if let Some(counts) = pop_counts {
                                operands_consumed += counts.get(&target).copied().unwrap_or(0);
                            }
                            analysis.tail_calls.insert(target);
                            analysis.returns += 1;
                            Step {
                                successors: Vec::new(),
                                operands_consumed,
                                returns: true,
                            }
                        }
                    }
                    None => {
                        match resolve_jump_table(image, &instruction, entry_point) {
                            Some(targets) => {
                                analysis.resolved_jump_tables += 1;
                                for target in &targets {
                                    starts.push_back(*target);
                                }
                                Step {
                                    successors: targets,
                                    operands_consumed,
                                    returns: false,
                                }
                            }
                            None => {
                                analysis.unresolved_indirect_jumps += 1;
                                Step {
                                    successors: Vec::new(),
                                    operands_consumed,
                                    returns: false,
                                }
                            }
                        }
                    }
                },
                _ if instruction.is_jcc_short_or_near() => {
                    let mut successors = vec![next];
                    if let Some(target) = branch_target(&instruction) {
                        entry_pointers.entry(target).or_insert_with(|| pointers.clone());
                        starts.push_back(target);
                        successors.push(target);
                    }
                    Step {
                        successors,
                        operands_consumed,
                        returns: false,
                    }
                }
                _ => Step {
                    successors: vec![next],
                    operands_consumed,
                    returns: false,
                },
            };
            let terminates = step.successors.is_empty()
                || step.successors.first() != Some(&next)
                || instruction.mnemonic() == Mnemonic::Jmp;
            steps.insert(address, step);
            if terminates {
                break;
            }
        }
    }

    analysis.writes_through_this = analysis.writes_through_pointer.remove(&THIS_POINTER);
    analysis.globals = globals
        .into_iter()
        .map(|(address, access, indexed)| GlobalRef {
            address,
            access,
            indexed,
        })
        .collect();

    let (arity, candidates, unbounded) = operand_dataflow(&steps, entry_point);
    analysis.arity = arity;
    analysis.arity_candidates = candidates;
    analysis.operand_count_unbounded = unbounded;
    Ok(analysis)
}

/// Track which registers hold a value read out of a data global, and record stores made through
/// one.
///
/// A deliberately shallow taint: it follows `mov`/`lea` chains within a run and gives up on
/// anything else. That direction of error is the safe one — a missed chain understates mutation —
/// and the alternative, a full value analysis, would put far more inference behind a claim the
/// table presents as observed.
fn update_pointer_taint(
    image: &PeImage<'_>,
    instruction: &Instruction,
    memory: &[iced_x86::UsedMemory],
    pointers: &mut BTreeMap<Register, u32>,
    writes: &mut BTreeSet<u32>,
) {
    for used in memory {
        let writing = matches!(
            used.access(),
            OpAccess::Write | OpAccess::CondWrite | OpAccess::ReadWrite | OpAccess::ReadCondWrite
        );
        if !writing {
            continue;
        }
        for register in [used.base(), used.index()] {
            if let Some(source) = pointers.get(&register) {
                writes.insert(*source);
            }
        }
    }

    let propagating = matches!(instruction.mnemonic(), Mnemonic::Mov | Mnemonic::Lea);
    if propagating && instruction.op0_kind() == OpKind::Register {
        let destination = instruction.op0_register();
        let source = match instruction.op1_kind() {
            OpKind::Register => pointers.get(&instruction.op1_register()).copied(),
            OpKind::Memory => {
                let displacement = instruction.memory_displacement64() as u32;
                if instruction.memory_base() == Register::None
                    && instruction.memory_index() == Register::None
                    && image.is_data_address(displacement)
                {
                    Some(displacement)
                } else {
                    pointers.get(&instruction.memory_base()).copied()
                }
            }
            _ => None,
        };
        match source {
            Some(address) => {
                pointers.insert(destination, address);
            }
            None => {
                pointers.remove(&destination);
            }
        }
        return;
    }

    // Anything else that writes a register invalidates whatever it held.
    if instruction.op0_kind() == OpKind::Register {
        pointers.remove(&instruction.op0_register());
    }
}

/// Label each instruction with the operands consumed before it, and read the answer off the `ret`s.
///
/// Every *distinct* count that can reach an instruction is propagated, not just the first one. The
/// first version of this kept one label per address, which made the answer depend on the order the
/// queue happened to visit blocks in: the underflow path of an operator that pops four operands
/// reaches the `ret` first, so the operator was reported as consuming nothing. Coverage that
/// depends on visit order is not coverage.
///
/// A loop that pops on each iteration has no finite label set. Rather than let one grow without
/// bound the count of states per instruction is capped, and hitting the cap is recorded as
/// `operand_count_unbounded` — which is the honest description of an operator that consumes a
/// script-determined number of operands.
fn operand_dataflow(
    steps: &BTreeMap<u32, Step>,
    entry_point: u32,
) -> (Option<usize>, BTreeSet<usize>, bool) {
    /// Distinct operand counts tracked per instruction before the body is called unbounded.
    const MAXIMUM_STATES: usize = 16;

    let mut states: HashMap<u32, BTreeSet<usize>> = HashMap::new();
    let mut unbounded = false;
    let mut queue = VecDeque::from([(entry_point, 0_usize)]);
    let mut at_returns: BTreeSet<usize> = BTreeSet::new();

    while let Some((address, before)) = queue.pop_front() {
        let seen = states.entry(address).or_default();
        if !seen.insert(before) {
            continue;
        }
        if seen.len() > MAXIMUM_STATES {
            unbounded = true;
            continue;
        }
        let Some(step) = steps.get(&address) else {
            continue;
        };
        let after = before + step.operands_consumed;
        if step.returns {
            at_returns.insert(after);
        }
        for successor in &step.successors {
            queue.push_back((*successor, after));
        }
    }

    let arity = (at_returns.len() == 1 && !unbounded)
        .then(|| at_returns.iter().next().copied())
        .flatten();
    (arity, at_returns, unbounded)
}

/// Follow `jmp [index*4 + table]`, the shape the compiler emits for a dense `switch`.
fn resolve_jump_table(
    image: &PeImage<'_>,
    instruction: &Instruction,
    entry_point: u32,
) -> Option<Vec<u32>> {
    if instruction.op0_kind() != OpKind::Memory
        || instruction.memory_base() != Register::None
        || instruction.memory_index() == Register::None
        || instruction.memory_index_scale() != 4
    {
        return None;
    }
    let table = instruction.memory_displacement64() as u32;
    let mut targets = Vec::new();
    for slot in 0..MAXIMUM_JUMP_TABLE {
        let Some(offset) = image.file_offset(table + (slot as u32) * 4) else {
            break;
        };
        let Some(target) = read_u32(image.bytes(), offset) else {
            break;
        };
        if !image.is_code_address(target) || target.abs_diff(entry_point) > JUMP_TABLE_SPAN {
            break;
        }
        targets.push(target);
    }
    (!targets.is_empty()).then_some(targets)
}

// ---------------------------------------------------------------------------------------------
// Whole-table analysis
// ---------------------------------------------------------------------------------------------

/// What the body evidence says an operator does.
///
/// The ladder is ordered by how much the evidence commits to: an import naming the filesystem says
/// more than a store to an unnamed global, which says more than a load. `Unknown` is the answer
/// whenever the walk did not finish, and is deliberately common.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Behaviour {
    Unknown,
    StackManipulation,
    Arithmetic,
    ReadsEngineState,
    MutatesEngineState,
    FileOrResourceIo,
    Rendering,
    Audio,
    Network,
}

impl Behaviour {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::StackManipulation => "stack",
            Self::Arithmetic => "arithmetic",
            Self::ReadsEngineState => "reads-state",
            Self::MutatesEngineState => "mutates-state",
            Self::FileOrResourceIo => "file-or-resource-io",
            Self::Rendering => "rendering",
            Self::Audio => "audio",
            Self::Network => "network",
        }
    }
}

/// One operator's row in the classification table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorReport {
    pub name: String,
    pub entry_point: u32,
    pub body: BodyAnalysis,
    /// Operand count from `operator_arity`, the measurement already in the repository.
    pub declared_arity: Option<usize>,
    /// At what call depth each import kind and the first state write become reachable.
    pub reach: Reach,
    pub behaviour: Behaviour,
    /// The next operator entry point above this one, when there is one. A body that runs past it
    /// is evidence the boundary heuristic failed.
    pub next_entry_point: Option<u32>,
}

impl OperatorReport {
    /// Whether the walk ran past the next operator entry point.
    ///
    /// Operators are not laid out contiguously and some tail call far away, so this overruns
    /// legitimately sometimes; it is a rate to report, not a per-row verdict.
    pub fn overruns_next_entry_point(&self) -> bool {
        self.next_entry_point
            .is_some_and(|next| next > self.entry_point && self.body.span_end > next)
    }

    /// Whether the body's own operand count contradicts the count already in the repository.
    ///
    /// Compares the nominal count — the successful path — against `operator_arity`'s site count,
    /// because those are the two numbers that claim to be the same thing.
    pub fn arity_disagreement(&self) -> Option<(usize, usize)> {
        match (self.body.nominal_arity(), self.declared_arity) {
            (Some(measured), Some(declared)) if measured != declared => Some((measured, declared)),
            _ => None,
        }
    }
}

/// Analyse every operator in the table.
pub struct Analysis {
    pub reports: Vec<OperatorReport>,
    pub index: ProgramIndex,
}

/// Run the whole analysis over a set of named entry points.
pub fn analyse(
    image: &PeImage<'_>,
    operators: &[(String, u32)],
) -> Result<Analysis, NativeTableError> {
    let entry_points: Vec<u32> = operators.iter().map(|(_, entry)| *entry).collect();
    let index = ProgramIndex::build(image, &entry_points)?;

    // Pass one: the operand counts this yields are what pass two folds into its callers.
    // Every function the image calls from anywhere, plus the operators themselves, walked
    // body-local. Walking the whole set rather than a frontier means the reachability search below
    // can be asked for any depth without the answer depending on how far pass one happened to go.
    let mut bodies: HashMap<u32, BodyAnalysis> = HashMap::new();
    for entry in entry_points
        .iter()
        .copied()
        .chain(index.call_targets.iter().copied())
    {
        if bodies.contains_key(&entry) {
            continue;
        }
        if let Ok(body) = walk(image, &index, entry, None) {
            bodies.insert(entry, body);
        }
    }

    // A callee contributes the operands it consumes on its successful path, for the same reason
    // `nominal_arity` exists: its underflow path consumed nothing and did not do the work.
    let mut pop_counts: HashMap<u32, usize> = HashMap::new();
    for (entry, body) in &bodies {
        if let Some(arity) = body.nominal_arity() {
            pop_counts.insert(*entry, arity);
        }
    }

    let mut sorted_entries: Vec<u32> = entry_points.clone();
    sorted_entries.sort_unstable();
    sorted_entries.dedup();

    let helpers: BTreeSet<u32> = index
        .pop_helpers
        .union(&index.push_helpers)
        .copied()
        .collect();
    let mut reports = Vec::with_capacity(operators.len());
    for (name, entry_point) in operators {
        let body = walk(image, &index, *entry_point, Some(&pop_counts))?;
        let declared_arity = crate::operator_arity::stack_effect(image, *entry_point)
            .ok()
            .map(|effect| effect.pops);
        let reach = reachability(&bodies, &index, &body, *entry_point);
        let next_entry_point = sorted_entries
            .iter()
            .copied()
            .find(|candidate| *candidate > *entry_point);
        let behaviour = classify(&body, &reach, CLASSIFY_DEPTH, &helpers);
        reports.push(OperatorReport {
            name: name.clone(),
            entry_point: *entry_point,
            body,
            declared_arity,
            reach,
            behaviour,
            next_entry_point,
        });
    }
    Ok(Analysis { reports, index })
}

/// At what call depth each import kind, and the first write to engine state, becomes reachable.
///
/// Reporting the depth rather than a yes/no is the difference between a signal and a tautology:
/// almost every operator reaches the allocator eventually, and a table that said so would be
/// describing the call graph rather than the operator.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reach {
    pub import_depth: BTreeMap<ImportKind, usize>,
    /// Shallowest call depth at which some function stores to a data global, or through a pointer
    /// read out of one.
    pub write_depth: Option<usize>,
    /// Whether a method called directly on a singleton this operator named stores through `this`.
    /// Not evidence of engine mutation on its own; see `reachability`.
    pub calls_mutating_method: bool,
}

impl Reach {
    pub fn reaches(&self, kind: ImportKind, depth: usize) -> bool {
        self.import_depth
            .get(&kind)
            .is_some_and(|first| *first <= depth)
    }

    pub fn mutates(&self, depth: usize) -> bool {
        self.write_depth.is_some_and(|first| first <= depth)
    }

    pub fn kinds_within(&self, depth: usize) -> BTreeSet<ImportKind> {
        self.import_depth
            .iter()
            .filter(|(_, first)| **first <= depth)
            .map(|(kind, _)| *kind)
            .collect()
    }
}

fn reachability(
    bodies: &HashMap<u32, BodyAnalysis>,
    index: &ProgramIndex,
    body: &BodyAnalysis,
    entry_point: u32,
) -> Reach {
    let mut reach = Reach::default();
    let note = |reach: &mut Reach, body: &BodyAnalysis, depth: usize| {
        for name in &body.direct_imports {
            if let Some(import) = parse_import(name) {
                reach
                    .import_depth
                    .entry(classify_import(&import))
                    .or_insert(depth);
            }
        }
        if body.writes_globals() {
            reach.write_depth.get_or_insert(depth);
        }
    };
    note(&mut reach, body, 0);
    let names_a_singleton = body
        .globals
        .iter()
        .any(|global| global.access == GlobalAccess::Taken);

    let mut seen = BTreeSet::from([entry_point]);
    let mut frontier: Vec<u32> = body
        .calls
        .iter()
        .chain(body.tail_calls.iter())
        .copied()
        .collect();
    for depth in 1..=MAXIMUM_SEARCH_DEPTH {
        let mut next = Vec::new();
        for address in frontier {
            if !seen.insert(address) {
                continue;
            }
            if let Some(import) = index.import_for(address) {
                reach
                    .import_depth
                    .entry(classify_import(import))
                    .or_insert(depth);
                continue;
            }
            let Some(callee) = bodies.get(&address) else {
                continue;
            };
            note(&mut reach, callee, depth);
            // A store through `this` one call down is recorded, but deliberately **not** folded
            // into `write_depth`. Measured across the table it is true of 55% of operators — the
            // engine's getters cache into their own object — so promoting it to "mutates engine
            // state" would relabel most of the API on evidence that does not distinguish it. It is
            // carried as its own flag instead, and the limitation is stated rather than hidden:
            // `setterrain` is known to mutate the map and this analysis cannot show that it does.
            if depth == 1 && callee.writes_through_this && names_a_singleton {
                reach.calls_mutating_method = true;
            }
            next.extend(callee.calls.iter().copied());
            next.extend(callee.tail_calls.iter().copied());
        }
        frontier = next;
    }
    reach
}

fn parse_import(text: &str) -> Option<Import> {
    let (library, symbol) = text.split_once('!')?;
    Some(Import {
        library: library.to_owned(),
        symbol: symbol.to_owned(),
    })
}

fn classify(
    body: &BodyAnalysis,
    reach: &Reach,
    depth: usize,
    helpers: &BTreeSet<u32>,
) -> Behaviour {
    if !body.boundary_complete() {
        return Behaviour::Unknown;
    }
    // A mutation the body performs itself outranks an import several calls away: the operator's own
    // stores are what it does, and the archive under them is how the engine happens to be built.
    if body.writes_globals() {
        return Behaviour::MutatesEngineState;
    }
    if reach.reaches(ImportKind::FileIo, depth) || reach.reaches(ImportKind::Archive, depth) {
        return Behaviour::FileOrResourceIo;
    }
    if reach.reaches(ImportKind::Graphics, depth) || reach.reaches(ImportKind::Video, depth) {
        return Behaviour::Rendering;
    }
    if reach.reaches(ImportKind::Audio, depth) {
        return Behaviour::Audio;
    }
    if reach.reaches(ImportKind::Network, depth) {
        return Behaviour::Network;
    }
    if reach.mutates(depth) {
        return Behaviour::MutatesEngineState;
    }
    if body.reads_globals() {
        return Behaviour::ReadsEngineState;
    }
    // Calls to the shared operand helpers are how an operator reaches its own arguments; counting
    // them as "this body calls something" would put every stack primitive in `unknown`, which is
    // where `dup` sat until this was measured.
    let engine_calls = body
        .calls
        .iter()
        .chain(body.tail_calls.iter())
        .filter(|target| !helpers.contains(target))
        .count();
    if body.globals.is_empty() && engine_calls == 0 {
        return if body.floating_point || body.instructions > 12 {
            Behaviour::Arithmetic
        } else {
            Behaviour::StackManipulation
        };
    }
    Behaviour::Unknown
}

/// Group every referenced global address into clusters of nearby addresses.
///
/// The gap is the whole model: addresses closer together than one gap are one subject. It is not
/// fitted to an expected answer — it is the smallest gap that does not merge `.rdata` constant
/// pools with `.data` singletons, and the cluster count is reported so a reader can see what a
/// different gap would do.
pub fn cluster_globals(reports: &[OperatorReport], gap: u32) -> Vec<GlobalCluster> {
    let mut by_address: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
    for report in reports {
        for global in &report.body.globals {
            by_address
                .entry(global.address)
                .or_default()
                .insert(report.name.clone());
        }
    }
    let mut clusters: Vec<GlobalCluster> = Vec::new();
    for (address, operators) in by_address {
        match clusters.last_mut() {
            Some(last) if address.saturating_sub(last.end) <= gap => {
                last.end = address;
                last.addresses += 1;
                last.operators.extend(operators);
            }
            _ => clusters.push(GlobalCluster {
                start: address,
                end: address,
                addresses: 1,
                operators,
            }),
        }
    }
    clusters.sort_by_key(|cluster| std::cmp::Reverse(cluster.operators.len()));
    clusters
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalCluster {
    pub start: u32,
    pub end: u32,
    pub addresses: usize,
    pub operators: BTreeSet<String>,
}

// ---------------------------------------------------------------------------------------------

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_c_string(image: &PeImage<'_>, address: u32) -> Option<String> {
    let offset = image.file_offset(address)?;
    let tail = image.bytes().get(offset..offset + 256.min(image.bytes().len() - offset))?;
    let length = tail.iter().position(|byte| *byte == 0)?;
    std::str::from_utf8(&tail[..length]).ok().map(str::to_owned)
}

fn is_printable_string(text: &str) -> bool {
    text.len() >= 4
        && text
            .chars()
            .all(|character| character.is_ascii_graphic() || character == ' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal 32-bit PE with a code section and a data section, so the walker is exercised
    /// without the proprietary binary.
    ///
    /// Mirrors the fixtures in `native_table` and `operator_arity` rather than inventing a third
    /// shape, and deliberately gives `.data` a virtual size larger than its raw size: the engine's
    /// own `.data` does, and a fixture without that property cannot fail on the bug where a global
    /// past the raw data is dropped.
    fn image_with(code: &[u8], data: &[u8]) -> Vec<u8> {
        const PE_OFFSET: usize = 0x80;
        const IMAGE_BASE: u32 = 0x0040_0000;
        const CODE_VA: u32 = 0x1000;
        const CODE_RAW: u32 = 0x200;
        const CODE_SIZE: u32 = 0x400;
        const DATA_VA: u32 = 0x2000;
        const DATA_RAW: u32 = 0x600;
        const DATA_RAW_SIZE: u32 = 0x200;
        const DATA_VIRTUAL_SIZE: u32 = 0x4000;

        let mut image = vec![0_u8; (DATA_RAW + DATA_RAW_SIZE) as usize];
        image[0x3c..0x40].copy_from_slice(&(PE_OFFSET as u32).to_le_bytes());
        image[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        image[PE_OFFSET + 6..PE_OFFSET + 8].copy_from_slice(&2_u16.to_le_bytes());
        let optional_size: u16 = 0xe0;
        image[PE_OFFSET + 20..PE_OFFSET + 22].copy_from_slice(&optional_size.to_le_bytes());
        image[PE_OFFSET + 24..PE_OFFSET + 26].copy_from_slice(&0x10b_u16.to_le_bytes());
        image[PE_OFFSET + 52..PE_OFFSET + 56].copy_from_slice(&IMAGE_BASE.to_le_bytes());

        let table = PE_OFFSET + 24 + usize::from(optional_size);
        let mut section =
            |index: usize, name: &[u8], va: u32, vsize: u32, raw: u32, rsize: u32, flags: u32| {
                let base = table + index * 40;
                image[base..base + name.len()].copy_from_slice(name);
                image[base + 8..base + 12].copy_from_slice(&vsize.to_le_bytes());
                image[base + 12..base + 16].copy_from_slice(&va.to_le_bytes());
                image[base + 16..base + 20].copy_from_slice(&rsize.to_le_bytes());
                image[base + 20..base + 24].copy_from_slice(&raw.to_le_bytes());
                image[base + 36..base + 40].copy_from_slice(&flags.to_le_bytes());
            };
        section(
            0,
            b".text",
            CODE_VA,
            CODE_SIZE,
            CODE_RAW,
            CODE_SIZE,
            0x2000_0000,
        );
        section(
            1,
            b".data",
            DATA_VA,
            DATA_VIRTUAL_SIZE,
            DATA_RAW,
            DATA_RAW_SIZE,
            0x4000_0000,
        );

        image[CODE_RAW as usize..CODE_RAW as usize + code.len()].copy_from_slice(code);
        image[DATA_RAW as usize..DATA_RAW as usize + data.len()].copy_from_slice(data);
        image
    }

    const ENTRY: u32 = 0x0040_1000;

    fn empty_index() -> ProgramIndex {
        ProgramIndex {
            call_targets: BTreeSet::new(),
            imports: BTreeMap::new(),
            import_stubs: BTreeMap::new(),
            pop_helpers: BTreeSet::new(),
            push_helpers: BTreeSet::new(),
        }
    }

    #[test]
    fn records_an_absolute_read_as_a_global() {
        // mov eax,[402010h] ; ret
        let code = [0xa1, 0x10, 0x20, 0x40, 0x00, 0xc3];
        let bytes = image_with(&code, &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(
            body.globals,
            vec![GlobalRef {
                address: 0x0040_2010,
                access: GlobalAccess::Read,
                indexed: false,
            }]
        );
        assert!(body.boundary_complete());
    }

    #[test]
    fn records_a_displacement_behind_a_register_as_an_indexed_table() {
        // The shape `resetvisibility` uses to reach a table: `mov eax,[eax+402100h]`. The
        // displacement is the table, not an offset into a struct, because it lands in `.data`.
        // mov eax,[eax+402100h] ; ret
        let code = [0x8b, 0x80, 0x00, 0x21, 0x40, 0x00, 0xc3];
        let bytes = image_with(&code, &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(body.globals.len(), 1);
        assert_eq!(body.globals[0].address, 0x0040_2100);
        assert!(body.globals[0].indexed);
    }

    #[test]
    fn records_an_address_materialised_as_an_immediate() {
        // mov ecx,402200h ; ret -- how the engine reaches a singleton object.
        let code = [0xb9, 0x00, 0x22, 0x40, 0x00, 0xc3];
        let bytes = image_with(&code, &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(body.globals[0].access, GlobalAccess::Taken);
    }

    #[test]
    fn sees_a_global_past_the_end_of_the_raw_data() {
        // The engine's zero-initialised globals live in virtual space with no file bytes behind
        // them. `.data` raw ends at 0x402200 in the fixture; this address is past it.
        // mov eax,[403f00h] ; ret
        let code = [0xa1, 0x00, 0x3f, 0x40, 0x00, 0xc3];
        let bytes = image_with(&code, &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        assert!(image.file_offset(0x0040_3f00).is_none());
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(body.globals[0].address, 0x0040_3f00);
    }

    #[test]
    fn a_write_is_reported_as_a_write() {
        // mov [402010h],eax ; ret
        let code = [0xa3, 0x10, 0x20, 0x40, 0x00, 0xc3];
        let bytes = image_with(&code, &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert!(body.writes_globals());
    }

    #[test]
    fn disagreeing_paths_leave_the_arity_unresolved() {
        // je +len(pop) ; <inline pop> ; ret -- one path consumes an operand, the other does not.
        let pop = [0x8b, 0x46, 0x54, 0x40, 0x89, 0x46, 0x54];
        let mut code = vec![0x74, pop.len() as u8];
        code.extend_from_slice(&pop);
        code.push(0xc3);
        let bytes = image_with(&code, &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(body.inline_pops, 1);
        assert_eq!(body.arity, None);
        assert_eq!(body.arity_candidates, BTreeSet::from([0, 1]));
    }

    #[test]
    fn agreeing_paths_resolve_to_one_arity() {
        // <inline pop> ; <inline pop> ; ret
        let pop = [0x8b, 0x46, 0x54, 0x40, 0x89, 0x46, 0x54];
        let mut code = Vec::new();
        code.extend_from_slice(&pop);
        code.extend_from_slice(&pop);
        code.push(0xc3);
        let bytes = image_with(&code, &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(body.arity, Some(2));
    }

    #[test]
    fn padding_ends_the_body_rather_than_extending_it() {
        // ret ; int3 ; int3 -- the walk must not run into the next function.
        let code = [0xc3, 0xcc, 0xcc, 0xcc];
        let bytes = image_with(&code, &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(body.instructions, 1);
        assert_eq!(body.span_end, ENTRY + 1);
    }

    #[test]
    fn a_jump_to_a_called_function_is_a_tail_call_not_a_body() {
        // jmp to the next instruction ; ret -- with the target registered as something the
        // program calls, which is what makes it a tail call rather than a fallthrough.
        let code = [0xeb, 0x00, 0xc3, 0xc3];
        let bytes = image_with(&code, &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let mut index = empty_index();
        index.call_targets.insert(ENTRY + 2);
        let body = walk(&image, &index, ENTRY, None).expect("entry is code");
        assert_eq!(body.tail_calls, BTreeSet::from([ENTRY + 2]));
        assert_eq!(body.instructions, 1);
    }

    #[test]
    fn a_string_reference_is_recorded_by_address() {
        let bytes = image_with(&[0x68, 0x00, 0x20, 0x40, 0x00, 0xc3], b"terrain\0");
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(body.string_refs, BTreeSet::from([0x0040_2000]));
    }

    #[test]
    fn clustering_splits_on_the_gap_and_not_on_the_operator() {
        let report = |name: &str, addresses: &[u32]| OperatorReport {
            name: name.to_owned(),
            entry_point: 0,
            body: BodyAnalysis {
                entry_point: 0,
                instructions: 0,
                span_end: 0,
                globals: addresses
                    .iter()
                    .map(|address| GlobalRef {
                        address: *address,
                        access: GlobalAccess::Read,
                        indexed: false,
                    })
                    .collect(),
                calls: BTreeSet::new(),
                tail_calls: BTreeSet::new(),
                direct_imports: BTreeSet::new(),
                writes_through_pointer: BTreeSet::new(),
                writes_through_this: false,
                string_refs: BTreeSet::new(),
                inline_pops: 0,
                inline_pushes: 0,
                helper_pops: 0,
                helper_pushes: 0,
                arity: None,
                arity_candidates: BTreeSet::new(),
                operand_count_unbounded: false,
                returns_arguments: 0,
                indirect_calls: 0,
                unresolved_indirect_jumps: 0,
                resolved_jump_tables: 0,
                invalid_instructions: 0,
                truncated: false,
                floating_point: false,
                returns: 1,
            },
            declared_arity: None,
            reach: Reach::default(),
            behaviour: Behaviour::Unknown,
            next_entry_point: None,
        };
        let reports = [
            report("a", &[0x1000, 0x1010]),
            report("b", &[0x1010, 0x9000]),
        ];
        let clusters = cluster_globals(&reports, 0x40);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].operators.len(), 2);
        assert_eq!(clusters[0].addresses, 2);
    }
}
