//! The IMP animation-control fields, recovered from the engine's own decoder.
//!
//! `imp.rs` decodes the container's *structure* -- how many sequences, how many facings, where the
//! pixels are -- and keeps the bytes it cannot explain as raw blobs: `ImpSequence::metadata` (11
//! bytes) and `ImpFacing::metadata` (one 16-bit word). This module explains the parts of those
//! blobs that `lomse.exe` actually reads, by reading the engine's decoder instead of watching the
//! game.
//!
//! Terminology is a minefield here, because three vocabularies collide. The authoring tool's
//! generated `.h` calls a 16-byte record a **cycle** (`WILLOWA_MOVE`, `WILLOWA_DIE`); this
//! repository's decoder calls it a **sequence** and calls the 8-byte sub-records **facings**; the
//! engine treats a facing as one entry in a list indexed by a **direction**, and the two are not
//! the same number. This module keeps the decoder's names (`sequence`, `facing`) and uses
//! *direction* only for the engine-side index, which is the distinction that matters: a sequence
//! with five facings is played through eight directions.
//!
//! Every claim below is anchored to an address in `lomse.exe` 3.02,
//! SHA-256 `a505f399d5be73fe0a2215633f663717f28daeb3075bbcc05b47d40653669052`. The recovery
//! functions do not trust those addresses blindly: each one re-derives the rule out of the
//! instruction stream and fails if the shape it expects is not there, so a different build reports
//! a refusal rather than a fabricated answer.

use std::collections::BTreeMap;
use std::fmt;

use iced_x86::{
    Decoder, DecoderOptions, Instruction, InstructionInfoFactory, Mnemonic, OpAccess, OpKind,
};

use crate::native_table::PeImage;

/// Byte 0 of a sequence record. `Imp::Advance` reads it with `and ecx,7` at 0x0049D9E8, so only
/// the low three bits reach any comparison.
pub const SEQUENCE_CONTROL_BYTE: usize = 0;

/// Mask the engine applies to [`SEQUENCE_CONTROL_BYTE`] (0x0049D9E8, 0x0049AC8A, 0x0049D900).
pub const CYCLE_MODE_MASK: u8 = 0x07;

/// Byte 1 of a sequence record. Only bit 7 is ever tested (0x0049AC4E, 0x0049AD50, 0x0049D95F).
pub const SEQUENCE_MIRROR_BYTE: usize = 1;

/// The only bit of [`SEQUENCE_MIRROR_BYTE`] the engine tests.
pub const SEQUENCE_MIRROR_BIT: u8 = 0x80;

/// What the engine does when the frame index runs off the end of a cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleEnd {
    /// The index is reset to zero and the cycle repeats.
    WrapToStart,
    /// The index is pulled back to the last frame and stays there.
    HoldLastFrame,
    /// The mode has a jump-table slot whose target this classifier does not recognise.
    Unclassified,
}

impl fmt::Display for CycleEnd {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WrapToStart => "wrap to start",
            Self::HoldLastFrame => "hold last frame",
            Self::Unclassified => "unclassified",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpAnimError(String);

impl ImpAnimError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ImpAnimError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ImpAnimError {}

/// The cycle mode the engine will read out of a sequence record's metadata.
pub fn cycle_mode(metadata: &[u8; 11]) -> u8 {
    metadata[SEQUENCE_CONTROL_BYTE] & CYCLE_MODE_MASK
}

/// Whether the engine mirrors the sequence's facings to reach the directions beyond the last one.
///
/// Pass [`AnimRules::mirror_bit`] as `mirror_bit`, not a literal and not [`SEQUENCE_MIRROR_BIT`].
/// The constant is the value [`recover`] refuses to disagree with; it is not the authority. This
/// used to read the constant directly, which left the hole `ping_pong_mode` was threaded through
/// `AnimRules` to close still open on this axis -- and open precisely for the next caller, who
/// wires this into the viewer without calling `recover` first.
pub fn mirrors_facings(mirror_bit: u8, metadata: &[u8; 11]) -> bool {
    metadata[SEQUENCE_MIRROR_BYTE] & mirror_bit != 0
}

/// How many distinct directions a sequence exposes.
///
/// `Imp::DirectionCount` at 0x0049D920: `2 * facings - 2` when the mirror bit is set
/// (`lea eax,[ecx+ecx-2]`, 0x0049D96A), otherwise the stored facing count (0x0049D976).
pub fn direction_count(mirror_bit: u8, metadata: &[u8; 11], facing_count: usize) -> usize {
    if mirrors_facings(mirror_bit, metadata) {
        (2 * facing_count).saturating_sub(2)
    } else {
        facing_count
    }
}

/// Which stored facing plays for a direction, and whether it is drawn horizontally flipped.
///
/// The fold is `Imp::GetFacing` at 0x0049AD5A..0x0049AD71 (and the identical copy in
/// `Imp::GetFrame` at 0x0049AC54..0x0049AC70): when the mirror bit is set and the direction index
/// has reached the facing count, the engine substitutes `2 * facings - direction - 2` and raises
/// the flag its caller passes down to the blitter.
///
/// Returns `None` for a direction the sequence does not cover. Note that the engine's own range
/// check (0x0049AC78) is applied *after* the fold, not before it, so a caller that passes a
/// direction at or past [`direction_count`] is not rejected -- it wraps back onto an early facing
/// drawn flipped. Callers are expected to stay inside [`direction_count`]; this function
/// reproduces the engine rather than tightening it.
pub fn facing_for_direction(
    mirror_bit: u8,
    metadata: &[u8; 11],
    facing_count: usize,
    direction: usize,
) -> Option<(usize, bool)> {
    if facing_count == 0 {
        return None;
    }
    if direction < facing_count {
        return Some((direction, false));
    }
    if !mirrors_facings(mirror_bit, metadata) {
        return None;
    }
    let folded = (2 * facing_count).checked_sub(direction + 2)?;
    (folded < facing_count).then_some((folded, true))
}

/// The mode value this module is built for, checked against the binary by [`recover`].
///
/// It is deliberately **not** consulted by the rules below: they take the mode the binary actually
/// encodes. A constant that both the implementation and its test read is self-consistent for any
/// value -- with this set to 3, every ping-pong sequence in the archive would truncate to
/// forward-only and a suite comparing the implementation to itself would stay green. So the only
/// thing this constant does is give [`recover`] something to disagree with.
pub const PING_PONG_MODE: u8 = 4;

/// How many steps the frame index takes before the cycle ends.
///
/// `Imp::CycleLength` at 0x0049D8F0: the ping-pong mode returns `2 * frames - 1`
/// (`lea eax,[edx+edx-1]`, 0x0049D90E); every other mode returns the stored frame count
/// (0x0049D915).
///
/// Pass [`AnimRules::ping_pong_mode`] as `ping_pong`, not a literal.
pub fn cycle_length(ping_pong: u8, mode: u8, frame_count: usize) -> usize {
    if mode == ping_pong {
        (2 * frame_count).saturating_sub(1)
    } else {
        frame_count
    }
}

/// Which stored frame a position in the cycle shows.
///
/// The reflection is `Imp::GetFrame` at 0x0049AC94..0x0049ACA9: for the ping-pong mode, a position
/// at or past the frame count is replaced by `2 * frames - position - 2`.
pub fn frame_for_cycle_index(
    ping_pong: u8,
    mode: u8,
    frame_count: usize,
    index: usize,
) -> Option<usize> {
    if frame_count == 0 {
        return None;
    }
    if mode == ping_pong && index >= frame_count {
        let folded = (2 * frame_count).checked_sub(index + 2)?;
        return (folded < frame_count).then_some(folded);
    }
    (index < frame_count).then_some(index)
}

/// The engine's cycle-mode dispatch, read back out of the instruction stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleModeTable {
    /// The function the switch was recovered from.
    pub advance: u32,
    /// The address of the `jmp dword [reg*4+imm]` that dispatches on the mode.
    pub dispatch_site: u32,
    /// The address of the jump table itself.
    pub table_address: u32,
    /// One entry per mode the table covers, in mode order.
    pub modes: Vec<CycleModeEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleModeEntry {
    pub mode: u8,
    pub target: u32,
    pub end: CycleEnd,
}

/// Recover the cycle-mode jump table from `Imp::Advance`.
///
/// Nothing about the table is assumed: the mask, the number of modes and the targets are all read
/// out of the code. The mode count comes from the `cmp reg,imm` / `ja` pair that guards the jump,
/// which is the same bound the processor enforces, so a build with a sixth mode reports six.
pub fn recover_cycle_modes(
    image: &PeImage<'_>,
    advance: u32,
) -> Result<CycleModeTable, ImpAnimError> {
    let instructions = decode_from(image, advance, 64)?;
    // Keyed by register, so the mask and the bound that are adopted are the ones applied to the
    // register the jump actually indexes with. Taking the textually nearest `and`/`cmp` would let
    // an unrelated masked value next door supply the answer, which is the opposite of what this
    // function's contract claims.
    let mut masks: BTreeMap<iced_x86::Register, u8> = BTreeMap::new();
    let mut bounds: BTreeMap<iced_x86::Register, i32> = BTreeMap::new();
    for instruction in instructions.iter() {
        if instruction.op0_kind() == OpKind::Register
            && instruction.op1_kind() == OpKind::Immediate8to32
        {
            let register = instruction.op0_register().full_register32();
            match instruction.mnemonic() {
                Mnemonic::And => {
                    masks.insert(register, instruction.immediate8to32() as u8);
                }
                Mnemonic::Cmp => {
                    bounds.insert(register, instruction.immediate8to32());
                }
                _ => {}
            }
        }
        if instruction.mnemonic() != Mnemonic::Jmp || instruction.op0_kind() != OpKind::Memory {
            continue;
        }
        if instruction.memory_index_scale() != 4 {
            continue;
        }
        let index = instruction.memory_index().full_register32();
        let mask = masks.get(&index).copied().ok_or_else(|| {
            ImpAnimError::new(format!(
                "the indexed jump at {:#010x} indexes with {index:?}, which nothing masked",
                instruction.ip()
            ))
        })?;
        if mask != CYCLE_MODE_MASK {
            return Err(ImpAnimError::new(format!(
                "the mode mask at {advance:#010x} is {mask:#04x}, not {CYCLE_MODE_MASK:#04x}"
            )));
        }
        let bound = bounds.get(&index).copied().ok_or_else(|| {
            ImpAnimError::new(format!(
                "the indexed jump at {:#010x} indexes with {index:?}, which nothing bounded",
                instruction.ip()
            ))
        })?;
        if bound < 0 {
            return Err(ImpAnimError::new("the mode range check is negative"));
        }
        let count = (bound as usize) + 1;
        let table_address = instruction.memory_displacement32();
        let mut modes = Vec::with_capacity(count);
        for index in 0..count {
            let entry = table_address
                .checked_add((index * 4) as u32)
                .ok_or_else(|| ImpAnimError::new("the jump table runs past the address space"))?;
            let target = read_u32(image, entry)?;
            modes.push(CycleModeEntry {
                mode: index as u8,
                target,
                end: classify_cycle_end(image, target)?,
            });
        }
        return Ok(CycleModeTable {
            advance,
            dispatch_site: instruction.ip() as u32,
            table_address,
            modes,
        });
    }
    Err(ImpAnimError::new(format!(
        "no indexed jump found within 64 instructions of {advance:#010x}"
    )))
}

/// Classify one jump-table target by what it does to the frame-index field.
///
/// `WrapToStart` is `mov dword [reg+0Ch],0`; `HoldLastFrame` is a `dec` of the loaded count
/// followed by a store back into the same field. Anything else is reported rather than guessed at.
fn classify_cycle_end(image: &PeImage<'_>, target: u32) -> Result<CycleEnd, ImpAnimError> {
    let instructions = decode_from(image, target, 8)?;
    let mut saw_decrement = false;
    for instruction in &instructions {
        if instruction.mnemonic() == Mnemonic::Mov
            && instruction.op0_kind() == OpKind::Memory
            && instruction.memory_displacement64() == u64::from(FRAME_INDEX_FIELD)
            && instruction.op1_kind() == OpKind::Immediate32
            && instruction.immediate32() == 0
        {
            return Ok(CycleEnd::WrapToStart);
        }
        if instruction.mnemonic() == Mnemonic::Dec {
            saw_decrement = true;
        }
        if saw_decrement
            && instruction.mnemonic() == Mnemonic::Mov
            && instruction.op0_kind() == OpKind::Memory
            && instruction.memory_displacement64() == u64::from(FRAME_INDEX_FIELD)
        {
            return Ok(CycleEnd::HoldLastFrame);
        }
        if matches!(instruction.mnemonic(), Mnemonic::Ret) {
            break;
        }
    }
    Ok(CycleEnd::Unclassified)
}

/// Which width parity the mirrored-placement path subtracts an extra pixel for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    Even,
    Odd,
}

impl fmt::Display for Parity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Even => "even",
            Self::Odd => "odd",
        })
    }
}

/// The addresses the recovery starts from. Defaults are for `lomse.exe` 3.02.
///
/// These are the only hardcoded numbers in the module that are not checked against something: they
/// say *where to look*. Everything read at them is verified, so a different build produces a
/// refusal rather than a wrong answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineAddresses {
    /// `Imp::Advance`, reached from operator `setimpplayeraction` (0x0049E860) via
    /// `Imp::SetAction` (0x0049DA80, calls it at 0x0049DACB).
    pub advance: u32,
    /// `Imp::CycleLength`: returns the number of steps in the current cycle.
    pub cycle_length: u32,
    /// `Imp::GetFrame`: resolves (action, facing, position) to a frame record.
    pub get_frame: u32,
    /// `Imp::DirectionCount`: how many directions the current action exposes.
    pub direction_count: u32,
    /// `ImpPlayer::GetPlacement`: writes the anchor-relative top-left of the current frame.
    pub placement: u32,
    /// The address range holding the engine's IMP code.
    ///
    /// Used only to enumerate the sites the documentation cites, and to separate in-module hits
    /// from offset collisions elsewhere. It is an operator entry-point span, not a call-graph
    /// closure -- the closure claim is [`call_sites`].
    pub module: std::ops::Range<u32>,
}

impl Default for EngineAddresses {
    fn default() -> Self {
        Self {
            advance: 0x0049_D9A0,
            cycle_length: 0x0049_D8F0,
            get_frame: 0x0049_ABE0,
            direction_count: 0x0049_D920,
            placement: 0x0049_CC80,
            module: 0x0049_9000..0x004a_0000,
        }
    }
}

/// Every animation rule this module implements, with the value the binary actually encodes.
///
/// The point of carrying the values rather than reading module constants is that a wrong constant
/// cannot hide. `recover` refuses when the binary disagrees with the constant, and the pure
/// functions below take these values, so there is no path where a mutated constant silently
/// changes what the survey reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimRules {
    pub cycle_modes: CycleModeTable,
    /// The mode value the two ping-pong sites special-case.
    pub ping_pong_mode: u8,
    /// Where the `2N-1` cycle length is computed.
    pub ping_pong_length_site: u32,
    /// Where a position past the end is reflected to `2N-i-2`.
    pub ping_pong_reflection_site: u32,
    /// The bit of sequence byte 1 that turns facing mirroring on.
    pub mirror_bit: u8,
    /// Where that bit is tested, in address order.
    pub mirror_test_sites: Vec<u32>,
    /// The width parity for which the mirrored placement subtracts one more pixel.
    pub mirror_decrements_when: Parity,
    /// The `dec` that subtracts it.
    pub mirror_parity_site: u32,
}

/// Read every animation rule out of the image, refusing on any disagreement with this module.
pub fn recover(
    image: &PeImage<'_>,
    addresses: &EngineAddresses,
) -> Result<AnimRules, ImpAnimError> {
    let cycle_modes = recover_cycle_modes(image, addresses.advance)?;
    let (ping_pong_mode, ping_pong_length_site) =
        recover_ping_pong_length(image, addresses.cycle_length)?;
    let (reflected_mode, ping_pong_reflection_site) =
        recover_ping_pong_reflection(image, addresses.get_frame)?;
    if reflected_mode != ping_pong_mode {
        return Err(ImpAnimError::new(format!(
            "the length site at {ping_pong_length_site:#010x} special-cases mode {ping_pong_mode} \
             but the reflection at {ping_pong_reflection_site:#010x} special-cases \
             {reflected_mode}; a ping-pong needs both"
        )));
    }
    if ping_pong_mode != PING_PONG_MODE {
        return Err(ImpAnimError::new(format!(
            "the binary special-cases cycle mode {ping_pong_mode}, but this module is built for \
             {PING_PONG_MODE}"
        )));
    }
    if !cycle_modes
        .modes
        .iter()
        .any(|entry| entry.mode == ping_pong_mode && entry.end == CycleEnd::WrapToStart)
    {
        return Err(ImpAnimError::new(format!(
            "mode {ping_pong_mode} is special-cased as a ping-pong but does not wrap; a cycle that \
             held its last frame could not run back"
        )));
    }
    let (mirror_bit, mirror_test_sites) =
        recover_mirror_bit(image, addresses.direction_count, &addresses.module)?;
    if mirror_bit != SEQUENCE_MIRROR_BIT {
        return Err(ImpAnimError::new(format!(
            "the binary tests sequence byte 1 with {mirror_bit:#04x}, but this module is built for \
             {SEQUENCE_MIRROR_BIT:#04x}"
        )));
    }
    let (mirror_decrements_when, mirror_parity_site) =
        recover_mirror_parity(image, addresses.placement)?;
    Ok(AnimRules {
        cycle_modes,
        ping_pong_mode,
        ping_pong_length_site,
        ping_pong_reflection_site,
        mirror_bit,
        mirror_test_sites,
        mirror_decrements_when,
        mirror_parity_site,
    })
}

/// Recover the ping-pong mode from `Imp::CycleLength`'s `2N-1`.
///
/// The shape is a `cmp r8,imm` whose taken branch reaches `lea r,[r+r-1]`. The immediate is the
/// mode; the `lea` is what makes it a doubling rather than some other special case.
fn recover_ping_pong_length(
    image: &PeImage<'_>,
    cycle_length: u32,
) -> Result<(u8, u32), ImpAnimError> {
    let instructions = decode_from(image, cycle_length, 24)?;
    let mut candidate = None;
    for instruction in &instructions {
        if instruction.mnemonic() == Mnemonic::Cmp
            && instruction.op0_kind() == OpKind::Register
            && instruction.op1_kind() == OpKind::Immediate8
        {
            candidate = Some(instruction.immediate8());
        }
        if let Some(mode) = candidate
            && is_double_minus(instruction, 1)
        {
            return Ok((mode, instruction.ip() as u32));
        }
    }
    Err(ImpAnimError::new(format!(
        "no `lea r,[r+r-1]` doubling found within 24 instructions of {cycle_length:#010x}"
    )))
}

/// Recover the ping-pong mode from `Imp::GetFrame`'s `2N-i-2` reflection.
///
/// The shape is a `cmp r8,imm` reaching `lea r,[r+r]` followed by two subtractions. Requiring the
/// doubling *and* the `sub ...,2` is what distinguishes the reflection from any other comparison
/// against a small constant in the same function.
fn recover_ping_pong_reflection(
    image: &PeImage<'_>,
    get_frame: u32,
) -> Result<(u8, u32), ImpAnimError> {
    let instructions = decode_from(image, get_frame, 128)?;
    let mut candidate = None;
    for (position, instruction) in instructions.iter().enumerate() {
        if instruction.mnemonic() == Mnemonic::Cmp
            && instruction.op0_kind() == OpKind::Register
            && instruction.op1_kind() == OpKind::Immediate8
        {
            candidate = Some(instruction.immediate8());
        }
        let Some(mode) = candidate else { continue };
        if !is_double_minus(instruction, 0) {
            continue;
        }
        let tail = &instructions[position..(position + 4).min(instructions.len())];
        let subtracts_two = tail.iter().any(|candidate| {
            candidate.mnemonic() == Mnemonic::Sub
                && candidate.op1_kind() == OpKind::Immediate8to32
                && candidate.immediate8to32() == 2
        });
        if subtracts_two {
            return Ok((mode, instruction.ip() as u32));
        }
    }
    Err(ImpAnimError::new(format!(
        "no `lea r,[r+r]` / `sub r,2` reflection found within 128 instructions of {get_frame:#010x}"
    )))
}

/// Whether an instruction is `lea r,[b+b-offset]`, i.e. a doubling with a constant subtracted.
fn is_double_minus(instruction: &Instruction, offset: i64) -> bool {
    instruction.mnemonic() == Mnemonic::Lea
        && instruction.memory_base() != iced_x86::Register::None
        && instruction.memory_base() == instruction.memory_index()
        && instruction.memory_index_scale() == 1
        && instruction.memory_displacement64() as i32 as i64 == -offset
}

/// Recover the mirror bit from `Imp::DirectionCount`, and every site that tests it.
///
/// The function is only a dozen instructions long and its whole content is: test a bit of byte 1,
/// and on the set branch return `2N-2`. Requiring the `lea r,[r+r-2]` is what ties the bit to
/// mirroring rather than to some other flag in the same byte.
fn recover_mirror_bit(
    image: &PeImage<'_>,
    direction_count: u32,
    module: &std::ops::Range<u32>,
) -> Result<(u8, Vec<u32>), ImpAnimError> {
    let instructions = decode_from(image, direction_count, 48)?;
    let mut bit = None;
    for instruction in &instructions {
        if instruction.mnemonic() == Mnemonic::Test
            && instruction.op0_kind() == OpKind::Memory
            && instruction.memory_size().size() == 1
            && instruction.memory_displacement64() == SEQUENCE_MIRROR_BYTE as u64
            && instruction.op1_kind() == OpKind::Immediate8
        {
            bit = Some((instruction.immediate8(), instruction.ip() as u32));
        }
        if let Some((value, _)) = bit
            && is_double_minus(instruction, 2)
        {
            let sites = mirror_test_sites(image, value, module)?;
            return Ok((value, sites));
        }
    }
    Err(ImpAnimError::new(format!(
        "no `lea r,[r+r-2]` direction doubling found within 48 instructions of \
         {direction_count:#010x}"
    )))
}

/// Every `test byte [r+1],bit` in the IMP module, so the doc's site list is measured not recalled.
fn mirror_test_sites(
    image: &PeImage<'_>,
    bit: u8,
    module: &std::ops::Range<u32>,
) -> Result<Vec<u32>, ImpAnimError> {
    let instructions = decode_range(image, module.start, (module.end - module.start) as usize)?;
    Ok(instructions
        .iter()
        .filter(|instruction| {
            instruction.mnemonic() == Mnemonic::Test
                && instruction.op0_kind() == OpKind::Memory
                && instruction.memory_size().size() == 1
                && instruction.memory_displacement64() == SEQUENCE_MIRROR_BYTE as u64
                && instruction.op1_kind() == OpKind::Immediate8
                && instruction.immediate8() == bit
        })
        .map(|instruction| instruction.ip() as u32)
        .collect())
}

/// Recover which width parity the mirrored placement path subtracts an extra pixel for.
///
/// This exists because the answer was got backwards once by reading the mnemonics in order and
/// assuming the `dec` after a `jne` runs on the tested condition. It does not: the shape is
/// `neg` / `test r8,1` / `jcc past` / `dec`, and a `jne` that jumps *over* the `dec` means the
/// `dec` runs when the low bit is **clear**, i.e. on even widths.
fn recover_mirror_parity(
    image: &PeImage<'_>,
    placement: u32,
) -> Result<(Parity, u32), ImpAnimError> {
    let instructions = decode_from(image, placement, 48)?;
    for (position, instruction) in instructions.iter().enumerate() {
        let tests_low_bit = instruction.mnemonic() == Mnemonic::Test
            && instruction.op0_kind() == OpKind::Register
            && instruction.op1_kind() == OpKind::Immediate8
            && instruction.immediate8() == 1;
        if !tests_low_bit {
            continue;
        }
        let Some(branch) = instructions.get(position + 1) else {
            continue;
        };
        let Some(decrement) = instructions.get(position + 2) else {
            continue;
        };
        if decrement.mnemonic() != Mnemonic::Dec {
            continue;
        }
        let skips_the_decrement = branch.near_branch32() > decrement.ip() as u32;
        let parity = match (branch.mnemonic(), skips_the_decrement) {
            // `jne` past the `dec`: the jump is taken when the bit is set, so the `dec` is the
            // fall-through and runs when the bit is clear.
            (Mnemonic::Jne, true) => Parity::Even,
            (Mnemonic::Je, true) => Parity::Odd,
            _ => {
                return Err(ImpAnimError::new(format!(
                    "the parity branch at {:#010x} is {:?} and does not skip the `dec` at \
                     {:#010x}; the shape this reads is not there",
                    branch.ip(),
                    branch.mnemonic(),
                    decrement.ip()
                )));
            }
        };
        return Ok((parity, decrement.ip() as u32));
    }
    Err(ImpAnimError::new(format!(
        "no `test r8,1` / branch / `dec` parity correction found within 48 instructions of \
         {placement:#010x}"
    )))
}

/// The anchor-relative top-left x of a frame drawn **flipped**.
///
/// `ImpPlayer::GetPlacement` (0x0049CC80) computes the unflipped value as
/// `placement_x - (width >> 1)` at 0x0049CD01 -- the x half of the rule in `docs/hotspots.md` --
/// and the flipped value at 0x0049CCC8..0x0049CCD3 as
/// `-((width >> 1) + placement_x)`, minus one more pixel when the width is **even**.
///
/// The even-width `dec` is Observed, not derived: reflecting the unflipped span about the anchor
/// column reproduces the odd-width result exactly and lands two pixels away on even widths, so the
/// extra pixel is the engine's own convention rather than a consequence of the mirror. Anyone
/// implementing flipped placement wants this function, not the algebra.
pub fn mirrored_anchor_x(rules: &AnimRules, width: u16, placement_x: i16) -> i32 {
    let half = i32::from(width >> 1);
    let base = -(half + i32::from(placement_x));
    let parity = if width.is_multiple_of(2) {
        Parity::Even
    } else {
        Parity::Odd
    };
    if parity == rules.mirror_decrements_when {
        base - 1
    } else {
        base
    }
}

/// The anchor-relative top-left x of a frame drawn unflipped, for comparison.
///
/// `0x0049CCFC`..`0x0049CD01`. Present so the flipped rule can be tested against the rule this
/// repository already established rather than against a restatement of itself.
pub fn anchor_x(width: u16, placement_x: i16) -> i32 {
    i32::from(placement_x) - i32::from(width >> 1)
}

/// Offset of the playback frame index inside the engine's animation-player object.
///
/// Written by `Imp::SetAction` at 0x0049DAC8 and by both jump-table targets (0x0049D9FC,
/// 0x0049DA05).
pub const FRAME_INDEX_FIELD: u32 = 0x0c;

/// One memory read the field-coverage scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRead {
    pub address: u32,
    pub displacement: u64,
    pub operand_size: usize,
    /// Whether the operand also carried a scaled index register.
    pub indexed: bool,
    /// The decoded instruction, for a caller that wants to render it.
    ///
    /// Deliberately not a pre-formatted string: rendering needs `iced-x86`'s `nasm` feature, and
    /// nothing in this library consumes prose. Carrying the `Instruction` keeps the formatter's
    /// tables out of the SDL viewer and the map editor, which link this crate for other reasons.
    pub instruction: Instruction,
}

/// Every base-register-relative memory read in a code range whose displacement falls in `range`.
///
/// This exists to bound a negative, and a bound is only worth what its exclusions cost. **What is
/// excluded, in full:** operands with no base register (absolute globals, which cannot be a record
/// field), `esp`-based operands (locals, and they swamp everything else), and stores. Nothing
/// else. In particular:
///
/// - **Indexed operands are kept**, and reported through [`FieldRead::indexed`]. An earlier version
///   dropped them, which would have hidden `0x0049AC8F mov di,[ebx+esi*8+2]` -- a real record read
///   at displacement 2 -- and would have let a timing field read as `mov cx,[edi+ebx*16+2]` produce
///   no hits at all.
/// - **`ebp` is kept.** In this module `ebp` is an object pointer, not a frame pointer:
///   `0x0049ABFC`, `0x0049AC0F` and `0x0049AC43` are genuine record reads through it. Excluding it
///   as "a local" was wrong.
/// - **`lea` is dropped**, because it computes an address and reads no memory. It is the one
///   exclusion added rather than removed on review; leaving it in inflated the counts.
pub fn field_reads(
    image: &PeImage<'_>,
    start: u32,
    length: usize,
    range: std::ops::RangeInclusive<u64>,
) -> Result<Vec<FieldRead>, ImpAnimError> {
    use iced_x86::Register;

    let mut factory = InstructionInfoFactory::new();
    let mut reads = Vec::new();
    for instruction in decode_range(image, start, length)? {
        if matches!(
            instruction.memory_base(),
            Register::None | Register::ESP | Register::EIP
        ) {
            continue;
        }
        if !range.contains(&instruction.memory_displacement64()) {
            continue;
        }
        if !reads_a_memory_operand(&mut factory, &instruction) {
            continue;
        }
        reads.push(FieldRead {
            address: instruction.ip() as u32,
            displacement: instruction.memory_displacement64(),
            operand_size: instruction.memory_size().size(),
            indexed: instruction.memory_index() != Register::None,
            instruction,
        });
    }
    Ok(reads)
}

/// Whether an instruction actually **reads** a memory operand.
///
/// Asked through `instr_info` rather than by listing mnemonics, because the mnemonic list is where
/// the bugs live. This one predicate covers all three cases that were previously special-cased or
/// missed: a store (`mov [mem],reg` -- memory is `Write`), an address computation (`lea` -- iced
/// reports `NoMemAccess`), and a read-modify-write (`add [mem],reg` -- `ReadWrite`, and it does
/// read).
fn reads_a_memory_operand(factory: &mut InstructionInfoFactory, instruction: &Instruction) -> bool {
    let info = factory.info(instruction);
    (0..instruction.op_count()).any(|index| {
        instruction.op_kind(index) == OpKind::Memory
            && matches!(
                info.op_access(index),
                OpAccess::Read | OpAccess::ReadWrite | OpAccess::CondRead | OpAccess::ReadCondWrite
            )
    })
}

/// A read reached through a pointer that was loaded out of a known struct field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaintedRead {
    /// The load that produced the pointer.
    pub source: u32,
    /// The displacement that load read from.
    pub source_displacement: u64,
    /// The read reached through it.
    pub address: u32,
    pub displacement: u64,
    pub operand_size: usize,
    /// The decoded instruction; see [`FieldRead::instruction`] for why it is not a string.
    pub instruction: Instruction,
}

/// How a pointer becomes a *record* pointer, and therefore what counts as a source.
///
/// Displacement alone cannot identify a struct, and in this engine that is not a hypothetical:
/// **offset 0x1C means two different things**. On the loaded IMP header it is the sequence table
/// (`0x0049ADE3`); on the animation-player object it is the current *frame* record
/// (`0x0049CC80`). A scan keyed on the displacement alone reports the second as the first and its
/// output is worthless -- which is what the first version of this scan did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerSource {
    /// A dword load at this displacement yields a **table base**. Reads are only reported once an
    /// index has been added to it, because that addition is what turns a table into a record.
    Table { displacement: u64 },
    /// A dword load at this displacement already yields a record pointer.
    Record { displacement: u64 },
}

impl PointerSource {
    fn displacement(&self) -> u64 {
        match self {
            Self::Table { displacement } | Self::Record { displacement } => *displacement,
        }
    }

    fn needs_index(&self) -> bool {
        matches!(self, Self::Table { .. })
    }
}

/// Follow pointers loaded from `sources` and report every field they are then read through.
///
/// This is the scan that bounds the "nothing reads sequence byte 2" claim, and it exists because
/// the argument it replaces was refuted by code already read. The old argument was "a
/// sequence-record address can only be formed by scaling an index by 16 and adding the header
/// pointer". It cannot: `Imp::SetAction` caches the record pointer into the player object at
/// `0x0049DAA2 mov [esi+24h],eax`, and `Imp::CycleLength` reads it straight back at `0x0049D8F7`
/// with no scaling anywhere, from a function **12** out-of-module callers can reach. (25 is
/// `Imp::Advance`'s out-of-module caller count, which an earlier revision of this comment used by
/// mistake while both `.md` files had it right.)
///
/// So the search starts from the *loads*. For each dword load at one of `sources`, the pointer is
/// followed for `window` instructions through register moves and through the `shl`/`add` that turns
/// a table base into a record address, and every memory read based on it is reported.
///
/// **Limits, stated because the negative rests on them:** the walk is linear, so it does not follow
/// branches, and it stops at the first `call` or `ret` because those clobber registers. It tracks
/// only 32-bit general registers, never memory-to-memory forwarding. A [`PointerSource::Table`]
/// source suppresses reads taken before an index is added, which is what makes the output about
/// records rather than about every struct in the image that happens to use the same offset. It is a
/// bound on straight-line reachability, not a proof.
pub fn record_pointer_reads(
    image: &PeImage<'_>,
    start: u32,
    length: usize,
    sources: &[PointerSource],
    window: usize,
) -> Result<Vec<TaintedRead>, ImpAnimError> {
    use iced_x86::Register;
    use std::collections::BTreeSet;

    let instructions = decode_range(image, start, length)?;
    let mut factory = InstructionInfoFactory::new();
    let mut found = Vec::new();
    for (position, load) in instructions.iter().enumerate() {
        let is_load = load.mnemonic() == Mnemonic::Mov
            && load.op0_kind() == OpKind::Register
            && load.op1_kind() == OpKind::Memory
            && load.memory_size().size() == 4;
        if !is_load {
            continue;
        }
        let Some(source) = sources
            .iter()
            .find(|source| source.displacement() == load.memory_displacement64())
        else {
            continue;
        };
        let mut tainted: BTreeSet<Register> = BTreeSet::new();
        tainted.insert(load.op0_register().full_register32());
        // For a table base, nothing is a record until an index has been added to it.
        let mut indexed = !source.needs_index();
        let high = (position + 1 + window).min(instructions.len());
        for instruction in &instructions[position + 1..high] {
            if matches!(instruction.mnemonic(), Mnemonic::Call | Mnemonic::Ret) {
                break;
            }
            let base = instruction.memory_base().full_register32();
            // An operand that carries its own scaled index is a record access in one step.
            let self_indexed = instruction.memory_index() != Register::None;
            if reads_a_memory_operand(&mut factory, instruction)
                && tainted.contains(&base)
                && (indexed || self_indexed)
            {
                found.push(TaintedRead {
                    source: load.ip() as u32,
                    source_displacement: load.memory_displacement64(),
                    address: instruction.ip() as u32,
                    displacement: instruction.memory_displacement64(),
                    operand_size: instruction.memory_size().size(),
                    instruction: *instruction,
                });
            }

            let destination = (instruction.op0_kind() == OpKind::Register)
                .then(|| instruction.op0_register().full_register32());
            let source_register = (instruction.op1_kind() == OpKind::Register)
                .then(|| instruction.op1_register().full_register32());
            let propagates = match (destination, instruction.mnemonic()) {
                // A register-to-register move carries the pointer; a load *through* it does not.
                // `mov eax,[eax+edx+0Ch]` yields the pointee -- the facing table a sequence record
                // points at -- which is a different object.
                (Some(_), Mnemonic::Mov) => {
                    source_register.is_some_and(|register| tainted.contains(&register))
                }
                // `lea` computes an address rather than dereferencing, so it does carry.
                (Some(_), Mnemonic::Lea) => {
                    tainted.contains(&base)
                        || tainted.contains(&instruction.memory_index().full_register32())
                }
                // The index has landed: from here the pointer designates a record. The addition
                // runs both ways round -- `add record,index` and `add index,record` -- and taking
                // only the first made `Imp::DirectionCount`, which forms its record as
                // `add eax,edx` with the *table* in `edx`, invisible to this scan.
                (Some(destination), Mnemonic::Add) => {
                    let carries = tainted.contains(&destination)
                        || source_register.is_some_and(|register| tainted.contains(&register));
                    if carries {
                        indexed = true;
                    }
                    carries
                }
                (Some(destination), Mnemonic::Shl | Mnemonic::Sub | Mnemonic::And) => {
                    tainted.contains(&destination)
                }
                _ => false,
            };

            // Clear taint from every register this instruction *writes*, and only those. Asking
            // `instr_info` rather than assuming "first operand is a register" means it is written:
            // `test ecx,ecx`, `cmp`, and `push` all have a register first operand and write
            // nothing. That assumption cost this scan its entire second source -- every cached
            // pointer in this engine is reloaded and immediately null-checked
            // (`0x0049D8F7 mov ecx,[ecx+24h]` / `0x0049D8FA test ecx,ecx`), so the `test` was
            // erasing a pointer it never touched and `0x0049D8FE mov cl,[ecx]` went unseen.
            let info = factory.info(instruction);
            for used in info.used_registers() {
                if !matches!(
                    used.access(),
                    OpAccess::Write | OpAccess::ReadWrite | OpAccess::CondWrite
                ) {
                    continue;
                }
                let register = used.register().full_register32();
                if propagates && destination == Some(register) {
                    continue;
                }
                tainted.remove(&register);
            }
            if let Some(destination) = destination.filter(|_| propagates) {
                tainted.insert(destination);
            }
        }
    }
    Ok(found)
}

/// Offset of the sequence-table pointer inside the loaded IMP header.
///
/// The count sits in the preceding word at 0x1A. Both are read together at 0x0049ADDB /
/// 0x0049ADE3, and they are the same two header fields `ImpSprite::parse` reads from file offsets
/// 26 and 28 -- which is what makes this scan's addressing a control rather than a guess.
pub const HEADER_SEQUENCE_TABLE: u64 = 0x1c;

/// Offset at which the player object caches the current action's sequence record.
///
/// Written at `0x0049DAA2`, read back at `0x0049D8F7`. This is the second way a sequence-record
/// pointer comes into existence, and missing it is what made the first version of the negative
/// overstated.
pub const PLAYER_CACHED_SEQUENCE: u64 = 0x24;

/// Offset at which an `Imp` object holds the loaded file image the header lives at the front of.
///
/// `0x0049ADB7 mov esi,[eax+8]`, where `eax` came from `[ecx]` -- the object behind the animation
/// player. Every route to a sequence record passes through this field, which is what lets an
/// otherwise untypeable `[x+0x1C]` load be tested for actually being an IMP header.
pub const IMP_FILE_IMAGE: u64 = 0x08;

/// The sequence-record bytes with no established meaning.
pub const UNEXPLAINED_SEQUENCE_BYTES: std::ops::RangeInclusive<u64> = 2..=10;

/// Every site that forms the address of a sequence record.
///
/// A sequence record is reached exactly one way: scale an index by the 16-byte stride and add the
/// sequence-table pointer from the loaded header. The scan finds each `shl reg,4` in the range and
/// asks whether a load of `[reg+0x1C]` appears within a short window either side. Frame records
/// share the 16-byte stride, so the window test is what separates the two; sites without a nearby
/// header load are reported as such rather than dropped, because the interesting failure would be
/// a consumer this scan cannot see.
pub fn sequence_record_sites(
    image: &PeImage<'_>,
    start: u32,
    length: usize,
) -> Result<Vec<RecordAddressSite>, ImpAnimError> {
    let instructions = decode_range(image, start, length)?;
    let mut sites = Vec::new();
    const WINDOW: usize = 8;
    for (position, instruction) in instructions.iter().enumerate() {
        let scales_by_sixteen = instruction.mnemonic() == Mnemonic::Shl
            && instruction.op1_kind() == OpKind::Immediate8
            && instruction.immediate8() == 4;
        if !scales_by_sixteen {
            continue;
        }
        let low = position.saturating_sub(WINDOW);
        let high = (position + WINDOW).min(instructions.len());
        let near_header_load = instructions[low..high].iter().any(|candidate| {
            candidate.op_count() > 1
                && candidate.op1_kind() == OpKind::Memory
                && candidate.memory_displacement64() == HEADER_SEQUENCE_TABLE
                && candidate.memory_size().size() == 4
        });
        sites.push(RecordAddressSite {
            address: instruction.ip() as u32,
            near_header_load,
            instruction: *instruction,
        });
    }
    Ok(sites)
}

/// One site that scales an index by the 16-byte record stride.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordAddressSite {
    pub address: u32,
    /// Whether a dword load at [`HEADER_SEQUENCE_TABLE`] sits within eight instructions.
    pub near_header_load: bool,
    pub instruction: Instruction,
}

/// Every little-endian dword in the image equal to `address`, as file offsets.
///
/// [`call_sites`] only finds `NearBranch32` operands, so on its own it leaves open the premise that
/// the target is never reached indirectly -- through a vtable slot, a dispatch table or a
/// `mov reg,imm32` followed by an indirect call. This closes that premise the only way it can be
/// closed: an address that appears nowhere as a literal dword cannot be loaded as one.
///
/// Scanned over the whole file rather than the code sections, because a function pointer is data.
pub fn absolute_references(image: &PeImage<'_>, address: u32) -> Vec<usize> {
    let needle = address.to_le_bytes();
    image
        .bytes()
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(offset, _)| offset)
        .collect()
}

/// Every direct `call`/`jmp` to `target` in a code range.
///
/// The second half of the negative's bound, and the half that is actually sound. Typing a struct by
/// the displacement a pointer was loaded from does not work -- offsets 0x1C and 0x24 are used by
/// plenty of unrelated objects -- so the question "could out-of-module code hold a sequence-record
/// pointer?" is answered instead by asking who can obtain one. `Imp::GetSequence` (0x0049ADB0) is
/// the only function that returns one, so enumerating its call sites bounds the answer exhaustively.
pub fn call_sites(
    image: &PeImage<'_>,
    start: u32,
    length: usize,
    target: u32,
) -> Result<Vec<u32>, ImpAnimError> {
    Ok(decode_range(image, start, length)?
        .iter()
        .filter(|instruction| {
            matches!(instruction.mnemonic(), Mnemonic::Call | Mnemonic::Jmp)
                && instruction.op0_kind() == OpKind::NearBranch32
                && instruction.near_branch32() == target
        })
        .map(|instruction| instruction.ip() as u32)
        .collect())
}

/// Decode a whole byte range linearly.
fn decode_range(
    image: &PeImage<'_>,
    start: u32,
    length: usize,
) -> Result<Vec<Instruction>, ImpAnimError> {
    let offset = image
        .file_offset(start)
        .ok_or_else(|| ImpAnimError::new(format!("{start:#010x} is not mapped")))?;
    let end = offset
        .checked_add(length)
        .filter(|end| *end <= image.bytes().len())
        .ok_or_else(|| ImpAnimError::new("the scan range runs past the image"))?;
    let mut decoder = Decoder::with_ip(
        32,
        &image.bytes()[offset..end],
        u64::from(start),
        DecoderOptions::NONE,
    );
    Ok(decoder.iter().collect())
}

fn decode_from(
    image: &PeImage<'_>,
    address: u32,
    count: usize,
) -> Result<Vec<Instruction>, ImpAnimError> {
    let offset = image
        .file_offset(address)
        .ok_or_else(|| ImpAnimError::new(format!("{address:#010x} is not mapped")))?;
    let mut decoder = Decoder::with_ip(
        32,
        &image.bytes()[offset..],
        u64::from(address),
        DecoderOptions::NONE,
    );
    Ok(decoder.iter().take(count).collect())
}

fn read_u32(image: &PeImage<'_>, address: u32) -> Result<u32, ImpAnimError> {
    let offset = image
        .file_offset(address)
        .ok_or_else(|| ImpAnimError::new(format!("{address:#010x} is not mapped")))?;
    let bytes = image
        .bytes()
        .get(offset..offset + 4)
        .ok_or_else(|| ImpAnimError::new(format!("{address:#010x} is truncated")))?;
    Ok(u32::from_le_bytes(bytes.try_into().expect("four bytes")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wrap a run of machine code in a minimal PE, so the recovery is exercised without the
    /// proprietary binary. Mirrors the helper in `operator_arity`.
    fn image_with_code(code: &[u8], table: &[u32]) -> Vec<u8> {
        const PE_OFFSET: usize = 0x80;
        const IMAGE_BASE: u32 = 0x0040_0000;
        const CODE_VA: u32 = 0x1000;
        const CODE_RAW: u32 = 0x200;
        const CODE_SIZE: u32 = 0x400;
        const DATA_VA: u32 = 0x2000;
        const DATA_RAW: u32 = 0x600;
        const DATA_SIZE: u32 = 0x200;

        let mut image = vec![0_u8; (DATA_RAW + DATA_SIZE) as usize];
        image[0x3c..0x40].copy_from_slice(&(PE_OFFSET as u32).to_le_bytes());
        image[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        image[PE_OFFSET + 6..PE_OFFSET + 8].copy_from_slice(&2_u16.to_le_bytes());
        let optional_size: u16 = 0xe0;
        image[PE_OFFSET + 20..PE_OFFSET + 22].copy_from_slice(&optional_size.to_le_bytes());
        image[PE_OFFSET + 24..PE_OFFSET + 26].copy_from_slice(&0x10b_u16.to_le_bytes());
        image[PE_OFFSET + 52..PE_OFFSET + 56].copy_from_slice(&IMAGE_BASE.to_le_bytes());
        let base = PE_OFFSET + 24 + usize::from(optional_size);
        image[base..base + 5].copy_from_slice(b".text");
        image[base + 12..base + 16].copy_from_slice(&CODE_VA.to_le_bytes());
        image[base + 16..base + 20].copy_from_slice(&CODE_SIZE.to_le_bytes());
        image[base + 20..base + 24].copy_from_slice(&CODE_RAW.to_le_bytes());
        image[base + 36..base + 40].copy_from_slice(&0x2000_0000_u32.to_le_bytes());
        let base = base + 40;
        image[base..base + 6].copy_from_slice(b".rdata");
        image[base + 12..base + 16].copy_from_slice(&DATA_VA.to_le_bytes());
        image[base + 16..base + 20].copy_from_slice(&DATA_SIZE.to_le_bytes());
        image[base + 20..base + 24].copy_from_slice(&DATA_RAW.to_le_bytes());
        image[base + 36..base + 40].copy_from_slice(&0x4000_0000_u32.to_le_bytes());

        image[CODE_RAW as usize..CODE_RAW as usize + code.len()].copy_from_slice(code);
        for (index, entry) in table.iter().enumerate() {
            let at = DATA_RAW as usize + index * 4;
            image[at..at + 4].copy_from_slice(&entry.to_le_bytes());
        }
        image
    }

    const ENTRY: u32 = 0x0040_1000;
    const TABLE: u32 = 0x0040_2000;

    /// The shape `Imp::Advance` has at 0x0049D9E6: load the control byte, mask it to three bits,
    /// bound it, and dispatch through a dword table.
    fn advance_prologue() -> Vec<u8> {
        let mut code = Vec::new();
        code.extend_from_slice(&[0x8a, 0x0f]); // mov cl,[edi]
        code.extend_from_slice(&[0x83, 0xe1, 0x07]); // and ecx,7
        code.extend_from_slice(&[0x83, 0xf9, 0x04]); // cmp ecx,4
        code.extend_from_slice(&[0x77, 0x10]); // ja +0x10
        code.extend_from_slice(&[0xff, 0x24, 0x8d]); // jmp dword [ecx*4+TABLE]
        code.extend_from_slice(&TABLE.to_le_bytes());
        code
    }

    /// `mov dword [esi+0Ch],0` then `ret`: the wrapping target at 0x0049DA05.
    const WRAP_BODY: [u8; 8] = [0xc7, 0x46, 0x0c, 0x00, 0x00, 0x00, 0x00, 0xc3];
    /// `dec eax` then `mov [esi+0Ch],eax` then `ret`: the holding target at 0x0049D9FB.
    const HOLD_BODY: [u8; 5] = [0x48, 0x89, 0x46, 0x0c, 0xc3];

    fn advance_image(order: &[bool]) -> (Vec<u8>, Vec<u32>) {
        let mut code = advance_prologue();
        let mut table = Vec::new();
        for wrap in order {
            table.push(ENTRY + code.len() as u32);
            if *wrap {
                code.extend_from_slice(&WRAP_BODY);
            } else {
                code.extend_from_slice(&HOLD_BODY);
            }
        }
        (code, table)
    }

    #[test]
    fn recovers_the_mode_count_from_the_range_check_rather_than_assuming_it() {
        let (code, table) = advance_image(&[true, false, false, true, true]);
        let bytes = image_with_code(&code, &table);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let recovered = recover_cycle_modes(&image, ENTRY).expect("switch is recovered");
        assert_eq!(recovered.modes.len(), 5);
        assert_eq!(recovered.table_address, TABLE);
        let ends: Vec<CycleEnd> = recovered.modes.iter().map(|entry| entry.end).collect();
        assert_eq!(
            ends,
            vec![
                CycleEnd::WrapToStart,
                CycleEnd::HoldLastFrame,
                CycleEnd::HoldLastFrame,
                CycleEnd::WrapToStart,
                CycleEnd::WrapToStart,
            ]
        );
    }

    #[test]
    fn refuses_a_switch_whose_mask_is_not_the_three_low_bits() {
        let (mut code, table) = advance_image(&[true, true]);
        // Widen `and ecx,7` to `and ecx,0x0f`: a build that partitioned the byte differently.
        code[4] = 0x0f;
        let bytes = image_with_code(&code, &table);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let error = recover_cycle_modes(&image, ENTRY).expect_err("the mask no longer matches");
        assert!(error.to_string().contains("mode mask"), "{error}");
    }

    #[test]
    fn ignores_a_mask_applied_to_a_register_the_jump_does_not_index_with() {
        // `and edx,7` before the jump, and nothing masking `ecx`. The earlier version of this
        // recovery took the textually nearest mask and would have accepted the unrelated one,
        // contradicting its own claim to assume nothing.
        let mut code = Vec::new();
        code.extend_from_slice(&[0x83, 0xe2, 0x07]); // and edx,7
        code.extend_from_slice(&[0x83, 0xfa, 0x04]); // cmp edx,4
        code.extend_from_slice(&[0xff, 0x24, 0x8d]); // jmp dword [ecx*4+TABLE]
        code.extend_from_slice(&TABLE.to_le_bytes());
        let bytes = image_with_code(&code, &[ENTRY]);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let error = recover_cycle_modes(&image, ENTRY).expect_err("ecx was never masked");
        assert!(
            error.to_string().contains("which nothing masked"),
            "{error}"
        );
    }

    /// `cmp cl,4` then `lea eax,[edx+edx-1]`: the `2N-1` at 0x0049D903..0x0049D90E.
    ///
    /// The `0x04` here is a literal standing for the byte the engine encodes, not for
    /// [`PING_PONG_MODE`] -- which is the point. If the module constant is changed to anything
    /// else, this test fails, because the two are now independent.
    #[test]
    fn recovers_the_ping_pong_mode_from_the_doubled_cycle_length() {
        let mut code = Vec::new();
        code.extend_from_slice(&[0x80, 0xf9, 0x04]); // cmp cl,4
        code.extend_from_slice(&[0x75, 0x04]); // jne +4
        code.extend_from_slice(&[0x8d, 0x44, 0x12, 0xff]); // lea eax,[edx+edx-1]
        code.extend_from_slice(&[0xc3]); // ret
        let bytes = image_with_code(&code, &[]);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let (mode, site) =
            recover_ping_pong_length(&image, ENTRY).expect("the doubling is recovered");
        assert_eq!(mode, PING_PONG_MODE);
        assert_eq!(site, ENTRY + 5);
    }

    #[test]
    fn a_comparison_without_a_doubling_is_not_taken_for_the_ping_pong_mode() {
        // A bare `cmp cl,4` guarding something else must not be mistaken for the length rule.
        let mut code = Vec::new();
        code.extend_from_slice(&[0x80, 0xf9, 0x04]); // cmp cl,4
        code.extend_from_slice(&[0x75, 0x01]); // jne +1
        code.extend_from_slice(&[0x40]); // inc eax
        code.extend_from_slice(&[0xc3]); // ret
        let bytes = image_with_code(&code, &[]);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        assert!(recover_ping_pong_length(&image, ENTRY).is_err());
    }

    /// `test byte [eax+1],0x80` then `lea eax,[ecx+ecx-2]`: 0x0049D95F..0x0049D96A.
    ///
    /// As above, `0x80` is an independent literal for the engine's encoded byte.
    #[test]
    fn recovers_the_mirror_bit_from_the_doubled_direction_count() {
        let mut code = Vec::new();
        code.extend_from_slice(&[0xf6, 0x40, 0x01, 0x80]); // test byte [eax+1],80h
        code.extend_from_slice(&[0x74, 0x04]); // je +4
        code.extend_from_slice(&[0x8d, 0x44, 0x09, 0xfe]); // lea eax,[ecx+ecx-2]
        code.extend_from_slice(&[0xc3]); // ret
        let bytes = image_with_code(&code, &[]);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let module = ENTRY..ENTRY + code.len() as u32;
        let (bit, sites) =
            recover_mirror_bit(&image, ENTRY, &module).expect("the mirror bit is recovered");
        assert_eq!(bit, SEQUENCE_MIRROR_BIT);
        // The site scan runs over the range it was given, so the one `test` in this image is it.
        assert_eq!(sites, vec![ENTRY]);
    }

    /// The error this module was reviewed for: reading `neg` / `test dl,1` / `jne` / `dec` as
    /// "decrement when odd". The branch is taken when the bit is **set**, so the `dec` it jumps
    /// over runs when the bit is **clear** -- on even widths.
    #[test]
    fn a_jne_over_the_decrement_means_the_decrement_runs_on_even_widths() {
        let mut code = Vec::new();
        code.extend_from_slice(&[0xf7, 0xd9]); // neg ecx
        code.extend_from_slice(&[0xf6, 0xc2, 0x01]); // test dl,1
        code.extend_from_slice(&[0x75, 0x01]); // jne +1  (skips the dec)
        code.extend_from_slice(&[0x49]); // dec ecx
        code.extend_from_slice(&[0xc3]); // ret
        let bytes = image_with_code(&code, &[]);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let (parity, site) = recover_mirror_parity(&image, ENTRY).expect("parity is recovered");
        assert_eq!(parity, Parity::Even);
        assert_eq!(site, ENTRY + 7);
    }

    #[test]
    fn a_je_over_the_decrement_means_the_opposite_parity() {
        let mut code = Vec::new();
        code.extend_from_slice(&[0xf7, 0xd9]); // neg ecx
        code.extend_from_slice(&[0xf6, 0xc2, 0x01]); // test dl,1
        code.extend_from_slice(&[0x74, 0x01]); // je +1
        code.extend_from_slice(&[0x49]); // dec ecx
        code.extend_from_slice(&[0xc3]); // ret
        let bytes = image_with_code(&code, &[]);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let (parity, _) = recover_mirror_parity(&image, ENTRY).expect("parity is recovered");
        assert_eq!(parity, Parity::Odd);
    }

    #[test]
    fn ping_pong_length_and_reflection_describe_one_traversal() {
        // The two rules live at different addresses in the engine and are only useful together:
        // walking the whole cycle length must visit every frame out and back without repeating
        // the endpoints. The mode number is deliberately arbitrary here -- this test is about the
        // arithmetic composing, and `recovers_the_ping_pong_mode_from_the_doubled_cycle_length`
        // is what ties the number to the binary.
        let ping_pong = 4_u8;
        for frames in 1..12_usize {
            let length = cycle_length(ping_pong, ping_pong, frames);
            let walked: Vec<usize> = (0..length)
                .map(|index| {
                    frame_for_cycle_index(ping_pong, ping_pong, frames, index)
                        .expect("inside the cycle")
                })
                .collect();
            let mut expected: Vec<usize> = (0..frames).collect();
            expected.extend((0..frames.saturating_sub(1)).rev());
            assert_eq!(walked, expected, "frames={frames}");
            // Any other mode plays forward once and stops.
            assert_eq!(cycle_length(ping_pong, ping_pong + 1, frames), frames);
        }
    }

    #[test]
    fn a_mirrored_sequence_covers_twice_its_facings_less_the_two_shared_ends() {
        let mirrored = [0, SEQUENCE_MIRROR_BIT, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let plain = [0_u8; 11];
        for facings in 2..10_usize {
            let directions = direction_count(SEQUENCE_MIRROR_BIT, &mirrored, facings);
            assert_eq!(directions, 2 * facings - 2);
            let resolved: Vec<(usize, bool)> = (0..directions)
                .map(|direction| {
                    facing_for_direction(SEQUENCE_MIRROR_BIT, &mirrored, facings, direction)
                        .expect("covered")
                })
                .collect();
            // The first and last stored facings are the two that are never mirrored: they face
            // straight along the axis, so there is nothing to flip them into.
            assert_eq!(resolved[0], (0, false));
            assert_eq!(resolved[facings - 1], (facings - 1, false));
            assert_eq!(
                resolved.iter().filter(|(_, flipped)| *flipped).count(),
                facings - 2
            );
            // Every direction inside the advertised count resolves to a facing that exists.
            assert!(resolved.iter().all(|(facing, _)| *facing < facings));
            // One past the end folds back onto facing 0 rather than being rejected: the engine
            // range-checks after the fold, and reproducing that is the point.
            assert_eq!(
                facing_for_direction(SEQUENCE_MIRROR_BIT, &mirrored, facings, directions),
                Some((0, true))
            );
            assert_eq!(
                direction_count(SEQUENCE_MIRROR_BIT, &plain, facings),
                facings
            );
            assert!(facing_for_direction(SEQUENCE_MIRROR_BIT, &plain, facings, facings).is_none());
        }
    }

    #[test]
    fn the_flipped_placement_reflects_the_established_rule_exactly_on_odd_widths() {
        // `anchor_x` is the x half of the rule in `docs/hotspots.md`, established by a different
        // method months earlier. Reflecting the span it produces about the anchor column must
        // reproduce the engine's flipped value -- and on odd widths it does, exactly. On even
        // widths it lands two pixels away.
        //
        // Scope, stated because it is easy to overread: both sides of this comparison are closed
        // forms in this file, so the `- 2` is **algebra, not measurement**. The engine fact it
        // depends on -- that the `dec` runs on even widths -- is pinned by `recover_mirror_parity`
        // against the instruction stream, which is the right place for it. What this test buys is
        // that the two forms stay in the relationship the engine put them in, so the even case
        // cannot be quietly "fixed" to match the reflection.
        let rules = AnimRules {
            cycle_modes: CycleModeTable {
                advance: 0,
                dispatch_site: 0,
                table_address: 0,
                modes: Vec::new(),
            },
            ping_pong_mode: PING_PONG_MODE,
            ping_pong_length_site: 0,
            ping_pong_reflection_site: 0,
            mirror_bit: SEQUENCE_MIRROR_BIT,
            mirror_test_sites: Vec::new(),
            mirror_decrements_when: Parity::Even,
            mirror_parity_site: 0,
        };
        for width in 1..40_u16 {
            for placement in [-20_i16, -7, 0, 3, 19] {
                let left = anchor_x(width, placement);
                let right_inclusive = left + i32::from(width) - 1;
                let reflected = -right_inclusive;
                let flipped = mirrored_anchor_x(&rules, width, placement);
                if width % 2 == 1 {
                    assert_eq!(flipped, reflected, "width={width} placement={placement}");
                } else {
                    assert_eq!(
                        flipped,
                        reflected - 2,
                        "width={width} placement={placement}"
                    );
                }
            }
        }
    }

    /// The shape every cached-pointer reload in this engine has: load, null-check, dereference.
    ///
    /// `test ecx,ecx` has a register first operand and writes nothing. A walk that cleared taint on
    /// "first operand is a register" erased the pointer at the null check, so `mov cl,[ecx]` was
    /// never seen and the `PLAYER_CACHED_SEQUENCE` source contributed **zero** reads to a scan
    /// whose whole purpose was to find reads. This is `Imp::CycleLength` at
    /// `0x0049D8F7`-`0x0049D8FE`, reduced.
    #[test]
    fn a_null_check_between_a_load_and_a_dereference_does_not_erase_the_pointer() {
        let mut code = Vec::new();
        code.extend_from_slice(&[0x8b, 0x49, 0x24]); // mov ecx,[ecx+24h]
        code.extend_from_slice(&[0x85, 0xc9]); // test ecx,ecx
        code.extend_from_slice(&[0x8a, 0x09]); // mov cl,[ecx]
        code.extend_from_slice(&[0xc3]); // ret
        let bytes = image_with_code(&code, &[]);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let reads = record_pointer_reads(
            &image,
            ENTRY,
            code.len(),
            &[PointerSource::Record { displacement: 0x24 }],
            8,
        )
        .expect("scan runs");
        assert_eq!(reads.len(), 1, "{reads:#x?}");
        assert_eq!(reads[0].address, ENTRY + 5);
        assert_eq!(reads[0].displacement, 0);
    }

    /// `add index, table` carries the pointer just as `add table, index` does.
    ///
    /// `Imp::DirectionCount` forms its record the second way round -- `0x0049D95D add eax,edx` with
    /// the table in `edx` -- so an `Add` arm that only looked at the destination made two reads at
    /// `0x0049D95F` and `0x0049D967` invisible.
    #[test]
    fn an_index_added_to_a_table_carries_the_pointer_either_way_round() {
        for (label, encoding) in [
            // add eax,edx  (index in eax, table in edx)
            ("index + table", vec![0x03, 0xc2]),
            // add edx,eax  (table in edx, index in eax)
            ("table + index", vec![0x03, 0xd0]),
        ] {
            let mut code = Vec::new();
            code.extend_from_slice(&[0x8b, 0x51, 0x1c]); // mov edx,[ecx+1Ch]
            code.extend_from_slice(&[0xc1, 0xe0, 0x04]); // shl eax,4
            code.extend_from_slice(&encoding);
            let read_at = ENTRY + code.len() as u32;
            // test byte [eax+1],80h  /  test byte [edx+1],80h
            let base = if label == "index + table" { 0x40 } else { 0x42 };
            code.extend_from_slice(&[0xf6, base, 0x01, 0x80]);
            code.extend_from_slice(&[0xc3]); // ret
            let bytes = image_with_code(&code, &[]);
            let image = PeImage::parse(&bytes).expect("synthetic image parses");
            let reads = record_pointer_reads(
                &image,
                ENTRY,
                code.len(),
                &[PointerSource::Table { displacement: 0x1c }],
                8,
            )
            .expect("scan runs");
            assert_eq!(reads.len(), 1, "{label}: {reads:#x?}");
            assert_eq!(reads[0].address, read_at, "{label}");
            assert_eq!(reads[0].displacement, 1, "{label}");
        }
    }

    /// A store is not a read, and neither is `lea`.
    ///
    /// The survey was printing `mov [esi+54h],eax` under a heading that said "every field read".
    /// Both cases are decided by `instr_info`'s operand access rather than by a mnemonic list.
    #[test]
    fn stores_and_address_computations_are_not_reported_as_reads() {
        let mut code = Vec::new();
        code.extend_from_slice(&[0x8b, 0x49, 0x24]); // mov ecx,[ecx+24h]   (the source)
        code.extend_from_slice(&[0x89, 0x41, 0x04]); // mov [ecx+4],eax     (a store)
        code.extend_from_slice(&[0x8d, 0x51, 0x08]); // lea edx,[ecx+8]     (no access)
        let read_at = ENTRY + code.len() as u32;
        code.extend_from_slice(&[0x8b, 0x41, 0x0c]); // mov eax,[ecx+0Ch]   (a read)
        code.extend_from_slice(&[0xc3]); // ret
        let bytes = image_with_code(&code, &[]);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let reads = record_pointer_reads(
            &image,
            ENTRY,
            code.len(),
            &[PointerSource::Record { displacement: 0x24 }],
            8,
        )
        .expect("scan runs");
        assert_eq!(reads.len(), 1, "{reads:#x?}");
        assert_eq!(reads[0].address, read_at);
        assert_eq!(reads[0].displacement, 0x0c);

        // `field_reads` applies the same predicate. Displacement 0x24 is outside the range asked
        // for, so the only survivor is the one genuine read at 0x0C: the store at 4 and the `lea`
        // at 8 are both inside the range and both correctly dropped.
        let all = field_reads(&image, ENTRY, code.len(), 0..=15).expect("scan runs");
        let displacements: Vec<u64> = all.iter().map(|read| read.displacement).collect();
        assert_eq!(displacements, vec![0x0c]);
    }

    /// A table base is not a record until an index reaches it.
    #[test]
    fn a_table_base_read_before_any_index_is_not_reported() {
        let mut code = Vec::new();
        code.extend_from_slice(&[0x8b, 0x51, 0x1c]); // mov edx,[ecx+1Ch]
        code.extend_from_slice(&[0xf6, 0x42, 0x01, 0x80]); // test byte [edx+1],80h -- on the table
        code.extend_from_slice(&[0xc3]); // ret
        let bytes = image_with_code(&code, &[]);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let reads = record_pointer_reads(
            &image,
            ENTRY,
            code.len(),
            &[PointerSource::Table { displacement: 0x1c }],
            8,
        )
        .expect("scan runs");
        assert!(reads.is_empty(), "{reads:#x?}");
    }

    /// The installed game. `#[ignore]`d rather than silently skipped: an `eprintln!` from a passing
    /// test is captured by libtest, so `cargo test` printed `ok` and nothing distinguished "ran
    /// and passed" from "could not run". `ignored` is visible in the default summary.
    ///
    /// Run with:
    ///   LOM_GAME_DIR=.../English LOM_LISTFILE=... cargo test --release -- --ignored
    fn game_directory() -> std::path::PathBuf {
        let directory = std::env::var_os("LOM_GAME_DIR")
            .map(std::path::PathBuf::from)
            .expect("set LOM_GAME_DIR to the installed English directory");
        assert!(
            directory.join("lomse.exe").is_file(),
            "no lomse.exe under {}",
            directory.display()
        );
        directory
    }

    #[test]
    #[ignore = "needs LOM_GAME_DIR and LOM_LISTFILE"]
    fn the_recovered_rules_match_the_installed_executable() {
        let directory = game_directory();
        let exe = std::fs::read(directory.join("lomse.exe")).expect("read lomse.exe");
        let image = PeImage::parse(&exe).expect("parse lomse.exe");
        // `recover` refuses when the binary disagrees with this module's constants, so reaching
        // here at all is the check. The site lists are asserted because they are what the
        // documentation cites.
        let rules = recover(&image, &EngineAddresses::default()).expect("rules are recovered");
        assert_eq!(rules.ping_pong_mode, PING_PONG_MODE);
        assert_eq!(rules.mirror_bit, SEQUENCE_MIRROR_BIT);
        assert_eq!(rules.mirror_decrements_when, Parity::Even);
        assert_eq!(rules.cycle_modes.modes.len(), 5);

        let (text_start, _text_offset, text_length) = *image
            .executable_ranges()
            .first()
            .expect("the image has a code section");

        // Part one of the timing negative: the only producer of a sequence-record pointer is
        // called from inside the IMP module and nowhere else, *and* its address is never taken --
        // so no vtable slot or `mov reg,imm32` can reach it either. `call_sites` only sees direct
        // branches, which is exactly why the second assertion has to exist.
        const IMP_MODULE: std::ops::Range<u32> = 0x0049_9000..0x004a_0000;
        let producers =
            call_sites(&image, text_start, text_length, 0x0049_ADB0).expect("enumerate call sites");
        assert!(!producers.is_empty(), "the producer is never called");
        assert!(
            producers.iter().all(|site| IMP_MODULE.contains(site)),
            "a sequence-record pointer is produced outside the IMP module: {producers:#x?}"
        );
        let absolute = absolute_references(&image, 0x0049_ADB0);
        assert!(
            absolute.is_empty(),
            "the producer's address appears as a literal dword, so an indirect call could reach \
             it: {absolute:#x?}"
        );

        // Part two: inside the module, nothing reads the unexplained bytes through such a pointer.
        let tainted = record_pointer_reads(
            &image,
            text_start,
            text_length,
            &[
                PointerSource::Table {
                    displacement: HEADER_SEQUENCE_TABLE,
                },
                PointerSource::Record {
                    displacement: PLAYER_CACHED_SEQUENCE,
                },
            ],
            48,
        )
        .expect("follow record pointers");
        let offending: Vec<&TaintedRead> = tainted
            .iter()
            .filter(|read| {
                IMP_MODULE.contains(&read.address)
                    && UNEXPLAINED_SEQUENCE_BYTES.contains(&read.displacement)
            })
            .collect();
        assert!(
            offending.is_empty(),
            "the engine reads an unexplained sequence byte after all: {offending:#x?}"
        );

        // The rows the documentation quotes. Pinned because both were wrong once: the
        // cached-pointer source contributed nothing until the null-check bug was fixed
        // (displacement 0 read 1, not 2) and `Imp::DirectionCount` was invisible until
        // `add index,table` propagated (displacements 1 and 11 read 2 and 5, not 3 and 6).
        let in_module_at = |displacement: u64| {
            tainted
                .iter()
                .filter(|read| {
                    IMP_MODULE.contains(&read.address) && read.displacement == displacement
                })
                .count()
        };
        assert_eq!(in_module_at(0), 2, "in-module reads at displacement 0");
        assert_eq!(in_module_at(1), 3, "in-module reads at displacement 1");
        assert_eq!(in_module_at(11), 6, "in-module reads at displacement 11");
        assert_eq!(in_module_at(12), 6, "in-module reads at displacement 12");
        assert!(
            tainted.iter().any(|read| {
                read.source_displacement == PLAYER_CACHED_SEQUENCE
                    && IMP_MODULE.contains(&read.address)
            }),
            "the cached-pointer source contributes no in-module reads, which is the symptom of \
             the null-check bug rather than a fact about the engine"
        );

        // Part 2b: no read at an unexplained displacement is reached from a load confirmed to be
        // on an IMP header. This types the out-of-module hits that the module filter merely sets
        // aside, since a displacement cannot type a struct on its own.
        let header_loads: std::collections::BTreeSet<u32> = record_pointer_reads(
            &image,
            text_start,
            text_length,
            &[PointerSource::Record {
                displacement: IMP_FILE_IMAGE,
            }],
            48,
        )
        .expect("follow file-image pointers")
        .iter()
        .filter(|read| read.displacement == HEADER_SEQUENCE_TABLE && read.operand_size == 4)
        .map(|read| read.address)
        .collect();
        assert!(
            header_loads.len() >= 10,
            "only {} header loads found; the chain walk has broken",
            header_loads.len()
        );
        let from_a_header: Vec<&TaintedRead> = tainted
            .iter()
            .filter(|read| {
                UNEXPLAINED_SEQUENCE_BYTES.contains(&read.displacement)
                    && header_loads.contains(&read.source)
            })
            .collect();
        assert!(
            from_a_header.is_empty(),
            "an unexplained byte is read through a confirmed IMP header: {from_a_header:#x?}"
        );
    }

    #[test]
    #[ignore = "needs LOM_GAME_DIR and LOM_LISTFILE"]
    fn the_corpus_matches_the_recovered_rules() {
        let directory = game_directory();
        let exe = std::fs::read(directory.join("lomse.exe")).expect("read lomse.exe");
        let image = PeImage::parse(&exe).expect("parse lomse.exe");
        let rules = recover(&image, &EngineAddresses::default()).expect("rules are recovered");
        let implemented: Vec<u8> = rules.cycle_modes.modes.iter().map(|e| e.mode).collect();

        let archive = crate::mpq::Archive::open(&directory.join("imp.mpq")).expect("open imp.mpq");
        let listfile =
            std::env::var("LOM_LISTFILE").expect("set LOM_LISTFILE alongside LOM_GAME_DIR");
        let names = std::fs::read_to_string(listfile).expect("read listfile");
        let mut sequences = 0_usize;
        let mut ping_pong = 0_usize;
        let mut mirrored = 0_usize;
        let mut exactly_mirror_byte = 0_usize;
        let mut facing_records = 0_usize;
        for name in names.lines() {
            if !name.to_ascii_lowercase().ends_with(".imp") {
                continue;
            }
            let Ok(bytes) = archive.read(name) else {
                continue;
            };
            let Ok(sprite) = crate::imp::ImpSprite::parse(&bytes) else {
                continue;
            };
            for sequence in &sprite.sequences {
                sequences += 1;
                let mode = cycle_mode(&sequence.metadata);
                assert!(
                    implemented.contains(&mode),
                    "{name} uses cycle mode {mode}, which the dispatch at {:#010x} does not cover",
                    rules.cycle_modes.table_address
                );
                if mode == rules.ping_pong_mode {
                    ping_pong += 1;
                }
                if mirrors_facings(rules.mirror_bit, &sequence.metadata) {
                    mirrored += 1;
                }
                if sequence.metadata[SEQUENCE_MIRROR_BYTE] == SEQUENCE_MIRROR_BIT {
                    exactly_mirror_byte += 1;
                }
                let directions =
                    direction_count(rules.mirror_bit, &sequence.metadata, sequence.facing_count);
                for direction in 0..directions {
                    let (facing, _) = facing_for_direction(
                        rules.mirror_bit,
                        &sequence.metadata,
                        sequence.facing_count,
                        direction,
                    )
                            .unwrap_or_else(|| {
                                panic!("{name} advertises {directions} directions but {direction} resolves to nothing")
                            });
                    assert!(
                        facing < sequence.facing_count,
                        "{name} direction {direction}"
                    );
                }
            }
            facing_records += sprite.facings.len();
            for facing in &sprite.facings {
                // The issue's "raw 16-bit field". If a build or a mod ever puts something there,
                // this is the assertion that says so.
                assert_eq!(
                    facing.metadata, 0,
                    "{name} has a nonzero facing metadata word"
                );
            }
        }
        assert_eq!(
            sequences, 4_667,
            "the archive is not the one this was measured on"
        );
        // Pinned too, because the facing count is the figure the "all 14,921 are zero" claim rests
        // on: without it a shrinking corpus would satisfy the zero check vacuously.
        assert_eq!(facing_records, 14_921, "facing records");
        // Measured counts, not restatements of the code. With `PING_PONG_MODE` set to any other
        // value these both collapse -- 3 has no users at all and the ping-pong count would be 0 --
        // which is what makes the constant falsifiable against the shipped archive.
        assert_eq!(ping_pong, 955, "ping-pong sequences");
        // 3,379, not the 2,931 that the byte-1 histogram shows for the value 0x80 exactly. The
        // difference is 448 sequences whose byte 1 is 0x81, 0xCC or 0xFF: the mirror bit is set and
        // other bits are set alongside it. Since the engine tests the bit and never the byte, those
        // 448 mirror. Asserting both numbers keeps that distinction from being lost again.
        assert_eq!(mirrored, 3_379, "sequences with the mirror bit set");
        assert_eq!(
            exactly_mirror_byte, 2_931,
            "sequences whose byte 1 is exactly the mirror bit"
        );
    }
}
