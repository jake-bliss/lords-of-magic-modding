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

/// The depth the behaviour classifier reads.
///
/// **One**, and the reported curve is why. By depth 3, 93% of operators reach `user32`, a timer and
/// the `other` bucket; by depth 5, 90% reach `CreateFileA` and 94% reach Storm. Those columns
/// describe how densely the engine's call graph is connected and say nothing about any operator, so
/// reading them would relabel almost the whole table. At depth 1 the same rows are 1%, 3% and 5%.
///
/// The cost is stated rather than hidden: `savescenariomap` genuinely writes a file and is **not**
/// labelled `file-or-resource-io`, because the imports it needs only become reachable at a depth
/// where `dup` reaches them too.
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

/// Where the pointer an object field was reached through came from.
///
/// The distinction is the whole difference between a field that has an absolute address and one
/// that does not, and it is **observed**, not assumed: the two forms are different instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BaseKind {
    /// The object *is* at this address. The body materialised it as an immediate — `mov
    /// ecx,0x5aa12c` — which is how this compiler reaches a statically allocated C++ object. A
    /// field at offset `n` is therefore also reachable as the absolute address `base + n`, and the
    /// two instruments must agree.
    Static,
    /// The address *holds* a pointer to the object — `mov ecx,[0x5ae958]`. The object is a heap
    /// allocation and a field at offset `n` has no absolute address at all. This is why the map
    /// half of the engine's state cannot be recovered from absolute addresses alone.
    Indirect,
    /// The object pointer the caller passed in `ecx`. Carries no address of its own; it only means
    /// something once a caller is found that says which object it passed.
    This,
}

impl BaseKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Indirect => "indirect",
            Self::This => "this",
        }
    }
}

/// An object pointer held in a register, and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PointerBase {
    pub address: u32,
    pub kind: BaseKind,
    /// How far into the object the register points.
    ///
    /// `lea edi,[ecx+0x50ac]` produces an **interior pointer**: still the same object, but a store
    /// through `edi` lands at `+0x50ac`, not at `+0`. Without this bias every sub-object and every
    /// embedded array in the engine reports its fields at the parent's offset 0 — which is how the
    /// save writer's documented `[gameobj+0x520]` block first came back as a field at zero.
    pub offset: u32,
    /// Whether the register holds something *loaded out of* the object rather than the object.
    ///
    /// `mov eax,[esi+8]` where `esi` is the map object leaves `eax` holding a **different** object
    /// — whatever pointer the map stores at `+8`. Attributing that object's offsets to the map
    /// would merge two structures into one, so a dereferenced base contributes no fields. It is
    /// still carried, because `writes_through_pointer` — a published column whose members were
    /// counted before this distinction existed — is defined over exactly this chain.
    pub dereferenced: bool,
    /// Whether an index register took part in forming this pointer.
    ///
    /// `lea ecx,[gameobj+eax*4]` followed by `lea edi,[ecx+0x50ac]` reaches element *n* of an
    /// embedded array. The offset that comes out is the field's offset **within element zero**;
    /// the stride is not recovered. Marking the pointer is what keeps the table from presenting
    /// such an offset as a scalar field of the containing object.
    pub element: bool,
}

impl PointerBase {
    /// Where an access at `displacement` through this pointer lands in the object.
    ///
    /// Saturating rather than wrapping: a negative displacement off a biased pointer is a real
    /// idiom, and the alternative is a field at `0xfffffff8` in the table.
    pub fn field_offset(self, displacement: u32) -> Option<u32> {
        if self.dereferenced {
            return None;
        }
        let offset = self.offset.checked_add(displacement)?;
        (offset <= MAXIMUM_FIELD_OFFSET).then_some(offset)
    }
}

/// One access to a field of an object reached through a tracked pointer.
///
/// `offset` is the instruction's displacement and `width` the decoder's operand size, so both are
/// **observed in a local binary**. `indexed` says a register took part in the effective address,
/// which makes the displacement the base of an array rather than the address of a scalar — a
/// distinction that decides whether "a 4-byte field at +0x10" is a field or a stride.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldAccess {
    pub base: u32,
    pub kind: BaseKind,
    pub offset: u32,
    /// Bytes touched, from the decoder's memory-size model.
    ///
    /// **Zero means the address was taken and not dereferenced** — `lea eax,[obj+0x520]`. The field
    /// is there and its offset is observed; its width is not, because the instruction that reads it
    /// is in a callee this instrument did not follow. Block copies, embedded arrays and sub-objects
    /// all look like this, and dropping them loses exactly the fields a serialiser writes wholesale.
    pub width: u8,
    pub write: bool,
    pub indexed: bool,
}

/// Largest displacement believed to be a field of the object in the register.
///
/// Not a claim about object size. A shallow taint occasionally survives into code where the
/// register no longer holds what it held, and an absurd displacement is the cheapest signal of
/// that. The engine's own structures are far inside this: the save writer copies a 164-byte block
/// from `[gameobj+0x520]` and a player name sits at `[player+0x50ac]`, so the bound has to clear
/// `0x5100` to avoid discarding known-real fields.
const MAXIMUM_FIELD_OFFSET: u32 = 0x1_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalRef {
    pub address: u32,
    pub access: GlobalAccess,
    /// Whether a register took part in the effective address, which is what distinguishes an
    /// element read `[eax+table]` from a scalar read `[variable]`.
    pub indexed: bool,
    /// Whether the address is in a **read-only** data section, which makes it a compiler-emitted
    /// constant and not engine state.
    ///
    /// The engine's `.rdata` is `0x40000040` and its `.data` is `0xc0000040`. Testing only
    /// "not executable" put the arithmetic operators' shared float pool at `0x0054dbc0` in the
    /// same bucket as the world object, so `abs`, `atan`, `cos` and sixteen others were published
    /// as touching engine data and `Behaviour::Arithmetic` had zero members in a 1,906-row table.
    pub read_only: bool,
}

/// Everything the walk could and could not establish about one function body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyAnalysis {
    pub entry_point: u32,
    pub instructions: usize,
    /// Highest address the walk decoded, exclusive.
    ///
    /// Deliberately **not** called a body size. Many operators are two-instruction thunks whose
    /// implementation sits tens of kilobytes away and is reached by an internal `jmp`, so this
    /// measures entry-point-to-farthest-decoded-byte: `abs` decodes 77 instructions across
    /// 15,207 bytes. The instruction count is the size measure.
    pub decoded_extent_end: u32,
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
    /// Every field access made through a tracked object pointer, including through the caller's
    /// `this` (`kind == BaseKind::This`).
    ///
    /// This is the half of the engine's state that absolute addresses cannot see. `writes_through_
    /// pointer` records only *that* a body stored through a pointer read out of a global; this
    /// records **where** — the displacement, the width and the direction — so a field map can be
    /// built instead of a list of objects.
    pub field_accesses: BTreeSet<FieldAccess>,
    /// Operand sizes seen at each absolute data address, from the decoder's memory-size model.
    ///
    /// Kept beside `globals` rather than folded into it because `globals` is deduplicated on a key
    /// the published table's counts depend on, and widening that key would silently change them.
    pub global_widths: BTreeMap<u32, BTreeSet<u8>>,
    /// For each directly called function, the object pointers held in `ecx` at the call sites.
    ///
    /// This is what lets a callee's `[this+n]` accesses be attributed to an object: the caller's
    /// `mov ecx,<global>` names the object and the callee's body supplies the offsets. A callee
    /// reached with `BaseKind::This` in `ecx` is forwarding its own `this`, which is how the chain
    /// continues past one level.
    pub this_call_bases: BTreeMap<u32, BTreeSet<PointerBase>>,
    /// Call sites whose `ecx` the taint could not name. The denominator for every statement of the
    /// form "no operator touches field X": each of these is a method call whose object is unknown,
    /// so whatever it touches is invisible to this instrument.
    pub untracked_calls: usize,
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
    /// Whether a loop consumes operands per iteration, so no finite count describes the body. This
    /// is what a genuinely variadic operator looks like from here, and it is the strong signal:
    /// only a loop can make the candidate counts an arithmetic progression.
    pub operand_loop_carried: bool,
    /// Whether the per-instruction state cap was hit without a loop being seen. That is a long
    /// chain of early exits converging on one instruction, not a variadic operator — `slider` has
    /// twenty candidate counts and no loop, `launchmissile` twenty-one and no loop. The two were
    /// reported under one flag and are now separate.
    pub operand_state_cap_hit: bool,
    /// Bytes of arguments the function releases on return — `ret 4` reports 4. Part of how the
    /// operand helper is recognised.
    pub returns_arguments: u32,
    pub indirect_calls: usize,
    /// Virtual-dispatch edges, as (the global the object pointer came from, the vtable byte
    /// offset).
    ///
    /// The analysis cannot follow these — that limitation stands — but it can *name* the slot, and
    /// the slot is what a reader chasing one subsystem needs. `netlockgame` is the whole shape in
    /// six instructions: load `[0x005d1e84]`, load its vtable, `jmp [vtable+0x58]`.
    pub virtual_calls: BTreeSet<(u32, u32)>,
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
        // A loop that pops per iteration has no nominal count at all. A long chain of early exits
        // does: the largest candidate is still the path that fetched everything.
        if self.operand_loop_carried {
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
    /// global. Read-only data is excluded: a constant is not state.
    pub fn writes_globals(&self) -> bool {
        !self.writes_through_pointer.is_empty()
            || self
                .globals
                .iter()
                .any(|global| global.access == GlobalAccess::Write && !global.read_only)
    }

    /// Whether the body reads engine state. Read-only data is excluded for the same reason.
    pub fn reads_globals(&self) -> bool {
        self.globals
            .iter()
            .any(|global| global.access != GlobalAccess::Write && !global.read_only)
    }

    /// References to read-only data: float pools, jump tables, string literals.
    pub fn constant_refs(&self) -> usize {
        self.globals.iter().filter(|global| global.read_only).count()
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
        decoded_extent_end: entry_point,
        globals: Vec::new(),
        calls: BTreeSet::new(),
        tail_calls: BTreeSet::new(),
        direct_imports: BTreeSet::new(),
        writes_through_pointer: BTreeSet::new(),
        writes_through_this: false,
        field_accesses: BTreeSet::new(),
        global_widths: BTreeMap::new(),
        this_call_bases: BTreeMap::new(),
        untracked_calls: 0,
        string_refs: BTreeSet::new(),
        inline_pops: 0,
        inline_pushes: 0,
        helper_pops: 0,
        helper_pushes: 0,
        arity: None,
        arity_candidates: BTreeSet::new(),
        operand_loop_carried: false,
        operand_state_cap_hit: false,
        returns_arguments: 0,
        indirect_calls: 0,
        virtual_calls: BTreeSet::new(),
        unresolved_indirect_jumps: 0,
        resolved_jump_tables: 0,
        invalid_instructions: 0,
        truncated: false,
        floating_point: false,
        returns: 0,
    };

    let mut steps: BTreeMap<u32, Step> = BTreeMap::new();
    let mut globals: BTreeSet<(u32, GlobalAccess, bool, bool)> = BTreeSet::new();
    let mut starts: VecDeque<u32> = VecDeque::from([entry_point]);
    let mut started: BTreeSet<u32> = BTreeSet::new();
    // The pointer taint each run begins with. Carried along control-flow edges, first writer
    // winning, so a `this` moved into a callee-saved register in the prologue is still recognised
    // in the block that stores through it. First-writer-wins is an approximation and errs towards
    // forgetting, which understates mutation rather than inventing it.
    let mut entry_pointers: BTreeMap<u32, BTreeMap<Register, PointerBase>> = BTreeMap::from([(
        entry_point,
        BTreeMap::from([(
            Register::ECX,
            PointerBase {
                address: THIS_POINTER,
                kind: BaseKind::This,
                offset: 0,
                dereferenced: false,
                element: false,
            },
        )]),
    )]);
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
        let mut pointers: BTreeMap<Register, PointerBase> =
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
            analysis.decoded_extent_end = analysis
                .decoded_extent_end
                .max(address + instruction.len() as u32);

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
            let (memory, written_registers) = {
                let info = info_factory.info(&instruction);
                let memory: Vec<iced_x86::UsedMemory> = info.used_memory().to_vec();
                let written: Vec<Register> = info
                    .used_registers()
                    .iter()
                    .filter(|used| {
                        matches!(
                            used.access(),
                            OpAccess::Write
                                | OpAccess::CondWrite
                                | OpAccess::ReadWrite
                                | OpAccess::ReadCondWrite
                        )
                    })
                    .map(|used| used.register())
                    .collect();
                (memory, written)
            };
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
                // A write is recorded once. Inserting a matching read alongside it duplicated the
                // address in 343 rows, so counting the operators on a cluster by grepping the
                // table gave 140 and 308 where the truth is 139 and 299.
                globals.insert((
                    displacement,
                    access,
                    indexed,
                    !image.is_writable_data_address(displacement),
                ));
                let width = used.memory_size().size();
                if width > 0 && width <= 16 {
                    analysis
                        .global_widths
                        .entry(displacement)
                        .or_default()
                        .insert(width as u8);
                }
            }
            update_pointer_taint(
                image,
                &instruction,
                &memory,
                &written_registers,
                &mut pointers,
                &mut analysis.writes_through_pointer,
                &mut analysis.field_accesses,
            );

            for operand in 0..instruction.op_count() {
                if instruction.op_kind(operand) != OpKind::Immediate32 {
                    continue;
                }
                let value = instruction.immediate32();
                if image.is_data_address(value) && !index.imports.contains_key(&value) {
                    // This linker put string literals in writable `.data`, not `.rdata`, so the
                    // section flags alone call a format string engine state. A NUL-terminated
                    // printable literal is a constant wherever it was placed; the script error
                    // raiser is 90% such references and was unclassifiable without this.
                    let literal = read_c_string(image, value)
                        .is_some_and(|text| is_printable_string(&text));
                    if literal {
                        analysis.string_refs.insert(value);
                    }
                    globals.insert((
                        value,
                        GlobalAccess::Taken,
                        false,
                        literal || !image.is_writable_data_address(value),
                    ));
                    // `mov ecx,<object>` is how this compiler reaches a statically allocated C++
                    // object, and the fields are then touched as `[ecx+n]` inside the method it
                    // calls. Tainting the register here is what makes those offsets attributable.
                    // A string literal is excluded: a format string in `ecx` is not an object.
                    if !literal
                        && image.is_writable_data_address(value)
                        && instruction.mnemonic() == Mnemonic::Mov
                        && instruction.op0_kind() == OpKind::Register
                    {
                        pointers.insert(
                            instruction.op0_register(),
                            PointerBase {
                                address: value,
                                kind: BaseKind::Static,
                                offset: 0,
                                dereferenced: false,
                                element: false,
                            },
                        );
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
                        match pointers.get(&Register::ECX) {
                            Some(base) => {
                                analysis
                                    .this_call_bases
                                    .entry(target)
                                    .or_default()
                                    .insert(*base);
                            }
                            None => analysis.untracked_calls += 1,
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
                            if let Some(slot) = virtual_slot(&instruction, &pointers) {
                                analysis.virtual_calls.insert(slot);
                            }
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
                            match pointers.get(&Register::ECX) {
                                Some(base) => {
                                    analysis
                                        .this_call_bases
                                        .entry(target)
                                        .or_default()
                                        .insert(*base);
                                }
                                None => analysis.untracked_calls += 1,
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
                                if let Some(slot) = virtual_slot(&instruction, &pointers) {
                                    analysis.virtual_calls.insert(slot);
                                }
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
        .map(|(address, access, indexed, read_only)| GlobalRef {
            address,
            access,
            indexed,
            read_only,
        })
        .collect();

    let (arity, candidates, state_cap_hit, loop_carried) = operand_dataflow(&steps, entry_point);
    analysis.arity = arity;
    analysis.arity_candidates = candidates;
    analysis.operand_state_cap_hit = state_cap_hit;
    analysis.operand_loop_carried = loop_carried;
    Ok(analysis)
}

/// Name the vtable slot behind an indirect branch, when the object pointer came from a global.
///
/// `mov ecx,[global]` then `mov eax,[ecx]` then `call/jmp [eax+n]` is one C++ virtual call. The
/// taint chain survives both loads, so `n` and the originating global are both recoverable even
/// though the destination is not.
fn virtual_slot(
    instruction: &Instruction,
    pointers: &BTreeMap<Register, PointerBase>,
) -> Option<(u32, u32)> {
    if instruction.op0_kind() != OpKind::Memory {
        return None;
    }
    let source = *pointers.get(&instruction.memory_base())?;
    // The caller's own `this` is not a named global, so there is nothing to attribute the slot to.
    // A statically allocated object is excluded for a different reason: its vtable pointer is a
    // link-time constant, so a slot recovered from one says nothing the relocation does not.
    if source.kind != BaseKind::Indirect {
        return None;
    }
    Some((source.address, instruction.memory_displacement64() as u32))
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
    written_registers: &[Register],
    pointers: &mut BTreeMap<Register, PointerBase>,
    writes: &mut BTreeSet<u32>,
    fields: &mut BTreeSet<FieldAccess>,
) {
    for used in memory {
        let writing = matches!(
            used.access(),
            OpAccess::Write | OpAccess::CondWrite | OpAccess::ReadWrite | OpAccess::ReadCondWrite
        );
        // Every access through a tracked pointer is a field access, read or write. The offset is
        // the instruction's own displacement; the width is the decoder's memory-size model.
        if let Some(base) = pointers.get(&used.base()) {
            let width = used.memory_size().size();
            if let Some(offset) = base.field_offset(used.displacement() as u32)
                && width > 0
                && width <= 16
            {
                fields.insert(FieldAccess {
                    base: base.address,
                    kind: base.kind,
                    offset,
                    width: width as u8,
                    write: writing,
                    indexed: base.element || used.index() != Register::None,
                });
            }
        }
        if !writing {
            continue;
        }
        for register in [used.base(), used.index()] {
            if let Some(source) = pointers.get(&register) {
                // A statically allocated object is deliberately excluded from this set. The set
                // feeds the published `mutates-state` class, whose members were counted before
                // static bases were tracked at all; folding them in silently would move a number
                // the documentation quotes. What the static bases add is reported separately.
                if source.kind != BaseKind::Static {
                    writes.insert(source.address);
                }
            }
        }
    }

    // `lea` computes a field's address without reading it, so the decoder reports no memory access
    // and the loop above never sees it. That is not a rare corner: the save writer reaches its
    // 164-byte setup block as `lea eax,[gameobj+0x520]` and a player name as `lea edi,[player+
    // 0x50ac]`, both of which this analysis would otherwise report as absent.
    if instruction.mnemonic() == Mnemonic::Lea
        && instruction.op1_kind() == OpKind::Memory
        && let Some(base) = pointers.get(&instruction.memory_base()).copied()
        && let Some(offset) = base.field_offset(instruction.memory_displacement64() as u32)
    {
        fields.insert(FieldAccess {
            base: base.address,
            kind: base.kind,
            offset,
            width: 0,
            write: false,
            indexed: base.element || instruction.memory_index() != Register::None,
        });
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
                    && image.is_writable_data_address(displacement)
                {
                    // `lea reg,[global]` takes the address; `mov reg,[global]` loads what is
                    // stored there. The first names a statically allocated object, the second a
                    // pointer variable, and conflating them loses exactly the distinction that
                    // decides whether a field has an absolute address.
                    Some(PointerBase {
                        address: displacement,
                        kind: if instruction.mnemonic() == Mnemonic::Lea {
                            BaseKind::Static
                        } else {
                            BaseKind::Indirect
                        },
                        offset: 0,
                        dereferenced: false,
                        element: false,
                    })
                } else if instruction.mnemonic() == Mnemonic::Lea {
                    // An interior pointer into the same object: carry the object and add the bias.
                    // A `mov` from `[base+n]` loads whatever is *stored* there, which is a
                    // different object entirely and must drop the taint, not inherit it.
                    pointers.get(&instruction.memory_base()).map(|base| {
                        match base.field_offset(instruction.memory_displacement64() as u32) {
                            Some(offset) => PointerBase {
                                offset,
                                element: base.element
                                    || instruction.memory_index() != Register::None,
                                ..*base
                            },
                            // The bias left the range this analysis believes, so the register is
                            // still in the object's chain but no longer at a known offset. Keeping
                            // it is what `writes_through_pointer` is defined over; closing it to
                            // field mapping is what stops a guessed offset entering the table.
                            None => PointerBase {
                                dereferenced: true,
                                ..*base
                            },
                        }
                    })
                } else {
                    // A load out of a tracked object. The value is a different object, so the
                    // chain is kept for `writes_through_pointer` and closed for field mapping.
                    pointers
                        .get(&instruction.memory_base())
                        .map(|base| PointerBase {
                            dereferenced: true,
                            ..*base
                        })
                }
            }
            _ => None,
        };
        match source {
            Some(base) => {
                pointers.insert(destination, base);
            }
            None => {
                pointers.remove(&destination);
            }
        }
        return;
    }

    // Anything else that writes a register invalidates whatever it held. Keyed on the decoder's
    // *write* set, not on "the first operand is a register": `test ecx,ecx` has a register first
    // operand and writes nothing, and dropping the taint there is what stopped `netlockgame` —
    // `mov ecx,[global]` / `test ecx,ecx` / `mov eax,[ecx]` / `jmp [eax+0x58]` — from naming its
    // own dispatch slot.
    for register in written_registers {
        pointers.remove(register);
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
) -> (Option<usize>, BTreeSet<usize>, bool, bool) {
    /// Distinct operand counts tracked per instruction before the body is called unbounded.
    const MAXIMUM_STATES: usize = 16;

    let mut states: HashMap<u32, BTreeSet<usize>> = HashMap::new();
    let mut state_cap_hit = false;
    // Whether any operand fetch sits inside a cycle in the control-flow graph.
    //
    // Found by strongly-connected components, not by a lower address. "The successor sits at a
    // lower address" looked like a back edge and is not one: the compiler puts the error path's
    // shared epilogue below the code that jumps to it, so 26 operators that fetch through a
    // per-subsystem wrapper were declared variadic and came back nullary, because a variadic
    // callee contributes nothing to its caller's count.
    let loop_carried = pops_inside_a_cycle(steps);
    let mut queue = VecDeque::from([(entry_point, 0_usize)]);
    let mut at_returns: BTreeSet<usize> = BTreeSet::new();

    while let Some((address, before)) = queue.pop_front() {
        let seen = states.entry(address).or_default();
        if !seen.insert(before) {
            continue;
        }
        if seen.len() > MAXIMUM_STATES {
            state_cap_hit = true;
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

    let arity = (at_returns.len() == 1 && !state_cap_hit && !loop_carried)
        .then(|| at_returns.iter().next().copied())
        .flatten();
    (arity, at_returns, state_cap_hit, loop_carried)
}

/// Whether any instruction that consumes an operand lies on a cycle.
///
/// Tarjan's algorithm over the walked instructions. A non-trivial strongly-connected component, or
/// a self-edge, is a loop; a fetch inside one means the operand count grows per iteration, which is
/// the only thing that can make the candidate counts an arithmetic progression — `armyexpense`
/// steps by five, `combat_controltarget` by two.
fn pops_inside_a_cycle(steps: &BTreeMap<u32, Step>) -> bool {
    struct State<'a> {
        steps: &'a BTreeMap<u32, Step>,
        index: HashMap<u32, usize>,
        low: HashMap<u32, usize>,
        on_stack: BTreeSet<u32>,
        stack: Vec<u32>,
        next: usize,
        found: bool,
    }

    fn visit(state: &mut State<'_>, node: u32) {
        state.index.insert(node, state.next);
        state.low.insert(node, state.next);
        state.next += 1;
        state.stack.push(node);
        state.on_stack.insert(node);

        let successors = state
            .steps
            .get(&node)
            .map(|step| step.successors.clone())
            .unwrap_or_default();
        for successor in successors {
            if successor == node {
                // A self-edge is a one-instruction loop.
                if state.steps.get(&node).is_some_and(|step| step.operands_consumed > 0) {
                    state.found = true;
                }
                continue;
            }
            if !state.index.contains_key(&successor) {
                visit(state, successor);
                let child = state.low[&successor];
                let own = state.low[&node];
                state.low.insert(node, own.min(child));
            } else if state.on_stack.contains(&successor) {
                let child = state.index[&successor];
                let own = state.low[&node];
                state.low.insert(node, own.min(child));
            }
        }

        if state.low[&node] == state.index[&node] {
            let mut component = Vec::new();
            while let Some(member) = state.stack.pop() {
                state.on_stack.remove(&member);
                component.push(member);
                if member == node {
                    break;
                }
            }
            if component.len() > 1
                && component.iter().any(|member| {
                    state
                        .steps
                        .get(member)
                        .is_some_and(|step| step.operands_consumed > 0)
                })
            {
                state.found = true;
            }
        }
    }

    let mut state = State {
        steps,
        index: HashMap::new(),
        low: HashMap::new(),
        on_stack: BTreeSet::new(),
        stack: Vec::new(),
        next: 0,
        found: false,
    };
    for node in steps.keys() {
        if !state.index.contains_key(node) {
            visit(&mut state, *node);
        }
    }
    state.found
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

/// What the body evidence says an operator does. The ladder is built in `classify`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Behaviour {
    Unknown,
    /// A body that is a single `ret`: a name the engine registers and does not implement.
    Stub,
    /// Acts on its operands and the operand stack only: no engine state, and no call to anything
    /// that touches engine state. Honestly named — `sleep` and `debug` qualify as surely as `dup`
    /// does, and calling the class `stack` claimed more than the evidence.
    OperandOnly,
    /// `OperandOnly`, and at least one x87 instruction decoded. Usually a numeric operator —
    /// `abs`, `atan`, `sqrt`, `add` — but the evidence is the x87 unit and not arithmetic intent:
    /// `sleep` is in this class because it coerces a float delay with `fld`. Named for what was
    /// observed rather than for what was meant.
    FloatingPoint,
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
            Self::Stub => "stub",
            Self::OperandOnly => "operand-only",
            Self::FloatingPoint => "floating-point",
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
            .is_some_and(|next| next > self.entry_point && self.body.decoded_extent_end > next)
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
    /// Every function the image calls from anywhere, walked body-local, keyed by entry point.
    ///
    /// Published because the operator bodies alone do not contain the engine's field accesses: the
    /// caller names the object and the callee supplies the offsets, so joining them needs both.
    /// See `crate::engine_state`.
    pub bodies: HashMap<u32, BodyAnalysis>,
}

/// Walk one function body on its own, for inspecting a callee the table only names.
///
/// Exists because the first version of `discounted_callees` was written from a guess about what the
/// script error raiser at `0x004d4550` touches, and a guess is not a measurement.
pub fn inspect(
    image: &PeImage<'_>,
    entry_point: u32,
    operator_entry_points: &[u32],
) -> Result<BodyAnalysis, NativeTableError> {
    let index = ProgramIndex::build(image, operator_entry_points)?;
    walk(image, &index, entry_point, None)
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

    // Callees that cannot distinguish one operator from another, and so are not evidence that the
    // operator does anything: the shared operand helpers, plus any callee that most of the table
    // calls. This is the same argument the import depth is chosen by — a thing reached by almost
    // everything describes the call graph, not the caller — applied to callees instead of imports.
    //
    // It replaces a rule written from a guess about what the script error raiser at `0x004d4550`
    // touches. Measuring it showed the guess was wrong: it references the engine's error-message
    // objects, so "no globals" never held, and `dup`, `exch`, `pop` and `roll` stayed in `unknown`.
    let mut discounted_callees: BTreeSet<u32> = index
        .pop_helpers
        .union(&index.push_helpers)
        .copied()
        .collect();
    let mut callers: HashMap<u32, usize> = HashMap::new();
    for entry in &entry_points {
        if let Some(body) = bodies.get(entry) {
            for callee in body.calls.iter().chain(body.tail_calls.iter()) {
                *callers.entry(*callee).or_default() += 1;
            }
        }
    }
    let shared_callee_threshold = entry_points.len() / 2;
    for (callee, count) in &callers {
        if *count >= shared_callee_threshold {
            discounted_callees.insert(*callee);
        }
    }
    // A leaf that references no data of any kind and reaches no import is pure computation — the
    // compiler's float conversion routines are the case that matters, and without this `add` and
    // `eq` land in `unknown` for calling one.
    for (entry, callee) in &bodies {
        if callee.globals.is_empty()
            && callee.direct_imports.is_empty()
            && callee.calls.is_empty()
            && callee.tail_calls.is_empty()
            && callee.boundary_complete()
        {
            discounted_callees.insert(*entry);
        }
    }
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
        let behaviour = classify(&body, &reach, CLASSIFY_DEPTH, &discounted_callees);
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
    Ok(Analysis {
        reports,
        index,
        bodies,
    })
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

/// What the body evidence says an operator does.
///
/// Two rules here were wrong first and are worth stating as rules.
///
/// **`MutatesEngineState` requires a store in *this* body.** It used to be granted on a direct
/// callee's store as well, which made the class false for 154 of its 487 rows — including the
/// predicates `armycanmove?`, `armystrength` and `ambientlight`. Somebody filtering the table for
/// the state-editing API got predicates and still did not get `setterrain`: two errors at once.
/// The callee's store is now carried as `Reach::write_depth` and as `calls_mutating_method`, which
/// is the treatment that flag already got for the same reason.
///
/// **A call to a shared operand helper, or to a body that touches no state and no import, is not
/// evidence that the operator does anything.** Every operator reaches the script error raiser, and
/// counting that as a call left `dup`, `exch`, `pop` and `roll` in `unknown`.
fn classify(
    body: &BodyAnalysis,
    reach: &Reach,
    depth: usize,
    discounted_callees: &BTreeSet<u32>,
) -> Behaviour {
    if !body.boundary_complete() {
        return Behaviour::Unknown;
    }
    // A one-instruction body that returns is a registered name with no implementation behind it.
    if body.instructions == 1 && body.returns == 1 {
        return Behaviour::Stub;
    }
    // A mutation the body performs itself is the only mutation this analysis can attribute.
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
    if body.reads_globals() {
        return Behaviour::ReadsEngineState;
    }
    let engine_calls = body
        .calls
        .iter()
        .chain(body.tail_calls.iter())
        .filter(|target| !discounted_callees.contains(target))
        .count();
    if !body.reads_globals() && !body.writes_globals() && engine_calls == 0 {
        // The x87 unit is the only positive evidence available here that the computation is
        // numeric. A reference to a constant is not: `sleep` and `debug` reference a string and
        // were published as arithmetic on that basis.
        return if body.floating_point {
            Behaviour::FloatingPoint
        } else {
            Behaviour::OperandOnly
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
    let mut by_address: BTreeMap<u32, (BTreeSet<String>, bool)> = BTreeMap::new();
    for report in reports {
        for global in &report.body.globals {
            let entry = by_address
                .entry(global.address)
                .or_insert_with(|| (BTreeSet::new(), true));
            entry.0.insert(report.name.clone());
            entry.1 &= global.read_only;
        }
    }
    let mut clusters: Vec<GlobalCluster> = Vec::new();
    for (address, (operators, read_only)) in by_address {
        match clusters.last_mut() {
            // A read-only run and a writable run are never merged: one is the engine's state and
            // the other is the compiler's constants, and merging them is what hid the arithmetic
            // operators' float pool inside "touches engine data".
            Some(last) if address.saturating_sub(last.end) <= gap && last.read_only == read_only => {
                last.end = address;
                last.addresses += 1;
                last.operators.extend(operators);
            }
            _ => clusters.push(GlobalCluster {
                start: address,
                end: address,
                addresses: 1,
                operators,
                read_only,
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
    /// Whether every address in the run is in read-only data: a constant pool, not engine state.
    pub read_only: bool,
}

// --------------------------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------------------------

/// The per-operator table. One row per operator, tab separated, sorted by name so a diff between
/// two runs is a diff of findings rather than of table order.
///
/// Lives in the library rather than in the driver so the staleness test can regenerate the exact
/// bytes and hash them. The first version checked three of twenty-eight columns, which left the
/// two columns the offline anchors actually read — `call_targets` and `global_addresses` — with no
/// staleness check at all.
pub fn operator_table(reports: &[OperatorReport]) -> String {
    const COLUMNS: [&str; 33] = [
        "name",
        "entry_point",
        "behaviour",
        "boundary",
        "instructions",
        "decoded_extent_bytes",
        "arity",
        "arity_candidates",
        "nominal_arity",
        "declared_arity",
        "arity_agrees",
        "loop_carried_operands",
        "operand_state_cap_hit",
        "helper_pops",
        "helper_pushes",
        "inline_pops",
        "inline_pushes",
        "calls",
        "call_targets",
        "tail_calls",
        "indirect_calls",
        "virtual_call_slots",
        "globals_read",
        "constant_refs",
        "globals_written",
        "pointer_write_addresses",
        "global_addresses",
        "constant_addresses",
        "string_refs",
        "import_kinds_at_depth",
        "state_write_depth",
        "calls_mutating_method",
        "direct_imports",
    ];

    let mut rows: Vec<&OperatorReport> = reports.iter().collect();
    rows.sort_by(|left, right| left.name.cmp(&right.name));

    let mut text = COLUMNS.join("\t");
    text.push('\n');
    for report in rows {
        let body = &report.body;
        // Engine state only. A read-only address is a compiler constant and is counted separately.
        let reads = body
            .globals
            .iter()
            .filter(|global| global.access != GlobalAccess::Write && !global.read_only)
            .count();
        let writes = body
            .globals
            .iter()
            .filter(|global| global.access == GlobalAccess::Write && !global.read_only)
            .count();
        let list = |values: Vec<String>| {
            if values.is_empty() {
                "-".to_owned()
            } else {
                values.join(",")
            }
        };
        let number = |value: Option<usize>| {
            value.map_or_else(|| "-".to_owned(), |value| value.to_string())
        };
        let fields: Vec<String> = vec![
            report.name.clone(),
            format!("{:#010x}", report.entry_point),
            report.behaviour.label().to_owned(),
            body.boundary_failure().unwrap_or("complete").to_owned(),
            body.instructions.to_string(),
            body
                .decoded_extent_end
                .saturating_sub(report.entry_point)
                .to_string(),
            number(body.arity),
            list(body.arity_candidates.iter().map(usize::to_string).collect()),
            number(body.nominal_arity()),
            number(report.declared_arity),
            match report.arity_disagreement() {
                None => "yes".to_owned(),
                Some(_) => "NO".to_owned(),
            },
            if body.operand_loop_carried { "yes" } else { "no" }.to_owned(),
            if body.operand_state_cap_hit { "yes" } else { "no" }.to_owned(),
            body.helper_pops.to_string(),
            body.helper_pushes.to_string(),
            body.inline_pops.to_string(),
            body.inline_pushes.to_string(),
            body.calls.len().to_string(),
            list(
                body.calls
                    .iter()
                    .chain(body.tail_calls.iter())
                    .map(|target| format!("{target:#x}"))
                    .collect(),
            ),
            body.tail_calls.len().to_string(),
            body.indirect_calls.to_string(),
            list(
                body.virtual_calls
                    .iter()
                    .map(|(source, offset)| format!("{source:#x}+{offset:#x}"))
                    .collect(),
            ),
            reads.to_string(),
            body.constant_refs().to_string(),
            writes.to_string(),
            list(
                body.writes_through_pointer
                    .iter()
                    .map(|address| format!("{address:#x}"))
                    .collect(),
            ),
            list(
                body.globals
                    .iter()
                    .filter(|global| !global.read_only)
                    .map(|global| format!("{:#x}", global.address))
                    .collect(),
            ),
            list(
                body.globals
                    .iter()
                    .filter(|global| global.read_only)
                    .map(|global| format!("{:#x}", global.address))
                    .collect(),
            ),
            body.string_refs.len().to_string(),
            list(
                report
                    .reach
                    .import_depth
                    .iter()
                    .map(|(kind, depth)| format!("{}@{depth}", kind.label()))
                    .collect(),
            ),
            report
                .reach
                .write_depth
                .map_or_else(|| "-".to_owned(), |depth| depth.to_string()),
            if report.reach.calls_mutating_method { "yes" } else { "no" }.to_owned(),
            list(body.direct_imports.iter().cloned().collect()),
        ];
        debug_assert_eq!(fields.len(), COLUMNS.len());
        text.push_str(&fields.join("\t"));
        text.push('\n');
    }
    text
}

pub fn cluster_table(clusters: &[GlobalCluster]) -> String {
    let mut text =
        String::from("start\tend\tbytes\tdistinct_addresses\tread_only\toperators\toperator_names\n");
    for cluster in clusters {
        let mut names: Vec<&str> = cluster.operators.iter().map(String::as_str).collect();
        names.sort_unstable();
        text.push_str(&format!(
            "{:#010x}\t{:#010x}\t{}\t{}\t{}\t{}\t{}\n",
            cluster.start,
            cluster.end,
            cluster.end - cluster.start,
            cluster.addresses,
            if cluster.read_only { "yes" } else { "no" },
            cluster.operators.len(),
            names.join(","),
        ));
    }
    text
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

    /// Build a minimal 32-bit PE with a code section, a **writable** `.data` and a read-only
    /// `.rdata`, so the walker is exercised without the proprietary binary.
    ///
    /// Mirrors the fixtures in `native_table` and `operator_arity` rather than inventing a third
    /// shape, and deliberately reproduces two properties of the engine's own image that a
    /// convenient fixture would not have: `.data` declares more virtual space than raw data, so a
    /// global past the file bytes must still be seen; and `.rdata` exists and is not writable, so a
    /// constant must not be mistaken for state. The first fixture had a single non-writable data
    /// section, which is exactly why nothing failed when `is_data_address` ignored the write flag.
    fn image_with(code: &[u8], data: &[u8], constants: &[u8]) -> Vec<u8> {
        const PE_OFFSET: usize = 0x80;
        const IMAGE_BASE: u32 = 0x0040_0000;
        const CODE_VA: u32 = 0x1000;
        const CODE_RAW: u32 = 0x200;
        const CODE_SIZE: u32 = 0x400;
        const DATA_VA: u32 = 0x2000;
        const DATA_RAW: u32 = 0x600;
        const DATA_RAW_SIZE: u32 = 0x200;
        const DATA_VIRTUAL_SIZE: u32 = 0x4000;
        const RDATA_VA: u32 = 0x8000;
        const RDATA_RAW: u32 = 0x800;
        const RDATA_SIZE: u32 = 0x200;

        let mut image = vec![0_u8; (RDATA_RAW + RDATA_SIZE) as usize];
        image[0x3c..0x40].copy_from_slice(&(PE_OFFSET as u32).to_le_bytes());
        image[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        image[PE_OFFSET + 6..PE_OFFSET + 8].copy_from_slice(&3_u16.to_le_bytes());
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
        // IMAGE_SCN_MEM_EXECUTE | IMAGE_SCN_CNT_CODE
        section(
            0,
            b".text",
            CODE_VA,
            CODE_SIZE,
            CODE_RAW,
            CODE_SIZE,
            0x2000_0020,
        );
        // IMAGE_SCN_MEM_READ | IMAGE_SCN_MEM_WRITE | IMAGE_SCN_CNT_INITIALIZED_DATA
        section(
            1,
            b".data",
            DATA_VA,
            DATA_VIRTUAL_SIZE,
            DATA_RAW,
            DATA_RAW_SIZE,
            0xc000_0040,
        );
        // IMAGE_SCN_MEM_READ only: the flag that makes this a constant and not state.
        section(
            2,
            b".rdata",
            RDATA_VA,
            RDATA_SIZE,
            RDATA_RAW,
            RDATA_SIZE,
            0x4000_0040,
        );

        image[CODE_RAW as usize..CODE_RAW as usize + code.len()].copy_from_slice(code);
        image[DATA_RAW as usize..DATA_RAW as usize + data.len()].copy_from_slice(data);
        image[RDATA_RAW as usize..RDATA_RAW as usize + constants.len()]
            .copy_from_slice(constants);
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
        let bytes = image_with(&code, &[], &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(
            body.globals,
            vec![GlobalRef {
                address: 0x0040_2010,
                access: GlobalAccess::Read,
                indexed: false,
                read_only: false,
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
        let bytes = image_with(&code, &[], &[]);
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
        let bytes = image_with(&code, &[], &[]);
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
        let bytes = image_with(&code, &[], &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        assert!(image.file_offset(0x0040_3f00).is_none());
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(body.globals[0].address, 0x0040_3f00);
    }

    #[test]
    fn a_read_only_data_reference_is_a_constant_and_not_engine_state() {
        // mov eax,[408000h] ; ret -- an address in the non-writable `.rdata` section.
        let code = [0xa1, 0x00, 0x80, 0x40, 0x00, 0xc3];
        let bytes = image_with(&code, &[], &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(body.globals.len(), 1);
        assert!(body.globals[0].read_only);
        assert!(!body.reads_globals(), "a constant is not engine state");
        assert_eq!(body.constant_refs(), 1);
    }

    #[test]
    fn a_string_literal_in_writable_data_is_still_a_constant() {
        // push 402000h ; ret -- the literal sits in `.data`, which this linker does.
        let bytes = image_with(&[0x68, 0x00, 0x20, 0x40, 0x00, 0xc3], b"terrain\0", &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert!(image.is_writable_data_address(0x0040_2000));
        assert!(body.globals[0].read_only, "a printable literal is a constant");
        assert!(!body.reads_globals());
    }

    #[test]
    fn a_write_is_reported_as_a_write() {
        // mov [402010h],eax ; ret
        let code = [0xa3, 0x10, 0x20, 0x40, 0x00, 0xc3];
        let bytes = image_with(&code, &[], &[]);
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
        let bytes = image_with(&code, &[], &[]);
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
        let bytes = image_with(&code, &[], &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(body.arity, Some(2));
    }

    #[test]
    fn padding_ends_the_body_rather_than_extending_it() {
        // ret ; int3 ; int3 -- the walk must not run into the next function.
        let code = [0xc3, 0xcc, 0xcc, 0xcc];
        let bytes = image_with(&code, &[], &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let body = walk(&image, &empty_index(), ENTRY, None).expect("entry is code");
        assert_eq!(body.instructions, 1);
        assert_eq!(body.decoded_extent_end, ENTRY + 1);
    }

    #[test]
    fn a_jump_to_a_called_function_is_a_tail_call_not_a_body() {
        // jmp to the next instruction ; ret -- with the target registered as something the
        // program calls, which is what makes it a tail call rather than a fallthrough.
        let code = [0xeb, 0x00, 0xc3, 0xc3];
        let bytes = image_with(&code, &[], &[]);
        let image = PeImage::parse(&bytes).expect("fixture parses");
        let mut index = empty_index();
        index.call_targets.insert(ENTRY + 2);
        let body = walk(&image, &index, ENTRY, None).expect("entry is code");
        assert_eq!(body.tail_calls, BTreeSet::from([ENTRY + 2]));
        assert_eq!(body.instructions, 1);
    }

    #[test]
    fn a_string_reference_is_recorded_by_address() {
        let bytes = image_with(&[0x68, 0x00, 0x20, 0x40, 0x00, 0xc3], b"terrain\0", &[]);
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
                decoded_extent_end: 0,
                globals: addresses
                    .iter()
                    .map(|address| GlobalRef {
                        address: *address,
                        access: GlobalAccess::Read,
                        indexed: false,
                        read_only: false,
                    })
                    .collect(),
                calls: BTreeSet::new(),
                tail_calls: BTreeSet::new(),
                direct_imports: BTreeSet::new(),
                writes_through_pointer: BTreeSet::new(),
                writes_through_this: false,
                field_accesses: BTreeSet::new(),
                global_widths: BTreeMap::new(),
                this_call_bases: BTreeMap::new(),
                untracked_calls: 0,
                string_refs: BTreeSet::new(),
                inline_pops: 0,
                inline_pushes: 0,
                helper_pops: 0,
                helper_pushes: 0,
                arity: None,
                arity_candidates: BTreeSet::new(),
                operand_loop_carried: false,
                operand_state_cap_hit: false,
                returns_arguments: 0,
                indirect_calls: 0,
                virtual_calls: BTreeSet::new(),
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
