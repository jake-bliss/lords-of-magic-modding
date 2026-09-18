//! Read the map file format out of `lomse.exe` instead of inferring it from the corpus.
//!
//! Everything this project knows about the map header, the cell tag and the six trailing-record
//! sizes was inferred from statistics over 365 shipped files. That method has already shipped two
//! wrong theses here. The engine's own loader is the authority, and it is sitting in the shipped
//! binary, so this program reads the field offsets and the version thresholds directly out of the
//! instructions rather than out of the files.
//!
//! **The executable is proprietary and is not committed**, so its path is an argument:
//!
//! ```sh
//! cargo run --release --example map_loader_survey -- "/path/to/English/lomse.exe"
//! ```
//!
//! Three reports come out, each labelled with the addresses that carry it:
//!
//! 1. `serialisation` -- the field-by-field `fread`/`fwrite` trace of the terrain block, the
//!    scenario wrapper and one trailing record, recovered by matching calls to the stream
//!    primitives and reading back the arguments that were pushed for them.
//! 2. `record-size` -- the trailing record's size for every header word, computed by walking the
//!    record serialiser's control flow with the header word bound to a value. The engine gates
//!    five separate field groups on it, and those gates are what the corpus saw as "six layouts".
//! 3. `cell-tag-bits` -- every instruction in a map-object method whose operand is a cell lane,
//!    and the bits each one masks, tests or sets. Bits nothing references are reported too.
//!
//! `MAP_SURVEY_DUMP_METHODS=1` additionally lists the map-object methods the third report walked,
//! which is what to look at when the cell survey seems too quiet.
//!
//! The anchors that prove the addresses are right are asserted, not assumed: the program exits
//! non-zero if the executable does not contain the `resetvisibility` operator body, the terrain
//! reader and the record serialiser at the addresses this analysis is built on. An instrument that
//! reports a clean survey of the wrong bytes is the failure this repository has a standing lesson
//! about.

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use iced_x86::{
    Decoder, DecoderOptions, Formatter, Instruction, InstructionInfoFactory, Mnemonic,
    NasmFormatter, OpAccess, OpKind, Register,
};
use lom_asset_viewer::native_table::{OperatorIndex, PeImage};

// ---------------------------------------------------------------------------------------------
// Addresses. Every one of these is checked by `verify_anchors` before anything is reported.
// ---------------------------------------------------------------------------------------------

/// `fread(void *destination, size_t size, size_t count, FILE *stream)`.
const STREAM_READ: u32 = 0x0053_97F0;
/// `fwrite(const void *source, size_t size, size_t count, FILE *stream)`.
const STREAM_WRITE: u32 = 0x0053_9660;

/// The single global map object. `loadmap` calls the terrain reader on it directly, and
/// `loadscenariomap` calls the same reader on the scenario object's terrain member at
/// `SCENARIO_OBJECT + 0x482c` -- which is this same address, so there is one map object, not two.
const MAP_OBJECT: u32 = 0x005A_E958;
/// The scenario object. Its first dword is the header word read from file offset `0x00`.
const SCENARIO_OBJECT: u32 = 0x005A_A12C;
/// Offset of the cell array pointer inside the map object.
const MAP_CELLS_FIELD: u32 = 0x54;
/// Offset of the terrain member inside the scenario object. `SCENARIO_OBJECT + this == MAP_OBJECT`.
const SCENARIO_TERRAIN_FIELD: u32 = 0x482C;
/// A two-instruction accessor that returns the map object in `eax`: `mov eax, MAP_OBJECT; ret`.
///
/// **This is the third route to the object and the largest.** Hundreds of call sites reach the map
/// object through it, and none of them contains the literal `0x005ae958` or the `lea` form, so
/// neither of the other two discovery patterns can see any of them. [`verify_anchors`] checks the
/// function really is that pair of instructions rather than trusting the address.
const MAP_OBJECT_ACCESSOR: u32 = 0x004C_6FC0;

/// `terrain_read(FILE *stream, int)` -- width, height, cell size, then the cell grid.
const TERRAIN_READER: u32 = 0x004A_52E0;
/// `terrain_write(FILE *stream)` -- the mirror of [`TERRAIN_READER`].
const TERRAIN_WRITER: u32 = 0x004A_5440;
/// `scenario_read(FILE *stream)` -- header word, terrain block, trailing records, version section.
const SCENARIO_READER: u32 = 0x0048_55C0;
/// `scenario_write(FILE *stream)` -- the body both `savescenariomap` and `savespecialmap` reach.
const SCENARIO_WRITER: u32 = 0x0048_5550;
/// `records_read(FILE *stream, int, int)` -- count, then one dispatch per record kind.
const RECORD_SECTION_READER: u32 = 0x004F_7120;
/// The record-kind jump table and its bound, as recovered from the dispatch itself.
///
/// Both used to be constants here. That defeated the point of [`verify_anchors`]: a build whose
/// dispatch had a different bound would have been surveyed against this file's idea of it. They are
/// now read out of the `cmp`/`ja`/`jmp` sequence by [`recover_record_kind_dispatch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RecordKindDispatch {
    /// Address of the `jmp dword [reg*4 + table]`.
    address: u32,
    table: u32,
    /// Entries in the table: the `cmp` bound plus one.
    count: usize,
}

/// How many rounds of `this`-forwarding to follow when collecting map-object methods.
const MAP_METHOD_ROUNDS: usize = 8;

/// Instructions decoded per analysis entry before giving up. A function longer than this is
/// surveyed only as far as the limit, which is a loss path, so the number of entries that hit it is
/// reported rather than left implicit.
const DECODE_LIMIT: usize = 2400;

/// Iteration cap for the per-function taint fixpoint. A cap rather than a proof of termination:
/// the lattice is finite and decreasing so it does terminate, but a decoder that walked off the
/// end of a function could produce a graph this never settles on, and hanging is worse than
/// reporting less.
const TAINT_ROUND_LIMIT: usize = 20_000;
/// The serialiser shared by the placed-object record's read and write virtuals.
const RECORD_SERIALISER: u32 = 0x0050_DA70;
/// The base-class prefix the record serialiser calls first: six dwords, unconditional.
const RECORD_BASE_PREFIX: u32 = 0x004F_6B00;
/// `resetvisibility`'s body. The confirmed anchor: it writes the cell tag's high half.
const RESET_VISIBILITY_BODY: u32 = 0x004A_90B0;
/// The map object's allocator: it stores width, height, cell count and the `count * 8` cell array.
const MAP_ALLOCATOR: u32 = 0x004A_4F20;

/// Recover the record-kind dispatch: its bound, its table and where it lives.
///
/// The engine guards the jump table with `cmp reg, N` and `ja error`, then `jmp dword [reg*4 + T]`.
/// Reading `N` and `T` out of those instructions is what lets the anchor check refuse a build whose
/// dispatch is shaped differently, rather than silently indexing this build's table by another
/// build's bound.
fn recover_record_kind_dispatch(image: &PeImage<'_>) -> Result<RecordKindDispatch, String> {
    let instructions = disassemble(image, RECORD_SECTION_READER, 400)?;
    let mut bound: Option<u32> = None;
    for window in instructions.windows(3) {
        if window[0].mnemonic() == Mnemonic::Cmp && is_immediate(window[0].op1_kind()) {
            bound = Some(window[0].immediate32());
        }
        if window[1].mnemonic() != Mnemonic::Ja {
            continue;
        }
        let jump = &window[2];
        if jump.mnemonic() != Mnemonic::Jmp
            || jump.op0_kind() != OpKind::Memory
            || jump.memory_index_scale() != 4
            || jump.memory_base() != Register::None
        {
            continue;
        }
        let Some(bound) = bound else { continue };
        return Ok(RecordKindDispatch {
            address: jump.ip() as u32,
            table: jump.memory_displacement64() as u32,
            count: bound as usize + 1,
        });
    }
    Err(format!(
        "no `cmp`/`ja`/`jmp [reg*4+table]` dispatch in {RECORD_SECTION_READER:#010x}"
    ))
}

/// Header words observed in the shipped corpus, so the computed record sizes can be listed against
/// the values that actually occur. This is corpus knowledge, not binary knowledge; the sizes beside
/// them are computed from the binary and are free to disagree.
const OBSERVED_HEADER_WORDS: [u32; 19] = [
    63, 73, 76, 79, 81, 87, 89, 96, 97, 98, 101, 102, 105, 106, 107, 108, 109, 110, 111,
];

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.len() != 1 {
        eprintln!("usage: map_loader_survey LOMSE.EXE");
        return ExitCode::from(2);
    }
    let bytes = match std::fs::read(&arguments[0]) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("could not read {}: {error}", arguments[0]);
            return ExitCode::from(2);
        }
    };
    let image = match PeImage::parse(&bytes) {
        Ok(image) => image,
        Err(error) => {
            eprintln!("{} is not a 32-bit PE image: {error}", arguments[0]);
            return ExitCode::from(2);
        }
    };

    if let Err(error) = verify_anchors(&image) {
        eprintln!("anchor check failed: {error}");
        eprintln!(
            "this survey is addressed to one build of lomse.exe; a different build needs the \
             addresses re-derived, not the check relaxed"
        );
        return ExitCode::from(1);
    }
    println!("anchors\tok");

    report_serialisation(&image);
    println!();
    let sizes = report_record_sizes(&image);
    println!();
    let survey = report_cell_tag_bits(&image);

    if sizes.is_empty() || survey.is_empty() {
        eprintln!("survey produced no rows; an empty result is a failed measurement");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------------------------
// Anchors
// ---------------------------------------------------------------------------------------------

/// Check that this executable is the one the addresses were read out of.
///
/// The cheap version of this check would be a file hash, which says nothing about *why* an address
/// is right. These checks are structural: each one asserts the property the analysis depends on.
fn verify_anchors(image: &PeImage<'_>) -> Result<(), String> {
    let operators = OperatorIndex::from_image(image.bytes())
        .map_err(|error| format!("operator table: {error}"))?;
    for name in [
        "resetvisibility",
        "loadmap",
        "loadscenariomap",
        "savescenariomap",
        "savespecialmap",
    ] {
        if !matches!(
            operators.classify(name),
            lom_asset_viewer::native_table::NameClass::Operator { .. }
        ) {
            return Err(format!("the engine does not implement `{name}`"));
        }
    }

    // The scenario object's terrain member is the global map object. Both loaders reach the same
    // reader, so there is one cell array in the process, and `resetvisibility` and the file loader
    // are talking about the same memory.
    if SCENARIO_OBJECT + SCENARIO_TERRAIN_FIELD != MAP_OBJECT {
        return Err("scenario terrain member does not coincide with the map object".to_owned());
    }

    // `resetvisibility` writes 0x0080 into the high half of the cell tag on the map perimeter.
    // That is the only confirmed fact about any tag bit, so it is the check that proves the cell
    // array really is at map object + 0x54.
    let body = disassemble(image, RESET_VISIBILITY_BODY, 400)?;
    let writes_perimeter_flag = body.iter().any(|instruction| {
        instruction.mnemonic() == Mnemonic::Mov
            && instruction.op0_kind() == OpKind::Memory
            && instruction.memory_index_scale() == 8
            && instruction.op1_register() == Register::DX
    });
    if !writes_perimeter_flag {
        return Err(format!(
            "no 16-bit write through an 8-byte stride at {RESET_VISIBILITY_BODY:#010x}"
        ));
    }

    // The terrain reader must read three dwords before the grid: width, height, cell size.
    let reader = trace_stream_fields(image, TERRAIN_READER, STREAM_READ, 200)?;
    if reader.len() < 4 {
        return Err(format!(
            "terrain reader at {TERRAIN_READER:#010x} makes {} stream calls, expected at least 4",
            reader.len()
        ));
    }

    // The accessor must be exactly `mov eax, MAP_OBJECT; ret`. Anything else and treating its
    // return value as the map object would taint an unrelated register in hundreds of functions.
    let accessor = disassemble(image, MAP_OBJECT_ACCESSOR, 4)?;
    let returns_the_object = accessor.first().is_some_and(|instruction| {
        instruction.mnemonic() == Mnemonic::Mov
            && instruction.op0_register() == Register::EAX
            && is_immediate(instruction.op1_kind())
            && instruction.immediate32() == MAP_OBJECT
    }) && accessor
        .get(1)
        .is_some_and(|instruction| instruction.mnemonic() == Mnemonic::Ret);
    if !returns_the_object {
        return Err(format!(
            "{MAP_OBJECT_ACCESSOR:#010x} is not `mov eax, {MAP_OBJECT:#010x}; ret`"
        ));
    }

    // The record-kind dispatch must be recoverable, and its table must hold code addresses.
    let dispatch = recover_record_kind_dispatch(image)?;
    if dispatch.count < 2 || dispatch.count > 64 {
        return Err(format!(
            "record-kind dispatch at {:#010x} claims {} kinds",
            dispatch.address, dispatch.count
        ));
    }
    for kind in 0..dispatch.count {
        let handler = image
            .file_offset(dispatch.table)
            .and_then(|offset| read_u32(image.bytes(), offset + kind * 4))
            .ok_or_else(|| format!("record-kind table at {:#010x} is truncated", dispatch.table))?;
        if !image.is_code_address(handler) {
            return Err(format!(
                "record-kind {kind} points at {handler:#010x}, which is not code"
            ));
        }
    }

    // The record serialiser must gate fields on the scenario object's first dword.
    let serialiser = disassemble(image, RECORD_SERIALISER, 600)?;
    if version_thresholds(&serialiser).is_empty() {
        return Err(format!(
            "no header-word comparisons at {RECORD_SERIALISER:#010x}"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Report 1: the serialisation trace
// ---------------------------------------------------------------------------------------------

/// One recovered call to a stream primitive: how many bytes, and where they land.
#[derive(Debug, Clone)]
struct StreamField {
    address: u32,
    /// `None` when the size or the count was a register rather than an immediate. The terrain
    /// grid is read that way -- its size comes out of the file -- so an unknown here is a finding,
    /// not a gap.
    bytes: Option<u64>,
    destination: String,
}

fn report_serialisation(image: &PeImage<'_>) {
    println!("serialisation-columns\tsite\taddress\tbytes\tdestination");
    let sites: [(&str, u32, u32, usize); 5] = [
        ("scenario-read", SCENARIO_READER, STREAM_READ, 120),
        ("scenario-write", SCENARIO_WRITER, STREAM_WRITE, 120),
        ("terrain-read", TERRAIN_READER, STREAM_READ, 200),
        ("terrain-write", TERRAIN_WRITER, STREAM_WRITE, 200),
        ("record-base-prefix", RECORD_BASE_PREFIX, STREAM_READ, 120),
    ];
    for (label, entry, primitive, limit) in sites {
        match trace_stream_fields(image, entry, primitive, limit) {
            Ok(fields) => {
                for field in fields {
                    println!(
                        "serialisation\t{label}\t{:#010x}\t{}\t{}",
                        field.address,
                        field
                            .bytes
                            .map_or_else(|| "variable".to_owned(), |bytes| bytes.to_string()),
                        field.destination
                    );
                }
            }
            Err(error) => println!("serialisation\t{label}\terror\t{error}"),
        }
    }

    // The record-kind dispatch. Two of the slots point at the error path, which is how the
    // engine says "this kind does not exist" rather than the table being shorter. The
    // count and the kind dword the table is indexed by are read by the section reader itself.
    match trace_stream_fields(image, RECORD_SECTION_READER, STREAM_READ, 60) {
        Ok(fields) => {
            for field in fields.iter().take(2) {
                println!(
                    "serialisation\trecord-section\t{:#010x}\t{}\t{}",
                    field.address,
                    field
                        .bytes
                        .map_or_else(|| "variable".to_owned(), |bytes| bytes.to_string()),
                    field.destination
                );
            }
        }
        Err(error) => println!("serialisation\trecord-section\terror\t{error}"),
    }

    let Ok(dispatch) = recover_record_kind_dispatch(image) else {
        return;
    };
    println!(
        "record-kind-dispatch\t{:#010x}\ttable\t{:#010x}\tkinds\t{}",
        dispatch.address, dispatch.table, dispatch.count
    );
    // The error target is whichever handler the most slots share; the engine points every
    // non-existent kind at one address. Deciding it by majority rather than by naming a slot means
    // a build with a different set of dead kinds still reports them correctly.
    let handlers: Vec<u32> = (0..dispatch.count)
        .filter_map(|kind| {
            image
                .file_offset(dispatch.table)
                .and_then(|offset| read_u32(image.bytes(), offset + kind * 4))
        })
        .collect();
    let mut tally: BTreeMap<u32, usize> = BTreeMap::new();
    for handler in &handlers {
        *tally.entry(*handler).or_default() += 1;
    }
    let error_slot = tally
        .iter()
        .filter(|(_, count)| **count > 1)
        .max_by_key(|(_, count)| **count)
        .map(|(handler, _)| *handler);
    println!("record-kind-columns\tkind\thandler\tvalid");
    for (kind, handler) in handlers.iter().enumerate() {
        println!(
            "record-kind\t{kind}\t{handler:#010x}\t{}",
            Some(*handler) != error_slot
        );
    }
}

/// Recover every call to one stream primitive inside a function, with its size argument.
///
/// The arguments are pushed right to left -- stream, count, size, destination -- so the size is the
/// second push before the call and the destination the first. Reading them back off a running list
/// of pushes is what turns the disassembly into a field list. A call whose arguments are not four
/// recoverable pushes is reported as unknown rather than skipped, because a silently dropped field
/// would read as a shorter record.
fn trace_stream_fields(
    image: &PeImage<'_>,
    entry: u32,
    primitive: u32,
    limit: usize,
) -> Result<Vec<StreamField>, String> {
    let instructions = disassemble(image, entry, limit)?;
    let mut formatter = NasmFormatter::new();
    let mut pushes: Vec<(u64, String)> = Vec::new();
    let mut fields = Vec::new();
    // `lea reg, [this + n]` immediately before a push is how the destination is named, so remember
    // the most recent one per register to render a readable destination.
    let mut addresses: BTreeMap<Register, String> = BTreeMap::new();
    for instruction in &instructions {
        match instruction.mnemonic() {
            Mnemonic::Lea => {
                let mut text = String::new();
                formatter.format(instruction, &mut text);
                addresses.insert(
                    instruction.op0_register(),
                    text.split_once(',').map_or(text.clone(), |(_, rest)| rest.trim().to_owned()),
                );
            }
            Mnemonic::Push => {
                let value = match instruction.op0_kind() {
                    OpKind::Immediate8 | OpKind::Immediate8to32 | OpKind::Immediate32 => {
                        instruction.immediate32() as u64
                    }
                    _ => u64::MAX,
                };
                let name = match instruction.op0_kind() {
                    OpKind::Register => addresses
                        .get(&instruction.op0_register())
                        .cloned()
                        .unwrap_or_else(|| format!("{:?}", instruction.op0_register())),
                    _ => format!("{value:#x}"),
                };
                pushes.push((value, name));
            }
            Mnemonic::Call if instruction.op0_kind() == OpKind::NearBranch32 => {
                if instruction.near_branch32() == primitive && pushes.len() >= 4 {
                    let size = pushes[pushes.len() - 2].0;
                    let destination = pushes[pushes.len() - 1].1.clone();
                    let count = pushes[pushes.len() - 3].0;
                    let bytes = (size != u64::MAX && count != u64::MAX)
                        .then(|| size.saturating_mul(count));
                    fields.push(StreamField {
                        address: instruction.ip() as u32,
                        bytes,
                        destination,
                    });
                }
                pushes.clear();
                addresses.clear();
            }
            _ => {}
        }
    }
    Ok(fields)
}

// ---------------------------------------------------------------------------------------------
// Report 2: the trailing record size, per header word
// ---------------------------------------------------------------------------------------------

/// Every header-word threshold the record serialiser compares against, in address order.
fn version_thresholds(instructions: &[Instruction]) -> Vec<(u32, u32)> {
    let mut thresholds = Vec::new();
    let mut version_register: Option<Register> = None;
    for instruction in instructions {
        match instruction.mnemonic() {
            Mnemonic::Mov
                if instruction.op0_kind() == OpKind::Register
                    && instruction.op1_kind() == OpKind::Memory
                    && is_scenario_version_operand(instruction) =>
            {
                version_register = Some(instruction.op0_register());
            }
            Mnemonic::Cmp if instruction.op1_kind() != OpKind::Memory => {
                let against_version = (instruction.op0_kind() == OpKind::Memory
                    && is_scenario_version_operand(instruction))
                    || (instruction.op0_kind() == OpKind::Register
                        && Some(instruction.op0_register()) == version_register);
                if against_version && is_immediate(instruction.op1_kind()) {
                    thresholds.push((instruction.ip() as u32, instruction.immediate32()));
                }
            }
            _ => {}
        }
    }
    thresholds
}

/// Whether a memory operand names the scenario object's first dword -- the header word.
fn is_scenario_version_operand(instruction: &Instruction) -> bool {
    instruction.memory_base() == Register::None
        && instruction.memory_index() == Register::None
        && instruction.memory_displacement64() == u64::from(SCENARIO_OBJECT)
}

/// Compute the trailing record's byte size for one header word by walking the serialiser.
///
/// The walk is deterministic on exactly the branches that depend on the header word; every other
/// conditional branch falls through. That is an assumption and it is the load-bearing one: the
/// branches it declines are the ones that attach a variable-length sub-object to a record, and the
/// whole shipped corpus has none. If a corpus file ever disagrees with a size computed here, this
/// assumption is the first thing to doubt -- which is exactly why the sizes are reported rather
/// than asserted against constants.
fn record_size_for_version(image: &PeImage<'_>, version: u32) -> Result<u64, String> {
    // Four bytes for the record kind, which the section reader consumes before dispatching.
    let mut total: u64 = 4;
    total += straight_line_bytes(image, RECORD_BASE_PREFIX, None)?;
    total += straight_line_bytes(image, RECORD_SERIALISER, Some(version))?;
    Ok(total)
}

/// Sum the bytes read along one path through a function.
fn straight_line_bytes(
    image: &PeImage<'_>,
    entry: u32,
    version: Option<u32>,
) -> Result<u64, String> {
    let instructions = disassemble(image, entry, 900)?;
    let index: BTreeMap<u64, usize> = instructions
        .iter()
        .enumerate()
        .map(|(position, instruction)| (instruction.ip(), position))
        .collect();

    let mut pushes: Vec<u64> = Vec::new();
    let mut total: u64 = 0;
    let mut position = 0_usize;
    let mut version_register: Option<Register> = None;
    let mut pending: Option<(u32, u32)> = None;
    let mut steps = 0_usize;
    while position < instructions.len() {
        steps += 1;
        if steps > 4000 {
            return Err(format!("walk of {entry:#010x} did not terminate"));
        }
        let instruction = &instructions[position];
        match instruction.mnemonic() {
            Mnemonic::Ret => break,
            Mnemonic::Mov
                if instruction.op0_kind() == OpKind::Register
                    && instruction.op1_kind() == OpKind::Memory
                    && is_scenario_version_operand(instruction) =>
            {
                version_register = Some(instruction.op0_register());
            }
            Mnemonic::Cmp => {
                let against_version = (instruction.op0_kind() == OpKind::Memory
                    && is_scenario_version_operand(instruction))
                    || (instruction.op0_kind() == OpKind::Register
                        && Some(instruction.op0_register()) == version_register);
                pending = (against_version && is_immediate(instruction.op1_kind()))
                    .then(|| (instruction.ip() as u32, instruction.immediate32()));
            }
            Mnemonic::Push => {
                pushes.push(match instruction.op0_kind() {
                    OpKind::Immediate8 | OpKind::Immediate8to32 | OpKind::Immediate32 => {
                        instruction.immediate32() as u64
                    }
                    _ => u64::MAX,
                });
            }
            Mnemonic::Call if instruction.op0_kind() == OpKind::NearBranch32 => {
                if instruction.near_branch32() == STREAM_READ && pushes.len() >= 4 {
                    let size = pushes[pushes.len() - 2];
                    let count = pushes[pushes.len() - 3];
                    if size != u64::MAX && count != u64::MAX {
                        total += size.saturating_mul(count);
                    }
                }
                pushes.clear();
            }
            Mnemonic::Jmp if instruction.op0_kind() == OpKind::NearBranch32 => {
                let Some(next) = index.get(&instruction.near_branch64()) else {
                    break;
                };
                position = *next;
                continue;
            }
            Mnemonic::Jl | Mnemonic::Jge => {
                // The only branches this walk resolves: the header-word gates.
                if let (Some((_, threshold)), Some(version)) = (pending.take(), version) {
                    let less = version < threshold;
                    let taken = if instruction.mnemonic() == Mnemonic::Jl {
                        less
                    } else {
                        !less
                    };
                    if taken {
                        let Some(next) = index.get(&instruction.near_branch64()) else {
                            break;
                        };
                        position = *next;
                        continue;
                    }
                }
            }
            _ => {
                if instruction.mnemonic() != Mnemonic::Cmp {
                    pending = None;
                }
            }
        }
        position += 1;
    }
    Ok(total)
}

fn report_record_sizes(image: &PeImage<'_>) -> Vec<(u32, u64)> {
    let Ok(serialiser) = disassemble(image, RECORD_SERIALISER, 600) else {
        return Vec::new();
    };
    println!("version-gate-columns\taddress\tthreshold");
    for (address, threshold) in version_thresholds(&serialiser) {
        println!("version-gate\t{address:#010x}\t{threshold}");
    }
    // The scenario reader's own gate: the section it guards is outside the record section entirely.
    if let Ok(scenario) = disassemble(image, SCENARIO_READER, 120) {
        for instruction in &scenario {
            if instruction.mnemonic() == Mnemonic::Cmp
                && instruction.op0_kind() == OpKind::Memory
                && instruction.memory_base() != Register::None
                && instruction.memory_displacement64() == 0
                && is_immediate(instruction.op1_kind())
            {
                println!(
                    "scenario-gate\t{:#010x}\t{}",
                    instruction.ip() as u32,
                    instruction.immediate32()
                );
            }
        }
    }

    println!("record-size-columns\theader-word\trecord-bytes\tobserved-in-corpus");
    let mut sizes = Vec::new();
    let observed: BTreeSet<u32> = OBSERVED_HEADER_WORDS.into_iter().collect();
    let mut interesting: BTreeSet<u32> = observed.clone();
    for (_, threshold) in version_thresholds(&serialiser) {
        interesting.insert(threshold);
        interesting.insert(threshold.saturating_sub(1));
    }
    for version in interesting {
        match record_size_for_version(image, version) {
            Ok(size) => {
                println!(
                    "record-size\t{version}\t{size}\t{}",
                    observed.contains(&version)
                );
                sizes.push((version, size));
            }
            Err(error) => println!("record-size\t{version}\terror\t{error}"),
        }
    }
    sizes
}

// ---------------------------------------------------------------------------------------------
// Report 3: which bits of a cell the engine touches
// ---------------------------------------------------------------------------------------------

/// One instruction that names a cell lane, and the bits it names.
#[derive(Debug, Clone)]
struct CellReference {
    address: u32,
    /// Byte offset within the eight-byte cell, `0..8`.
    lane: u64,
    /// Width of the access in bytes.
    width: u64,
    mnemonic: String,
    /// Bits of the whole 64-bit cell the instruction names, when it carries an immediate.
    mask: Option<u64>,
    /// `direct` when the instruction's own operand is the cell; `one-hop` when the mask lands on a
    /// register loaded from the cell by the immediately preceding instruction.
    tier: &'static str,
    /// `read`, `write` or `read-write`.
    access: &'static str,
    /// How the cell was addressed: `index*8`, `byte-offset` or `cell-pointer`. See [`cell_lane`].
    addressing: &'static str,
    /// How the base register was established: `this` or `field-0x54`. See [`cell_lane`].
    basis: &'static str,
    text: String,
}

fn report_cell_tag_bits(image: &PeImage<'_>) -> Vec<CellReference> {
    let methods = map_object_methods(image);
    println!("map-object-methods\t{}", methods.len());
    if std::env::var_os("MAP_SURVEY_DUMP_METHODS").is_some() {
        for method in &methods {
            println!("map-object-method\t{method:#010x}");
        }
    }

    // Two kinds of analysis entry. A map-object method starts with the object in `ecx`. An
    // accessor call site is a point *inside* some other function where the object lands in `eax`;
    // those functions are not in the method set at all, and hundreds of them exist, so without
    // this the whole third access route would contribute nothing to the survey.
    let accessor_sites = accessor_call_sites(image);
    let cell_pointer_sites = absolute_reference_sites(image, MAP_OBJECT + MAP_CELLS_FIELD);
    println!("map-object-accessor-call-sites\t{}", accessor_sites.len());
    println!(
        "map-cell-array-absolute-loads\t{}",
        cell_pointer_sites.len()
    );
    let entries: Vec<(u32, TaintState)> = methods
        .iter()
        .map(|method| (*method, TaintState::at_entry()))
        .chain(
            accessor_sites
                .iter()
                .chain(cell_pointer_sites.iter())
                .map(|site| (*site, TaintState::empty())),
        )
        .collect();

    let mut references: Vec<CellReference> = Vec::new();
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut unclassified: BTreeSet<u32> = BTreeSet::new();
    let mut unreached_blocks = 0_usize;
    let mut truncated = 0_usize;
    let mut undecodable = 0_usize;
    for (entry, seed) in &entries {
        let Ok(instructions) = disassemble(image, *entry, DECODE_LIMIT) else {
            undecodable += 1;
            continue;
        };
        let (found, dropped, unreached) = cell_references(&instructions, seed.clone());
        unreached_blocks += unreached;
        if instructions.len() >= DECODE_LIMIT {
            truncated += 1;
        }
        for reference in found {
            if seen.insert(reference.address) {
                references.push(reference);
            }
        }
        unclassified.extend(dropped);
    }
    references.sort_by_key(|reference| reference.address);
    // Addressed through the cell array but not decoded. A non-zero count here is the warning that
    // the negative results below are negatives over an incomplete set.
    unclassified.retain(|address| !seen.contains(address));
    println!("cell-unclassified\t{}", unclassified.len());
    for address in &unclassified {
        println!("cell-unclassified-site\t{address:#010x}");
    }

    // `cell-unclassified` counts operands the taint *reached* and could not decode. It says nothing
    // about operands the analysis never reached at all, which is a different and larger hole. These
    // three count the parts of that hole which are countable; the parts that are not are named in
    // the scope note printed at the end.
    println!("survey-entries\t{}", entries.len());
    println!("survey-entries-undecodable\t{undecodable}");
    println!("survey-functions-truncated-at-{DECODE_LIMIT}-instructions\t{truncated}");
    println!("survey-unreached-blocks-with-cell-shaped-operands\t{unreached_blocks}");
    // The taint-independent form of the result. See `masked_strided_operands`.
    let masked = masked_strided_operands(image, &entries);
    println!("survey-masked-stride8-operands-ignoring-taint\t{}", masked.len());
    for address in &masked {
        println!("survey-masked-stride8-site\t{address:#010x}");
    }

    println!(
        "cell-reference-columns\taddress\tlane\twidth\ttier\taccess\taddressing\tbasis\tmask\tinstruction"
    );
    for reference in &references {
        println!(
            "cell-reference\t{:#010x}\t+{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            reference.address,
            reference.lane,
            reference.width,
            reference.tier,
            reference.access,
            reference.addressing,
            reference.basis,
            reference
                .mask
                .map_or_else(|| "-".to_owned(), |mask| format!("{mask:#018x}")),
            reference.text,
        );
    }

    // How the surveyed sites were reached. Printed because the three addressing forms and the two
    // bases do not carry equal weight: an `index*8` operand off a register proven to hold the map
    // object is the strongest kind of site, and a `byte-offset` operand off a `field-0x54` load is
    // the weakest. A negative result should be readable against that split rather than as one
    // undifferentiated count.
    let mut provenance: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for reference in &references {
        *provenance
            .entry((reference.addressing, reference.basis))
            .or_default() += 1;
    }
    println!("cell-provenance-columns\taddressing\tbasis\tsites");
    for ((addressing, basis), count) in &provenance {
        println!("cell-provenance\t{addressing}\t{basis}\t{count}");
    }

    // Byte-lane coverage: which of the eight bytes any instruction reaches at all, and how wide
    // the access is. This is the weaker but broader half -- a `mov` of a whole dword names no bit
    // in particular, but it does prove the lane is live.
    let mut by_lane: BTreeMap<u64, BTreeSet<(u64, &'static str)>> = BTreeMap::new();
    for reference in &references {
        if reference.tier == "direct" {
            by_lane
                .entry(reference.lane)
                .or_default()
                .insert((reference.width, "access"));
        }
    }
    let census = surveyed_lane_census(image, &methods);
    println!(
        "cell-lane-columns\tbyte\tword\treads\twrites\twidths\tstride8-operands-in-surveyed-methods"
    );
    for lane in 0..8_u64 {
        let of_lane = |access: &str| {
            references
                .iter()
                .filter(|reference| {
                    reference.tier == "direct"
                        && reference.lane == lane
                        && reference.access == access
                })
                .count()
        };
        let widths: Vec<String> = by_lane
            .get(&lane)
            .map(|set| set.iter().map(|(width, _)| width.to_string()).collect())
            .unwrap_or_default();
        println!(
            "cell-lane\t{lane}\t{}\t{}\t{}\t{}\t{}",
            lane / 4,
            of_lane("read"),
            of_lane("write"),
            if widths.is_empty() { "-".to_owned() } else { widths.join(",") },
            census.get(&lane).copied().unwrap_or(0),
        );
    }

    // The bit map. A bit is "referenced" when some instruction's immediate names it, which is a
    // weaker statement than "the engine means something by it" -- one instruction is one
    // instruction. Bits with no reference at all are the stronger half of this report.
    let mut by_bit: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for reference in &references {
        let Some(mask) = reference.mask else { continue };
        for bit in 0..64_u32 {
            if mask & (1_u64 << bit) != 0 {
                by_bit.entry(bit).or_default().push(reference.address);
            }
        }
    }
    println!("cell-bit-columns\tword\tbit\treferences\tsites");
    for bit in 0..64_u32 {
        let sites = by_bit.get(&bit).map(Vec::as_slice).unwrap_or(&[]);
        let rendered = if sites.is_empty() {
            "-".to_owned()
        } else {
            sites
                .iter()
                .map(|address| format!("{address:#010x}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        println!(
            "cell-bit\t{}\t{}\t{}\t{}",
            bit / 32,
            bit % 32,
            sites.len(),
            rendered
        );
    }

    // Floating-point reads of the second word, which is what would settle whether it is a float.
    let mut float_sites = 0_usize;
    for reference in &references {
        if reference.lane == 4 && reference.mnemonic.starts_with('f') {
            float_sites += 1;
        }
    }
    println!("cell-word1-fpu-sites\t{float_sites}");

    // The ceiling on every negative result above, stated where the negatives are printed.
    //
    // Both discovery paths require `op0_kind() == NearBranch32`, so **virtual dispatch is entirely
    // uncovered**: a method reached only through a vtable slot is not in the surveyed set and its
    // cell accesses are not in these counts. Recovering vtables is the next step, not a caveat to
    // wave at. Until then the honest form of the no-masks finding is scoped to directly-called
    // map-object methods.
    let indirect = indirect_call_sites(image, &methods);
    println!("cell-survey-ceiling\tdirectly-called-map-object-methods-only");
    // Loss paths that cannot be counted, named so the negative is read against them. An operand
    // lost to any of these is absent from both the results and every counter above.
    for path in [
        "object-pointer-spilled-to-a-stack-slot-and-reloaded",
        "object-or-cell-pointer-passed-as-a-function-argument",
        "cell-pointer-stored-into-another-object-field",
        "pointer-arithmetic-forms-the-taint-does-not-model",
        "access-routes-to-the-object-beyond-the-four-found",
    ] {
        println!("cell-survey-uncounted-loss-path\t{path}");
    }
    println!("cell-survey-indirect-calls-in-surveyed-methods\t{indirect}");
    references
}

/// Every instruction that names an absolute address, by address.
///
/// Used for the cell-array pointer at `MAP_OBJECT + 0x54`. **This is the fourth route to the cells**
/// and the widest-reaching: code that needs the grid and nothing else loads the pointer straight out
/// of the global with `mov ecx, [0x5ae9ac]`, touching neither the object's address nor the accessor.
/// Three `+2` readers live in functions reached only this way -- `0x004c5cc2`, `0x00517b67` and
/// `0x00519ced` -- and none of the first three discovery routes can see any of them.
///
/// Found by scanning the raw section bytes for the four-byte constant and then trying to decode an
/// instruction that *covers* the hit, over every start offset that could produce one. Decoding
/// around the constant rather than assuming an encoding is what makes this independent of whether
/// the operand is `8b 0d`, `a1`, or a form with a prefix.
fn absolute_reference_sites(image: &PeImage<'_>, target: u32) -> BTreeSet<u32> {
    /// Longest backward reach tried when looking for the instruction covering a constant. An x86
    /// instruction is at most 15 bytes, and a `disp32` cannot start more than 11 bytes into one.
    const MAXIMUM_LEAD: usize = 11;

    let mut sites: BTreeSet<u32> = BTreeSet::new();
    let needle = target.to_le_bytes();
    for (start, length) in image.executable_ranges() {
        let Some(offset) = image.file_offset(start) else {
            continue;
        };
        let end = (offset + length as usize).min(image.bytes().len());
        let Some(section) = image.bytes().get(offset..end) else {
            continue;
        };
        for position in 0..section.len().saturating_sub(4) {
            if section[position..position + 4] != needle {
                continue;
            }
            for lead in 1..=MAXIMUM_LEAD {
                if lead > position {
                    break;
                }
                let address = start + (position - lead) as u32;
                let Ok(window) = disassemble_window(image, address, 1) else {
                    continue;
                };
                let Some(instruction) = window.first() else {
                    continue;
                };
                // The instruction must reach the constant, and must reach it as an absolute
                // memory operand rather than happening to span those bytes.
                if instruction.len() <= lead {
                    continue;
                }
                if instruction.memory_base() == Register::None
                    && instruction.memory_index() == Register::None
                    && instruction.memory_displacement64() == u64::from(target)
                {
                    sites.insert(address);
                    break;
                }
            }
        }
    }
    sites
}

/// Every direct `call` to the map-object accessor, by address.
///
/// Found by a raw byte scan for `E8` and a relative target equal to the accessor, then confirmed by
/// decoding at the hit -- so a coincidental `E8` inside data or inside a longer instruction cannot
/// contribute. A byte scan rather than a linear disassembly for the reason this file has already
/// been bitten by: a linear decode of a whole section drifts out of phase on embedded data and
/// silently loses everything after it.
fn accessor_call_sites(image: &PeImage<'_>) -> BTreeSet<u32> {
    let mut sites: BTreeSet<u32> = BTreeSet::new();
    for (start, length) in image.executable_ranges() {
        let Some(offset) = image.file_offset(start) else {
            continue;
        };
        let end = (offset + length as usize).min(image.bytes().len());
        let Some(section) = image.bytes().get(offset..end) else {
            continue;
        };
        for position in 0..section.len().saturating_sub(5) {
            if section[position] != 0xE8 {
                continue;
            }
            let Some(relative) = read_u32(section, position + 1) else {
                continue;
            };
            let address = start + position as u32;
            // A `call rel32` is five bytes, so the target is relative to the next instruction.
            if address
                .wrapping_add(5)
                .wrapping_add(relative)
                != MAP_OBJECT_ACCESSOR
            {
                continue;
            }
            let Ok(window) = disassemble_window(image, address, 1) else {
                continue;
            };
            let confirmed = window.first().is_some_and(|instruction| {
                instruction.mnemonic() == Mnemonic::Call
                    && instruction.op0_kind() == OpKind::NearBranch32
                    && instruction.near_branch32() == MAP_OBJECT_ACCESSOR
            });
            if confirmed {
                sites.insert(address);
            }
        }
    }
    sites
}

/// Every function the engine calls as a method of the global map object.
///
/// A call is a method of the map object when `ecx` was loaded with the object's address and nothing
/// reloaded `ecx` before the call. That is the MSVC `thiscall` convention, and it is the filter that
/// keeps unrelated eight-byte-stride arrays -- of which the binary has several -- out of the survey.
fn map_object_methods(image: &PeImage<'_>) -> BTreeSet<u32> {
    let mut methods: BTreeSet<u32> = BTreeSet::new();

    // Find the call sites by searching the raw section bytes for the two encodings that put the
    // map object in `ecx`, then decoding forward from each hit rather than decoding the section
    // linearly from its start. A linear decode drifts out of phase on the first embedded jump
    // table and then silently misses every call site after it.
    //
    // **Two encodings, not one.** `mov ecx, 0x005ae958` is the obvious one. But the map object is
    // also the scenario object's terrain member, so code that already holds the scenario object
    // reaches it as `lea ecx, [reg + 0x482c]` -- which never mentions `0x005ae958` at all and is
    // invisible to a scan keyed on that constant. Missing this form cost nine methods, one of
    // which contains a plain eight-byte-stride `+2` write the lane filter would have caught.
    //
    // A probe for `mov ecx, [reg + 0x482c]` finds nothing and is not evidence either way: the
    // member is a subobject, so its address is taken with `lea`, never loaded with `mov`.
    let mut patterns: Vec<Vec<u8>> = Vec::new();
    // mov ecx, imm32
    let mut mov_immediate = vec![0xB9_u8];
    mov_immediate.extend_from_slice(&MAP_OBJECT.to_le_bytes());
    patterns.push(mov_immediate);
    // lea ecx, [reg + disp32], for every base register encoding. ModRM = 0b10_001_rrr: mod=10
    // (disp32), reg=ecx, rm=the base. `esp` (rm=100) needs a SIB byte and `ebp` (rm=101) is the
    // plain disp32 form, so both are handled by the decode check rather than the pattern.
    for register in 0..8_u8 {
        if register == 4 {
            continue;
        }
        let mut lea = vec![0x8D_u8, 0x88 | register];
        lea.extend_from_slice(&SCENARIO_TERRAIN_FIELD.to_le_bytes());
        patterns.push(lea);
    }

    for (start, length) in image.executable_ranges() {
        let Some(offset) = image.file_offset(start) else {
            continue;
        };
        let end = (offset + length as usize).min(image.bytes().len());
        let Some(section) = image.bytes().get(offset..end) else {
            continue;
        };
        for pattern in &patterns {
            for position in 0..section.len().saturating_sub(pattern.len()) {
                if &section[position..position + pattern.len()] != pattern.as_slice() {
                    continue;
                }
                let address = start + position as u32;
                let Ok(window) = disassemble_window(image, address, 12) else {
                    continue;
                };
                // Confirm the hit really decodes as one of the two forms, so a byte coincidence
                // inside a longer instruction cannot introduce a bogus method.
                let Some(first) = window.first() else { continue };
                let loads_the_object = match first.mnemonic() {
                    Mnemonic::Mov => {
                        first.op0_register() == Register::ECX
                            && is_immediate(first.op1_kind())
                            && first.immediate32() == MAP_OBJECT
                    }
                    Mnemonic::Lea => {
                        first.op0_register() == Register::ECX
                            && first.memory_base() != Register::None
                            && first.memory_index() == Register::None
                            && first.memory_displacement64() == u64::from(SCENARIO_TERRAIN_FIELD)
                    }
                    _ => false,
                };
                if !loads_the_object {
                    continue;
                }
                for instruction in window.iter().skip(1) {
                    if instruction.mnemonic() == Mnemonic::Call {
                        if instruction.op0_kind() == OpKind::NearBranch32 {
                            methods.insert(instruction.near_branch32());
                        }
                        break;
                    }
                    // Anything that redefines `ecx` before the call means the object was not the
                    // `this` pointer of that call.
                    if instruction.op0_kind() == OpKind::Register
                        && instruction.op0_register().full_register32() == Register::ECX
                    {
                        break;
                    }
                }
            }
        }
    }

    // Route three: `call MAP_OBJECT_ACCESSOR` leaves the object in `eax`, and a caller that then
    // moves it to `ecx` and calls is invoking a map-object method. Neither byte pattern above can
    // see any of these, because such a call site contains neither the literal nor the `lea`.
    for site in accessor_call_sites(image) {
        let Ok(window) = disassemble_window(image, site, 16) else {
            continue;
        };
        // Registers currently holding the object. `eax` does, immediately after the call.
        let mut holders: BTreeSet<Register> = BTreeSet::from([Register::EAX]);
        for instruction in window.iter().skip(1) {
            match instruction.mnemonic() {
                Mnemonic::Call => {
                    if holders.contains(&Register::ECX)
                        && instruction.op0_kind() == OpKind::NearBranch32
                    {
                        methods.insert(instruction.near_branch32());
                    }
                    // A call clobbers the volatile registers, so the object survives only where it
                    // was moved into a callee-saved one.
                    for volatile in [Register::EAX, Register::ECX, Register::EDX] {
                        holders.remove(&volatile);
                    }
                    if holders.is_empty() {
                        break;
                    }
                }
                Mnemonic::Mov
                    if instruction.op0_kind() == OpKind::Register
                        && instruction.op1_kind() == OpKind::Register =>
                {
                    let destination = instruction.op0_register().full_register32();
                    if holders.contains(&instruction.op1_register().full_register32()) {
                        holders.insert(destination);
                    } else {
                        holders.remove(&destination);
                    }
                }
                _ => {
                    if instruction.op0_kind() == OpKind::Register {
                        holders.remove(&instruction.op0_register().full_register32());
                    }
                }
            }
        }
    }

    // The reader, the writer and the visibility reset are methods of this object by construction;
    // seed them so a missed call site cannot silently shrink the survey.
    for seed in [
        RESET_VISIBILITY_BODY,
        TERRAIN_READER,
        TERRAIN_WRITER,
        MAP_ALLOCATOR,
    ] {
        methods.insert(seed);
    }

    // A method that forwards its own `this` to another function has handed the map object on, so
    // that function is a map-object method too. Without this round the survey sees only the first
    // layer and reports a suspiciously small set of cell accesses -- which would read as "the
    // engine barely touches the cell tag" when it means "the analysis stopped early".
    let mut converged = false;
    for _ in 0..MAP_METHOD_ROUNDS {
        let mut discovered: BTreeSet<u32> = BTreeSet::new();
        for method in &methods {
            let Ok(body) = disassemble(image, *method, 1200) else {
                continue;
            };
            let mut this_registers: BTreeSet<Register> = BTreeSet::from([Register::ECX]);
            let mut ecx_is_this = true;
            for instruction in &body {
                match instruction.mnemonic() {
                    Mnemonic::Call => {
                        if ecx_is_this && instruction.op0_kind() == OpKind::NearBranch32 {
                            discovered.insert(instruction.near_branch32());
                        }
                        this_registers.remove(&Register::ECX);
                        ecx_is_this = false;
                    }
                    Mnemonic::Mov
                        if instruction.op0_kind() == OpKind::Register
                            && instruction.op1_kind() == OpKind::Register =>
                    {
                        let destination = instruction.op0_register().full_register32();
                        if this_registers.contains(&instruction.op1_register().full_register32()) {
                            this_registers.insert(destination);
                        } else {
                            this_registers.remove(&destination);
                        }
                        ecx_is_this = this_registers.contains(&Register::ECX);
                    }
                    _ => {
                        if instruction.op0_kind() == OpKind::Register {
                            this_registers
                                .remove(&instruction.op0_register().full_register32());
                        }
                        ecx_is_this = this_registers.contains(&Register::ECX);
                    }
                }
            }
        }
        let before = methods.len();
        methods.extend(discovered);
        if methods.len() == before {
            converged = true;
            break;
        }
    }
    if !converged {
        println!(
            "survey-discovery-did-not-converge-in\t{MAP_METHOD_ROUNDS}\trounds"
        );
    }
    methods
}

/// Cell-lane references inside one function.
///
/// The cell array is reached as `mov reg, [this + 0x54]`; from there a cell lane is any memory
/// operand based on that register with an eight-byte index scale. Only registers that were loaded
/// from `+0x54` count, which is what stops the survey drifting onto the other eight-byte array the
/// same object holds at `+0x64`.
fn cell_references(
    instructions: &[Instruction],
    seed: TaintState,
) -> (Vec<CellReference>, Vec<u32>, usize) {
    let positions: BTreeMap<u64, usize> = instructions
        .iter()
        .enumerate()
        .map(|(position, instruction)| (instruction.ip(), position))
        .collect();

    // Basic-block leaders: the entry, every in-range branch target, and every instruction that
    // follows a branch or a return.
    let mut leaders: BTreeSet<usize> = BTreeSet::from([0]);
    for (position, instruction) in instructions.iter().enumerate() {
        let is_branch = matches!(
            instruction.flow_control(),
            iced_x86::FlowControl::ConditionalBranch | iced_x86::FlowControl::UnconditionalBranch
        );
        if is_branch
            && instruction.op0_kind() == OpKind::NearBranch32
            && let Some(target) = positions.get(&instruction.near_branch64())
        {
            leaders.insert(*target);
        }
        if is_branch || instruction.flow_control() == iced_x86::FlowControl::Return {
            leaders.insert(position + 1);
        }
    }
    leaders.retain(|leader| *leader < instructions.len());
    let blocks: Vec<usize> = leaders.into_iter().collect();
    let block_of = |position: usize| match blocks.binary_search(&position) {
        Ok(index) => index,
        Err(index) => index.saturating_sub(1),
    };

    // Forward must-analysis. A register is treated as holding the cell array, a cell pointer or
    // the map object only where that is true on *every* path reaching the instruction, so the
    // merge is an intersection. A linear walk instead of this reported a third of the cell
    // accesses, because the taint died at every early-return epilogue it walked through.
    let mut entry_states: Vec<Option<TaintState>> = vec![None; blocks.len()];
    entry_states[0] = Some(seed);
    let mut worklist: Vec<usize> = vec![0];
    let mut rounds = 0_usize;
    while let Some(block) = worklist.pop() {
        rounds += 1;
        if rounds > TAINT_ROUND_LIMIT {
            break;
        }
        let Some(mut state) = entry_states[block].clone() else {
            continue;
        };
        let first = blocks[block];
        let last = blocks.get(block + 1).copied().unwrap_or(instructions.len());
        let mut successors: Vec<usize> = Vec::new();
        for instruction in &instructions[first..last] {
            step(&mut state, instruction, &mut None);
            if instruction.op0_kind() == OpKind::NearBranch32
                && let Some(target) = positions.get(&instruction.near_branch64())
            {
                successors.push(block_of(*target));
            }
        }
        let falls_through = instructions
            .get(last - 1)
            .is_some_and(|instruction| {
                !matches!(
                    instruction.flow_control(),
                    iced_x86::FlowControl::Return | iced_x86::FlowControl::UnconditionalBranch
                )
            });
        if falls_through && block + 1 < blocks.len() {
            successors.push(block + 1);
        }
        for successor in successors {
            let merged = match &entry_states[successor] {
                None => Some(state.clone()),
                Some(existing) => existing.intersect(&state),
            };
            if let Some(merged) = merged {
                entry_states[successor] = Some(merged);
                worklist.push(successor);
            }
        }
    }

    let mut references = Vec::new();
    let mut unclassified = Vec::new();
    for (block, entry) in entry_states.iter().enumerate() {
        let Some(entry) = entry else { continue };
        let mut state = entry.clone();
        let first = blocks[block];
        let last = blocks.get(block + 1).copied().unwrap_or(instructions.len());
        let mut emitted = Some((Vec::new(), Vec::new()));
        for instruction in &instructions[first..last] {
            step(&mut state, instruction, &mut emitted);
        }
        let (block_references, block_unclassified) = emitted.unwrap_or_default();
        references.extend(block_references);
        unclassified.extend(block_unclassified);
    }
    // Blocks the dataflow never assigned an entry state are blocks this analysis did not look at
    // at all -- reachable in the real program only through an indirect jump, or through a path the
    // successor computation does not model, or (most of them) simply lying past the end of the
    // function because the decoder overshot.
    //
    // A raw count of those is a bad instrument: raising the decode limit inflates it, because it is
    // dominated by junk past the function end. What matters is how many of them contain something
    // *shaped like* a cell access, because those are the ones that could have changed a result. A
    // count of zero there is a real statement; a count of zero blocks is not.
    let unreached = entry_states
        .iter()
        .enumerate()
        .filter(|(_, state)| state.is_none())
        .filter(|(block, _)| {
            let first = blocks[*block];
            let last = blocks.get(block + 1).copied().unwrap_or(instructions.len());
            instructions[first..last].iter().any(|instruction| {
                instruction.memory_index_scale() == 8
                    || (instruction.op1_kind() == OpKind::Memory
                        && instruction.memory_index() == Register::None
                        && instruction.memory_displacement64() == u64::from(MAP_CELLS_FIELD))
            })
        })
        .count();
    (references, unclassified, unreached)
}

/// Which registers hold the map object, the cell array, or a pointer into one cell.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TaintState {
    this_registers: BTreeSet<Register>,
    /// Loaded from `+0x54` of a register **proven** to hold the map object on every path here.
    cell_registers: BTreeSet<Register>,
    /// Loaded from `+0x54` of some other register, inside a function already known to be a
    /// map-object method. See [`cell_lane`] for why this weaker basis is kept and labelled rather
    /// than dropped.
    field_registers: BTreeSet<Register>,
    cell_pointers: BTreeMap<Register, u64>,
    /// A register just loaded from a cell lane, for the one-hop mask rule.
    loaded: Option<(Register, u64, u64)>,
}

impl TaintState {
    /// At a `thiscall` entry point, `ecx` is the object and nothing else is known.
    fn at_entry() -> Self {
        Self {
            this_registers: BTreeSet::from([Register::ECX]),
            cell_registers: BTreeSet::new(),
            field_registers: BTreeSet::new(),
            cell_pointers: BTreeMap::new(),
            loaded: None,
        }
    }

    /// At an arbitrary point inside a function, nothing is known. Used when the analysis starts at
    /// a `call MAP_OBJECT_ACCESSOR` rather than at a function entry: `ecx` there is whatever the
    /// surrounding code was doing, so seeding it as the object would invent cell accesses.
    fn empty() -> Self {
        Self {
            this_registers: BTreeSet::new(),
            cell_registers: BTreeSet::new(),
            field_registers: BTreeSet::new(),
            cell_pointers: BTreeMap::new(),
            loaded: None,
        }
    }

    /// The must-merge of two states, or `None` when it equals what is already there.
    fn intersect(&self, other: &Self) -> Option<Self> {
        let merged = Self {
            this_registers: self.this_registers.intersection(&other.this_registers).copied().collect(),
            cell_registers: self.cell_registers.intersection(&other.cell_registers).copied().collect(),
            field_registers: self
                .field_registers
                .intersection(&other.field_registers)
                .copied()
                .collect(),
            cell_pointers: self
                .cell_pointers
                .iter()
                .filter(|(register, lane)| other.cell_pointers.get(register) == Some(lane))
                .map(|(register, lane)| (*register, *lane))
                .collect(),
            // A pending load never survives a control-flow merge: the one-hop rule only means
            // anything for two adjacent instructions.
            loaded: None,
        };
        (&merged != self).then_some(merged)
    }
}

/// Apply one instruction to the taint state, optionally recording the cell references it makes.
fn step(
    state: &mut TaintState,
    instruction: &Instruction,
    emitted: &mut Option<(Vec<CellReference>, Vec<u32>)>,
) {
    if let Some((sink, unclassified)) = emitted.as_mut() {
        let mut formatter = NasmFormatter::new();
        let mut info = InstructionInfoFactory::new();
        let mut text = String::new();
        formatter.format(instruction, &mut text);

        match cell_lane(
            instruction,
            &state.cell_registers,
            &state.field_registers,
            &state.cell_pointers,
        ) {
            LaneClass::Lane {
                lane,
                width,
                addressing,
                basis,
            } => sink.push(CellReference {
                address: instruction.ip() as u32,
                lane,
                width,
                mnemonic: format!("{:?}", instruction.mnemonic()).to_lowercase(),
                mask: immediate_mask(instruction, lane, width),
                tier: "direct",
                access: lane_access(&mut info, instruction),
                addressing,
                basis,
                text: text.clone(),
            }),
            LaneClass::Unclassified => unclassified.push(instruction.ip() as u32),
            LaneClass::Elsewhere => {}
        }

        if let Some((register, lane, width)) = state.loaded
            && instruction.op0_kind() == OpKind::Register
            && instruction.op0_register().full_register32() == register.full_register32()
            && matches!(
                instruction.mnemonic(),
                Mnemonic::And
                    | Mnemonic::Test
                    | Mnemonic::Or
                    | Mnemonic::Xor
                    | Mnemonic::Shr
                    | Mnemonic::Sar
            )
            && is_immediate(instruction.op1_kind())
        {
            // A byte- or word-sized operand names bits inside the loaded value, offset by which
            // part of it the sub-register is. `ah` is the only awkward one and it is the one the
            // engine actually uses.
            let sub_shift = if instruction.op0_register() == Register::AH { 8 } else { 0 };
            sink.push(CellReference {
                address: instruction.ip() as u32,
                lane,
                width,
                mnemonic: format!("{:?}", instruction.mnemonic()).to_lowercase(),
                mask: shift_mask(instruction, lane, width, sub_shift),
                tier: "one-hop",
                access: "read",
                addressing: "register",
                basis: "register",
                text,
            });
        }
    }

    let previous_loaded = state.loaded;
    state.loaded = None;
    match instruction.mnemonic() {
        Mnemonic::Lea if instruction.op0_kind() == OpKind::Register => {
            let destination = instruction.op0_register();
            // `lea` is how the engine reaches a cell it is about to write: compute the address
            // once, then `mov [reg], cx`. Both addressing forms must be carried, including the
            // scale-1 one -- `lea eax,[eax+esi+2]` at 0x004a9393 is followed by the `+2` write at
            // 0x004a93a6, and missing the `lea` loses the write.
            let scale = instruction.memory_index_scale();
            let tainted_base = state.cell_registers.contains(&instruction.memory_base())
                || state.field_registers.contains(&instruction.memory_base());
            let tainted_index = state.cell_registers.contains(&instruction.memory_index())
                || state.field_registers.contains(&instruction.memory_index());
            let displacement = instruction.memory_displacement64() as i64;
            let lane = if (scale == 8 && tainted_base) || (scale == 1 && (tainted_base || tainted_index)) {
                Some(displacement.rem_euclid(8) as u64)
            } else if let Some(base) = state.cell_pointers.get(&instruction.memory_base()) {
                (instruction.memory_index() == Register::None)
                    .then(|| (*base as i64 + displacement).rem_euclid(8) as u64)
            } else {
                None
            };
            forget(destination, state);
            if let Some(lane) = lane {
                state.cell_pointers.insert(destination.full_register32(), lane);
            }
        }
        Mnemonic::Mov if instruction.op0_kind() == OpKind::Register => {
            let destination = instruction.op0_register();
            let source_is_this = instruction.op1_kind() == OpKind::Register
                && state
                    .this_registers
                    .contains(&instruction.op1_register().full_register32());
            let source_is_object =
                is_immediate(instruction.op1_kind()) && instruction.immediate32() == MAP_OBJECT;
            let loads_cells_field = instruction.op1_kind() == OpKind::Memory
                && instruction.memory_index() == Register::None
                && instruction.memory_displacement64() == u64::from(MAP_CELLS_FIELD);
            let source_is_cell_field = (loads_cells_field
                && state.this_registers.contains(&instruction.memory_base()))
                || (instruction.op1_kind() == OpKind::Memory
                    && instruction.memory_index() == Register::None
                    && instruction.memory_base() == Register::None
                    && instruction.memory_displacement64()
                        == u64::from(MAP_OBJECT + MAP_CELLS_FIELD));
            // The weaker basis. Inside a function already established as a map-object method,
            // `mov r, [b+0x54]` loads this class's cell array even where the must-analysis has
            // lost the proof that `b` is the object -- which it does in long functions that spill
            // `this` to the stack and reload it, such as `0x004a8c40`, where `ebx` holds the
            // object but some path redefines it. Dropping these lost four `+2` accesses including
            // the only *read* of that field in the binary, so they are kept and labelled.
            let source_is_object_field = loads_cells_field
                && !source_is_cell_field
                && instruction.memory_base() != Register::None;
            let carried = (instruction.op1_kind() == OpKind::Register)
                .then(|| {
                    state
                        .cell_pointers
                        .get(&instruction.op1_register().full_register32())
                        .copied()
                })
                .flatten();
            let lane = cell_lane(
                instruction,
                &state.cell_registers,
                &state.field_registers,
                &state.cell_pointers,
            );
            forget(destination, state);
            let destination = destination.full_register32();
            if source_is_this || source_is_object {
                state.this_registers.insert(destination);
            } else if source_is_cell_field {
                state.cell_registers.insert(destination);
            } else if source_is_object_field {
                state.field_registers.insert(destination);
            } else if let Some(base) = carried {
                state.cell_pointers.insert(destination, base);
            } else if let LaneClass::Lane { lane, width, .. } = lane {
                state.loaded = Some((destination, lane, width));
            }
        }
        Mnemonic::Movsx | Mnemonic::Movzx if instruction.op0_kind() == OpKind::Register => {
            let destination = instruction.op0_register();
            let lane = cell_lane(
                instruction,
                &state.cell_registers,
                &state.field_registers,
                &state.cell_pointers,
            );
            forget(destination, state);
            if let LaneClass::Lane { lane, width, .. } = lane {
                state.loaded = Some((destination.full_register32(), lane, width));
            }
        }
        Mnemonic::Call => {
            // A call clobbers the volatile registers, so every taint on them is dropped. Doing
            // this at all is new: an earlier version left them alone, which is unsound in the
            // permissive direction.
            for volatile in [Register::EAX, Register::ECX, Register::EDX] {
                forget(volatile, state);
            }
            // ...and a call to the accessor then *defines* `eax` as the map object. This is the
            // third route to the object, and the one that reaches the most code.
            if instruction.op0_kind() == OpKind::NearBranch32
                && instruction.near_branch32() == MAP_OBJECT_ACCESSOR
            {
                state.this_registers.insert(Register::EAX);
            }
        }
        Mnemonic::Test | Mnemonic::Cmp | Mnemonic::Push => {
            // Flag-only and stack instructions define no register, so a load stays live across
            // them. Without this a `mov`/`test`/`and` triple loses the taint at the `test`.
            state.loaded = previous_loaded;
        }
        _ => {
            if instruction.op0_kind() == OpKind::Register {
                forget(instruction.op0_register(), state);
            }
        }
    }
}

/// Drop every taint on a register, including the 32-bit register a sub-register belongs to.
fn forget(register: Register, state: &mut TaintState) {
    let full = register.full_register32();
    state.this_registers.remove(&full);
    state.cell_registers.remove(&full);
    state.field_registers.remove(&full);
    state.cell_pointers.remove(&full);
}

/// What a memory operand turned out to be, relative to the cell grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaneClass {
    /// Not addressed through anything this analysis has tainted.
    Elsewhere,
    /// Byte offset within the eight-byte cell, the access width, and how it was addressed.
    Lane {
        lane: u64,
        width: u64,
        addressing: &'static str,
        /// `this` when the base register was proven to hold the map object on every path reaching
        /// the instruction; `field-0x54` when it was only loaded from some register's `+0x54`
        /// inside a known map-object method.
        basis: &'static str,
    },
    /// Addressed through a tainted register, but the lane could not be decided. **These are the
    /// blind spot.** They are counted and printed, because the first version of this survey
    /// silently returned `Elsewhere` for a whole addressing form the engine uses constantly, and a
    /// negative result over an incomplete set reads exactly like a negative result over a complete
    /// one.
    Unclassified,
}

/// The cell lane a memory operand names.
///
/// Two addressing forms reach a cell and both must be accepted:
///
/// - `[cell_array + index*8 + disp]`, where the index is a cell number. The lane is `disp mod 8`.
/// - `[cell_array + offset + disp]` with **scale 1**, where the register already holds
///   `cell_number * 8`. The engine uses this constantly -- `movsx ecx, word [eax+esi+2]` at
///   `0x004a938e` is one, and `resetvisibility`'s own second perimeter write at `0x004a9178` is
///   another. An earlier version of this program required scale 8 and so dropped every one of
///   them, including a write inside its own anchor.
///
/// The scale-1 form carries an assumption the scale-8 form does not: that the register holds a
/// multiple of the eight-byte stride. Nothing here proves that, so those sites are reported under a
/// distinct `addressing` value rather than being mixed in. A `[cell_array + index*4]` would look
/// identical and would be misreported -- which is why the distinction is printed and not collapsed.
fn cell_lane(
    instruction: &Instruction,
    cell_registers: &BTreeSet<Register>,
    field_registers: &BTreeSet<Register>,
    cell_pointers: &BTreeMap<Register, u64>,
) -> LaneClass {
    // `lea` names a cell address but accesses no cell byte, and has no memory size at all. The
    // taint propagation consumes it; counting it here would report every address computation as an
    // undecodable access.
    if instruction.mnemonic() == Mnemonic::Lea {
        return LaneClass::Elsewhere;
    }
    let has_memory = (0..instruction.op_count())
        .any(|operand| instruction.op_kind(operand) == OpKind::Memory);
    if !has_memory {
        return LaneClass::Elsewhere;
    }
    let base = instruction.memory_base();
    let index = instruction.memory_index();
    let scale = instruction.memory_index_scale();
    // Displacements like `-8` and `-6` appear where the index was pre-incremented; the lane is the
    // displacement modulo the cell stride either way.
    let displacement = instruction.memory_displacement64() as i64;

    let proven_base = cell_registers.contains(&base);
    let proven_index = cell_registers.contains(&index);
    let tainted_base = proven_base || field_registers.contains(&base);
    let tainted_index = proven_index || field_registers.contains(&index);
    let pointer_base = cell_pointers.get(&base).copied();
    let basis = if proven_base || proven_index {
        "this"
    } else {
        "field-0x54"
    };

    let (lane, addressing) = if scale == 8 && tainted_base {
        (displacement.rem_euclid(8) as u64, "index*8")
    } else if scale == 1 && (tainted_base || tainted_index) {
        // Either register may be the array: `[eax+esi+2]` and `[edx+ecx+2]` both occur, with the
        // array in either position.
        (displacement.rem_euclid(8) as u64, "byte-offset")
    } else if let Some(pointer) = pointer_base {
        if index != Register::None {
            return LaneClass::Unclassified;
        }
        ((pointer as i64 + displacement).rem_euclid(8) as u64, "cell-pointer")
    } else if tainted_base || tainted_index {
        // Reached through the cell array but in a shape this does not decode -- a scale of 2 or 4,
        // or an eight-byte scale on the index rather than the base.
        return LaneClass::Unclassified;
    } else {
        return LaneClass::Elsewhere;
    };

    let width = instruction.memory_size().size() as u64;
    if width == 0 || width > 8 {
        return LaneClass::Unclassified;
    }
    LaneClass::Lane {
        lane,
        width,
        addressing,
        basis,
    }
}

/// The bits of the whole cell an instruction with an immediate names.
fn immediate_mask(instruction: &Instruction, lane: u64, width: u64) -> Option<u64> {
    if !matches!(
        instruction.mnemonic(),
        Mnemonic::And | Mnemonic::Test | Mnemonic::Or | Mnemonic::Xor | Mnemonic::Mov
    ) {
        return None;
    }
    let operand = instruction.op_count().checked_sub(1)?;
    if !is_immediate(instruction.op_kind(operand)) {
        return None;
    }
    let value = u64::from(instruction.immediate32());
    let truncated = if width >= 8 { value } else { value & ((1_u64 << (width * 8)) - 1) };
    Some(truncated << (lane * 8))
}

/// The bits a shift or mask on a register loaded from a lane names, in cell coordinates.
fn shift_mask(instruction: &Instruction, lane: u64, width: u64, sub_shift: u32) -> Option<u64> {
    let value = u64::from(instruction.immediate32()) << sub_shift;
    let full = if width >= 8 { u64::MAX } else { (1_u64 << (width * 8)) - 1 };
    let bits = match instruction.mnemonic() {
        Mnemonic::And | Mnemonic::Test | Mnemonic::Or | Mnemonic::Xor => value & full,
        // A right shift by `n` says the bits below `n` were being discarded, which is a reference
        // to those bits and to nothing else that can be named from the shift alone.
        Mnemonic::Shr | Mnemonic::Sar => {
            let amount = value & 31;
            if amount == 0 || amount >= 64 {
                return None;
            }
            (1_u64 << amount) - 1
        }
        _ => return None,
    };
    (bits != 0).then(|| bits << (lane * 8))
}

/// Whether an instruction reads or writes the cell lane its operand names.
///
/// Decided by the decoder's own operand-access information rather than by the mnemonic, because
/// the mnemonic gets it wrong exactly where it matters: for `fld dword [cell+4]` the memory operand
/// is operand zero, so a "first operand is memory means write" rule calls every floating-point
/// *load* of the elevation a store.
fn lane_access(info: &mut InstructionInfoFactory, instruction: &Instruction) -> &'static str {
    let accesses: Vec<OpAccess> = info
        .info(instruction)
        .used_memory()
        .iter()
        .map(|memory| memory.access())
        .collect();
    let reads = accesses
        .iter()
        .any(|access| matches!(access, OpAccess::Read | OpAccess::ReadWrite | OpAccess::CondRead));
    let writes = accesses.iter().any(|access| {
        matches!(
            access,
            OpAccess::Write | OpAccess::ReadWrite | OpAccess::CondWrite
        )
    });
    match (reads, writes) {
        (true, true) => "read-write",
        (false, true) => "write",
        _ => "read",
    }
}

/// Every masking instruction in the surveyed code whose operand is eight-byte strided, **ignoring
/// the taint entirely**.
///
/// This is the most robust form of the no-masks result, because it does not depend on the taint
/// analysis being complete. It over-counts freely -- any eight-byte array in any surveyed function
/// qualifies, and there are several -- so a non-zero answer would need triage. A **zero** answer is
/// a statement no amount of missed taint can weaken: within this code there is no masking
/// instruction against an eight-byte-strided operand at a cell-lane displacement at all, whether or
/// not the analysis could prove the base was the cell array.
fn masked_strided_operands(
    image: &PeImage<'_>,
    entries: &[(u32, TaintState)],
) -> BTreeSet<u32> {
    let mut found: BTreeSet<u32> = BTreeSet::new();
    for (entry, _) in entries {
        let Ok(instructions) = disassemble(image, *entry, DECODE_LIMIT) else {
            continue;
        };
        for instruction in &instructions {
            if instruction.memory_index_scale() != 8
                || instruction.memory_base() == Register::None
                || instruction.mnemonic() == Mnemonic::Lea
            {
                continue;
            }
            let lane = (instruction.memory_displacement64() as i64).rem_euclid(8);
            if !matches!(lane, 0 | 2 | 4) {
                continue;
            }
            let masks = matches!(
                instruction.mnemonic(),
                Mnemonic::And
                    | Mnemonic::Test
                    | Mnemonic::Or
                    | Mnemonic::Xor
                    | Mnemonic::Shr
                    | Mnemonic::Sar
                    | Mnemonic::Bt
                    | Mnemonic::Bts
                    | Mnemonic::Btr
            ) && (0..instruction.op_count())
                .any(|operand| is_immediate(instruction.op_kind(operand)));
            if masks {
                found.insert(instruction.ip() as u32);
            }
        }
    }
    found
}

/// How many calls inside the surveyed methods go through a register or memory operand.
///
/// Every one is a method this survey did not follow. The figure is the size of the hole in the
/// negative results, reported rather than described.
fn indirect_call_sites(image: &PeImage<'_>, methods: &BTreeSet<u32>) -> usize {
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for method in methods {
        let Ok(instructions) = disassemble(image, *method, 1200) else {
            continue;
        };
        for instruction in &instructions {
            if instruction.mnemonic() == Mnemonic::Call
                && instruction.op0_kind() != OpKind::NearBranch32
            {
                seen.insert(instruction.ip() as u32);
            }
        }
    }
    seen.len()
}

/// How many eight-byte-strided memory operands the surveyed methods contain, per lane, regardless
/// of whether the base was tainted.
///
/// **This is the scope denominator, and it is computed from the same per-function decodes the
/// survey itself uses** -- not from a linear decode of the whole `.text`. An earlier version took
/// the figure from a whole-section linear disassembly, which drifts out of phase on embedded jump
/// tables; quoting a deliberately misaligned decode as the denominator of a negative result was
/// wrong in the same direction as the result it was supposed to qualify.
///
/// It is still not evidence. Several unrelated eight-byte arrays live in these functions, the map
/// object's own second array at `+0x64` among them. Its job is to make it obvious when the
/// filtered survey has collapsed because the analysis stopped early rather than because the engine
/// is quiet. [`LaneClass::Unclassified`] is the sharper instrument for that; this is the blunt one.
fn surveyed_lane_census(image: &PeImage<'_>, methods: &BTreeSet<u32>) -> BTreeMap<u64, usize> {
    let mut census: BTreeMap<u64, usize> = BTreeMap::new();
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for method in methods {
        let Ok(instructions) = disassemble(image, *method, 1200) else {
            continue;
        };
        for instruction in &instructions {
            if instruction.memory_index_scale() != 8
                || instruction.memory_base() == Register::None
                || instruction.mnemonic() == Mnemonic::Lea
            {
                continue;
            }
            if !seen.insert(instruction.ip() as u32) {
                continue;
            }
            let lane = (instruction.memory_displacement64() as i64).rem_euclid(8) as u64;
            *census.entry(lane).or_default() += 1;
        }
    }
    census
}

// ---------------------------------------------------------------------------------------------
// Disassembly helpers
// ---------------------------------------------------------------------------------------------

/// Decode a function from its entry point.
///
/// The bound is the furthest forward branch seen so far: decoding stops at the first `ret` or
/// unconditional `jmp` that nothing jumps past. That overshoots into the next function when a
/// function ends in a tail call, and undershoots nothing, which is the safe direction for a survey.
fn disassemble(image: &PeImage<'_>, entry: u32, limit: usize) -> Result<Vec<Instruction>, String> {
    let offset = image
        .file_offset(entry)
        .ok_or_else(|| format!("{entry:#010x} is not mapped"))?;
    let mut decoder = Decoder::with_ip(
        32,
        &image.bytes()[offset..],
        u64::from(entry),
        DecoderOptions::NONE,
    );
    let mut instructions = Vec::new();
    let mut furthest = u64::from(entry);
    for instruction in decoder.iter().take(limit) {
        let terminal = matches!(instruction.mnemonic(), Mnemonic::Ret | Mnemonic::Int3);
        let unconditional = instruction.mnemonic() == Mnemonic::Jmp
            && instruction.op0_kind() == OpKind::NearBranch32;
        if instruction.op0_kind() == OpKind::NearBranch32
            && matches!(
                instruction.flow_control(),
                iced_x86::FlowControl::ConditionalBranch | iced_x86::FlowControl::UnconditionalBranch
            )
        {
            furthest = furthest.max(instruction.near_branch64());
        }
        let ip = instruction.ip();
        instructions.push(instruction);
        if (terminal || unconditional) && ip >= furthest {
            break;
        }
    }
    if instructions.is_empty() {
        return Err(format!("nothing decoded at {entry:#010x}"));
    }
    Ok(instructions)
}

/// Decode a fixed number of instructions from an address, without trying to find a function end.
fn disassemble_window(
    image: &PeImage<'_>,
    address: u32,
    count: usize,
) -> Result<Vec<Instruction>, String> {
    let offset = image
        .file_offset(address)
        .ok_or_else(|| format!("{address:#010x} is not mapped"))?;
    let mut decoder = Decoder::with_ip(
        32,
        &image.bytes()[offset..],
        u64::from(address),
        DecoderOptions::NONE,
    );
    Ok(decoder.iter().take(count).collect())
}

fn is_immediate(kind: OpKind) -> bool {
    matches!(
        kind,
        OpKind::Immediate8
            | OpKind::Immediate8to16
            | OpKind::Immediate8to32
            | OpKind::Immediate16
            | OpKind::Immediate32
    )
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}
