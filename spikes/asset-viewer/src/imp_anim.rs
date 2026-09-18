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

use std::fmt;

use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, Mnemonic, NasmFormatter, OpKind};

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

/// Byte 11 of a sequence record: the facing count, read as `mov cl,[edi+0Bh]` at 0x0049D967.
pub const SEQUENCE_FACING_COUNT_BYTE: usize = 11;

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
pub fn mirrors_facings(metadata: &[u8; 11]) -> bool {
    metadata[SEQUENCE_MIRROR_BYTE] & SEQUENCE_MIRROR_BIT != 0
}

/// How many distinct directions a sequence exposes.
///
/// `Imp::DirectionCount` at 0x0049D920: `2 * facings - 2` when the mirror bit is set
/// (`lea eax,[ecx+ecx-2]`, 0x0049D96A), otherwise the stored facing count (0x0049D976).
pub fn direction_count(metadata: &[u8; 11], facing_count: usize) -> usize {
    if mirrors_facings(metadata) {
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
    if !mirrors_facings(metadata) {
        return None;
    }
    let folded = (2 * facing_count).checked_sub(direction + 2)?;
    (folded < facing_count).then_some((folded, true))
}

/// How many steps the frame index takes before the cycle ends.
///
/// `Imp::CycleLength` at 0x0049D8F0: mode 4 returns `2 * frames - 1`
/// (`lea eax,[edx+edx-1]`, 0x0049D90E); every other mode returns the stored frame count
/// (0x0049D915).
pub fn cycle_length(mode: u8, frame_count: usize) -> usize {
    if mode == PING_PONG_MODE {
        (2 * frame_count).saturating_sub(1)
    } else {
        frame_count
    }
}

/// The mode whose cycle runs forward and then back again.
///
/// Named by two independent sites that both special-case exactly 4: the length rule at 0x0049D906
/// and the reflection at 0x0049AC94.
pub const PING_PONG_MODE: u8 = 4;

/// Which stored frame a position in the cycle shows.
///
/// The reflection is `Imp::GetFrame` at 0x0049AC94..0x0049ACA9: for mode 4, a position at or past
/// the frame count is replaced by `2 * frames - position - 2`.
pub fn frame_for_cycle_index(mode: u8, frame_count: usize, index: usize) -> Option<usize> {
    if frame_count == 0 {
        return None;
    }
    if mode == PING_PONG_MODE && index >= frame_count {
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
    let mut mask = None;
    let mut bound = None;
    for instruction in instructions.iter() {
        if instruction.mnemonic() == Mnemonic::And
            && instruction.op1_kind() == OpKind::Immediate8to32
        {
            mask = Some(instruction.immediate8to32() as u8);
        }
        if instruction.mnemonic() == Mnemonic::Cmp
            && instruction.op1_kind() == OpKind::Immediate8to32
        {
            bound = Some(instruction.immediate8to32());
        }
        if instruction.mnemonic() != Mnemonic::Jmp || instruction.op0_kind() != OpKind::Memory {
            continue;
        }
        if instruction.memory_index_scale() != 4 {
            continue;
        }
        let mask = mask.ok_or_else(|| {
            ImpAnimError::new(format!(
                "the indexed jump at {:#010x} is not preceded by a mask",
                instruction.ip()
            ))
        })?;
        if mask != CYCLE_MODE_MASK {
            return Err(ImpAnimError::new(format!(
                "the mode mask at {advance:#010x} is {mask:#04x}, not {CYCLE_MODE_MASK:#04x}"
            )));
        }
        let bound = bound.ok_or_else(|| {
            ImpAnimError::new(format!(
                "the indexed jump at {:#010x} is not preceded by a range check",
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
    pub text: String,
}

/// Every register-relative memory read in a code range whose displacement falls in `range`.
///
/// This exists to bound a negative. Claiming "the engine never reads sequence byte 2" is only
/// worth something if the search that failed to find such a read was exhaustive, so the scan walks
/// the whole range and reports what it found rather than answering yes or no. Stack-relative
/// operands are excluded: `esp` and `ebp` displacements are locals, not record fields, and they
/// swamp everything else.
pub fn field_reads(
    image: &PeImage<'_>,
    start: u32,
    length: usize,
    range: std::ops::RangeInclusive<u64>,
) -> Result<Vec<FieldRead>, ImpAnimError> {
    use iced_x86::Register;

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
    let mut formatter = NasmFormatter::new();
    let mut text = String::new();
    let mut reads = Vec::new();
    let mut instruction = Instruction::default();
    while decoder.can_decode() {
        decoder.decode_out(&mut instruction);
        let reads_memory =
            (0..instruction.op_count()).any(|index| instruction.op_kind(index) == OpKind::Memory);
        if !reads_memory {
            continue;
        }
        if instruction.memory_index() != Register::None {
            continue;
        }
        if matches!(
            instruction.memory_base(),
            Register::None | Register::ESP | Register::EBP
        ) {
            continue;
        }
        if !range.contains(&instruction.memory_displacement64()) {
            continue;
        }
        // A store writes the field; only loads can be reading a meaning out of it.
        if instruction.op0_kind() == OpKind::Memory && instruction.mnemonic() == Mnemonic::Mov {
            continue;
        }
        text.clear();
        formatter.format(&instruction, &mut text);
        reads.push(FieldRead {
            address: instruction.ip() as u32,
            displacement: instruction.memory_displacement64(),
            operand_size: instruction.memory_size().size(),
            text: text.clone(),
        });
    }
    Ok(reads)
}

/// Offset of the sequence-table pointer inside the loaded IMP header.
///
/// The count sits in the preceding word at 0x1A. Both are read together at 0x0049ADDB /
/// 0x0049ADE3, and they are the same two header fields `ImpSprite::parse` reads from file offsets
/// 26 and 28 -- which is what makes this scan's addressing a control rather than a guess.
pub const HEADER_SEQUENCE_TABLE: u64 = 0x1c;

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
) -> Result<Vec<(u32, bool, String)>, ImpAnimError> {
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
    let instructions: Vec<Instruction> = decoder.iter().collect();
    let mut formatter = NasmFormatter::new();
    let mut text = String::new();
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
        text.clear();
        formatter.format(instruction, &mut text);
        sites.push((instruction.ip() as u32, near_header_load, text.clone()));
    }
    Ok(sites)
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

    /// The installed game, when this machine has one. Set `LOM_GAME_DIR` to the directory holding
    /// `lomse.exe` and `imp.mpq` to turn the corpus tests on; they announce themselves when they
    /// cannot run, because a test that skips in silence is a test nobody notices has stopped.
    fn game_directory() -> Option<std::path::PathBuf> {
        let directory = std::env::var_os("LOM_GAME_DIR")?;
        let directory = std::path::PathBuf::from(directory);
        directory.join("lomse.exe").is_file().then_some(directory)
    }

    #[test]
    fn every_cycle_mode_the_corpus_uses_has_a_slot_in_the_recovered_dispatch() {
        let Some(directory) = game_directory() else {
            eprintln!("skipped: set LOM_GAME_DIR to the installed English directory");
            return;
        };
        let exe = std::fs::read(directory.join("lomse.exe")).expect("read lomse.exe");
        let image = PeImage::parse(&exe).expect("parse lomse.exe");
        let table = recover_cycle_modes(&image, 0x0049_D9A0).expect("recover the dispatch");
        let implemented: Vec<u8> = table.modes.iter().map(|entry| entry.mode).collect();

        let archive = crate::mpq::Archive::open(&directory.join("imp.mpq")).expect("open imp.mpq");
        let listfile =
            std::env::var("LOM_LISTFILE").expect("set LOM_LISTFILE alongside LOM_GAME_DIR");
        let names = std::fs::read_to_string(listfile).expect("read listfile");
        let mut seen = 0_usize;
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
                seen += 1;
                let mode = cycle_mode(&sequence.metadata);
                assert!(
                    implemented.contains(&mode),
                    "{name} uses cycle mode {mode}, which the dispatch at {:#010x} does not cover",
                    table.table_address
                );
                // Every direction the engine advertises has to land on a facing that exists.
                let directions = direction_count(&sequence.metadata, sequence.facing_count);
                for direction in 0..directions {
                    let resolved =
                        facing_for_direction(&sequence.metadata, sequence.facing_count, direction);
                    let (facing, _) = resolved.unwrap_or_else(|| {
                        panic!("{name} advertises {directions} directions but direction {direction} resolves to nothing")
                    });
                    assert!(
                        facing < sequence.facing_count,
                        "{name} direction {direction}"
                    );
                }
            }
            for facing in &sprite.facings {
                // The issue's "raw 16-bit field". If a build or a mod ever puts something there,
                // this is the assertion that says so.
                assert_eq!(
                    facing.metadata, 0,
                    "{name} has a nonzero facing metadata word"
                );
            }
        }
        assert!(
            seen > 1000,
            "only {seen} sequences read; the listfile looks wrong"
        );
    }

    #[test]
    fn ping_pong_length_and_reflection_describe_one_traversal() {
        // The two rules live at different addresses in the engine and are only useful together:
        // walking the whole cycle length must visit every frame out and back without repeating
        // the endpoints. A length or a fold that disagreed would show up as a wrong sequence here.
        for frames in 1..12_usize {
            let length = cycle_length(PING_PONG_MODE, frames);
            let walked: Vec<usize> = (0..length)
                .map(|index| {
                    frame_for_cycle_index(PING_PONG_MODE, frames, index).expect("inside the cycle")
                })
                .collect();
            let mut expected: Vec<usize> = (0..frames).collect();
            expected.extend((0..frames.saturating_sub(1)).rev());
            assert_eq!(walked, expected, "frames={frames}");
        }
    }

    #[test]
    fn a_mirrored_sequence_covers_twice_its_facings_less_the_two_shared_ends() {
        let mirrored = [0, SEQUENCE_MIRROR_BIT, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let plain = [0_u8; 11];
        for facings in 2..10_usize {
            let directions = direction_count(&mirrored, facings);
            assert_eq!(directions, 2 * facings - 2);
            let resolved: Vec<(usize, bool)> = (0..directions)
                .map(|direction| {
                    facing_for_direction(&mirrored, facings, direction).expect("covered")
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
                facing_for_direction(&mirrored, facings, directions),
                Some((0, true))
            );
            assert_eq!(direction_count(&plain, facings), facings);
            assert!(facing_for_direction(&plain, facings, facings).is_none());
        }
    }
}
