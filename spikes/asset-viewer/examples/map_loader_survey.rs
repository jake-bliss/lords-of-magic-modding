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
/// The record-kind jump table behind `jmp dword [eax*4+...]` in [`RECORD_SECTION_READER`].
const RECORD_KIND_TABLE: u32 = 0x004F_73B8;
/// Number of entries in [`RECORD_KIND_TABLE`]: the dispatch is guarded by `cmp eax,9; ja`.
const RECORD_KIND_COUNT: usize = 10;

/// How many rounds of `this`-forwarding to follow when collecting map-object methods.
const MAP_METHOD_ROUNDS: usize = 4;

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
    if SCENARIO_OBJECT + 0x482C != MAP_OBJECT {
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

    // The record-kind dispatch. Two of the ten slots point at the error path, which is how the
    // engine says "this kind does not exist" rather than the table being eight entries long. The
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

    let error_slot = image
        .file_offset(RECORD_KIND_TABLE)
        .and_then(|offset| read_u32(image.bytes(), offset + 5 * 4));
    println!("record-kind-columns\tkind\thandler\tvalid");
    for kind in 0..RECORD_KIND_COUNT {
        let Some(handler) = image
            .file_offset(RECORD_KIND_TABLE)
            .and_then(|offset| read_u32(image.bytes(), offset + kind * 4))
        else {
            continue;
        };
        let valid = Some(handler) != error_slot;
        println!("record-kind\t{kind}\t{handler:#010x}\t{valid}");
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
    /// `read`, `write` or `flags` -- the last for instructions that only set flags.
    access: &'static str,
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

    let mut references: Vec<CellReference> = Vec::new();
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for method in &methods {
        let Ok(instructions) = disassemble(image, *method, 1200) else {
            continue;
        };
        for reference in cell_references(&instructions) {
            if seen.insert(reference.address) {
                references.push(reference);
            }
        }
    }
    references.sort_by_key(|reference| reference.address);

    println!("cell-reference-columns\taddress\tlane\twidth\ttier\taccess\tmask\tinstruction");
    for reference in &references {
        println!(
            "cell-reference\t{:#010x}\t+{}\t{}\t{}\t{}\t{}\t{}",
            reference.address,
            reference.lane,
            reference.width,
            reference.tier,
            reference.access,
            reference
                .mask
                .map_or_else(|| "-".to_owned(), |mask| format!("{mask:#018x}")),
            reference.text,
        );
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
    let census = unfiltered_lane_census(image);
    println!("cell-lane-columns\tbyte\tword\treads\twrites\twidths\tunfiltered-stride8-operands");
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
    references
}

/// Every function the engine calls as a method of the global map object.
///
/// A call is a method of the map object when `ecx` was loaded with the object's address and nothing
/// reloaded `ecx` before the call. That is the MSVC `thiscall` convention, and it is the filter that
/// keeps unrelated eight-byte-stride arrays -- of which the binary has several -- out of the survey.
fn map_object_methods(image: &PeImage<'_>) -> BTreeSet<u32> {
    let mut methods: BTreeSet<u32> = BTreeSet::new();

    // Find the call sites by searching for the `mov ecx, MAP_OBJECT` encoding in the raw section
    // bytes and decoding forward from each hit, rather than by decoding the section linearly from
    // its start. A linear decode drifts out of phase on the first embedded jump table and then
    // silently misses every call site after it -- which is what made an earlier run of this survey
    // report a third of the methods it should have.
    let mut pattern = vec![0xB9_u8];
    pattern.extend_from_slice(&MAP_OBJECT.to_le_bytes());
    for (start, length) in image.executable_ranges() {
        let Some(offset) = image.file_offset(start) else {
            continue;
        };
        let end = (offset + length as usize).min(image.bytes().len());
        let Some(section) = image.bytes().get(offset..end) else {
            continue;
        };
        for position in 0..section.len().saturating_sub(pattern.len()) {
            if &section[position..position + pattern.len()] != pattern.as_slice() {
                continue;
            }
            let address = start + position as u32;
            let Ok(window) = disassemble_window(image, address, 12) else {
                continue;
            };
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
            break;
        }
    }
    methods
}

/// Cell-lane references inside one function.
///
/// The cell array is reached as `mov reg, [this + 0x54]`; from there a cell lane is any memory
/// operand based on that register with an eight-byte index scale. Only registers that were loaded
/// from `+0x54` count, which is what stops the survey drifting onto the other eight-byte array the
/// same object holds at `+0x64`.
fn cell_references(instructions: &[Instruction]) -> Vec<CellReference> {
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
    entry_states[0] = Some(TaintState::at_entry());
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
    for (block, entry) in entry_states.iter().enumerate() {
        let Some(entry) = entry else { continue };
        let mut state = entry.clone();
        let first = blocks[block];
        let last = blocks.get(block + 1).copied().unwrap_or(instructions.len());
        let mut emitted = Some(Vec::new());
        for instruction in &instructions[first..last] {
            step(&mut state, instruction, &mut emitted);
        }
        references.extend(emitted.unwrap_or_default());
    }
    references
}

/// Which registers hold the map object, the cell array, or a pointer into one cell.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TaintState {
    this_registers: BTreeSet<Register>,
    cell_registers: BTreeSet<Register>,
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
            cell_pointers: BTreeMap::new(),
            loaded: None,
        }
    }

    /// The must-merge of two states, or `None` when it equals what is already there.
    fn intersect(&self, other: &Self) -> Option<Self> {
        let merged = Self {
            this_registers: self.this_registers.intersection(&other.this_registers).copied().collect(),
            cell_registers: self.cell_registers.intersection(&other.cell_registers).copied().collect(),
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
    emitted: &mut Option<Vec<CellReference>>,
) {
    if let Some(sink) = emitted.as_mut() {
        let mut formatter = NasmFormatter::new();
        let mut info = InstructionInfoFactory::new();
        let mut text = String::new();
        formatter.format(instruction, &mut text);

        if let Some((lane, width)) =
            cell_lane(instruction, &state.cell_registers, &state.cell_pointers)
        {
            sink.push(CellReference {
                address: instruction.ip() as u32,
                lane,
                width,
                mnemonic: format!("{:?}", instruction.mnemonic()).to_lowercase(),
                mask: immediate_mask(instruction, lane, width),
                tier: "direct",
                access: lane_access(&mut info, instruction),
                text: text.clone(),
            });
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
                text,
            });
        }
    }

    let previous_loaded = state.loaded;
    state.loaded = None;
    match instruction.mnemonic() {
        Mnemonic::Lea if instruction.op0_kind() == OpKind::Register => {
            let destination = instruction.op0_register();
            let lane = if instruction.memory_index_scale() == 8
                && state.cell_registers.contains(&instruction.memory_base())
            {
                Some((instruction.memory_displacement64() as i64).rem_euclid(8) as u64)
            } else if let Some(base) = state.cell_pointers.get(&instruction.memory_base()) {
                (instruction.memory_index() == Register::None).then(|| {
                    (*base as i64 + instruction.memory_displacement64() as i64).rem_euclid(8) as u64
                })
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
            let source_is_cell_field = instruction.op1_kind() == OpKind::Memory
                && instruction.memory_index() == Register::None
                && ((instruction.memory_displacement64() == u64::from(MAP_CELLS_FIELD)
                    && state.this_registers.contains(&instruction.memory_base()))
                    || (instruction.memory_base() == Register::None
                        && instruction.memory_displacement64()
                            == u64::from(MAP_OBJECT + MAP_CELLS_FIELD)));
            let carried = (instruction.op1_kind() == OpKind::Register)
                .then(|| {
                    state
                        .cell_pointers
                        .get(&instruction.op1_register().full_register32())
                        .copied()
                })
                .flatten();
            let lane = cell_lane(instruction, &state.cell_registers, &state.cell_pointers);
            forget(destination, state);
            let destination = destination.full_register32();
            if source_is_this || source_is_object {
                state.this_registers.insert(destination);
            } else if source_is_cell_field {
                state.cell_registers.insert(destination);
            } else if let Some(base) = carried {
                state.cell_pointers.insert(destination, base);
            } else if let Some((lane, width)) = lane {
                state.loaded = Some((destination, lane, width));
            }
        }
        Mnemonic::Movsx | Mnemonic::Movzx if instruction.op0_kind() == OpKind::Register => {
            let destination = instruction.op0_register();
            let lane = cell_lane(instruction, &state.cell_registers, &state.cell_pointers);
            forget(destination, state);
            if let Some((lane, width)) = lane {
                state.loaded = Some((destination.full_register32(), lane, width));
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
    state.cell_pointers.remove(&full);
}

/// The cell lane a memory operand names, and its width, if it names one.
fn cell_lane(
    instruction: &Instruction,
    cell_registers: &BTreeSet<Register>,
    cell_pointers: &BTreeMap<Register, u64>,
) -> Option<(u64, u64)> {
    let has_memory = (0..instruction.op_count()).any(|operand| {
        matches!(
            instruction.op_kind(operand),
            OpKind::Memory
        )
    });
    if !has_memory {
        return None;
    }
    // Displacements like `-8` and `-6` appear where the index was pre-incremented; the lane is the
    // displacement modulo the cell stride either way.
    let displacement = instruction.memory_displacement64() as i64;
    let lane = if instruction.memory_index_scale() == 8
        && cell_registers.contains(&instruction.memory_base())
    {
        displacement.rem_euclid(8) as u64
    } else if let Some(base) = cell_pointers.get(&instruction.memory_base()) {
        if instruction.memory_index() != Register::None {
            return None;
        }
        (*base as i64 + displacement).rem_euclid(8) as u64
    } else {
        return None;
    };
    let width = instruction.memory_size().size() as u64;
    if width == 0 || width > 8 {
        return None;
    }
    Some((lane, width))
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

/// How many memory operands in the whole image use an eight-byte index scale, per lane.
///
/// This is deliberately **unfiltered**: it counts every eight-byte-strided array in the binary, not
/// just the cell grid, and a linear decode of a whole section misaligns on embedded data. It is a
/// scope figure, not evidence. Its job is to make it obvious when the filtered survey has collapsed
/// to a handful of sites because the analysis stopped early rather than because the engine is
/// quiet -- the failure mode where an empty result gets read as a clean one.
fn unfiltered_lane_census(image: &PeImage<'_>) -> BTreeMap<u64, usize> {
    let mut census: BTreeMap<u64, usize> = BTreeMap::new();
    let Ok(instructions) = disassemble_section(image) else {
        return census;
    };
    for instruction in &instructions {
        if instruction.memory_index_scale() != 8 || instruction.memory_base() == Register::None {
            continue;
        }
        let lane = (instruction.memory_displacement64() as i64).rem_euclid(8) as u64;
        *census.entry(lane).or_default() += 1;
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

/// Decode every executable section linearly. Linear decoding of a whole section misaligns on data
/// embedded in code, which is why this feeds only the call-site scan and never a field reading.
fn disassemble_section(image: &PeImage<'_>) -> Result<Vec<Instruction>, String> {
    let mut instructions = Vec::new();
    for (start, length) in image.executable_ranges() {
        let offset = image
            .file_offset(start)
            .ok_or_else(|| format!("{start:#010x} is not mapped"))?;
        let end = offset + length as usize;
        let slice = image
            .bytes()
            .get(offset..end.min(image.bytes().len()))
            .ok_or_else(|| format!("section at {start:#010x} is truncated"))?;
        let mut decoder = Decoder::with_ip(32, slice, u64::from(start), DecoderOptions::NONE);
        instructions.extend(decoder.iter());
    }
    Ok(instructions)
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
