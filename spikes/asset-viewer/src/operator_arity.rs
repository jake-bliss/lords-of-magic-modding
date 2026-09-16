//! Recovery of how many operands each engine operator consumes and produces.
//!
//! Every native reaches the interpreter's operand stack through the same inlined idiom. The
//! interpreter context arrives as the function's first argument, and three of its fields matter:
//!
//! | Offset | Meaning |
//! | --- | --- |
//! | `+0x50` | base of the operand array, whose entries are an eight-byte `(tag, value)` pair |
//! | `+0x54` | current index, which counts **down** as values are pushed |
//! | `+0x58` | the limit index, compared against `+0x54` to detect underflow |
//!
//! A pop increments the index and stores it back; a push decrements it and stores it back:
//!
//! ```text
//! pop:   mov eax,[esi+0x54] ; cmp against [esi+0x58] ; inc eax ; ... ; mov [esi+0x54],eax
//! push:  mov eax,[esi+0x54] ; test eax,eax          ; dec eax ; ... ; mov [esi+0x54],eax
//! ```
//!
//! The `inc`/`dec` is not always adjacent to the commit — the compiler interleaves unrelated stores
//! — so the register is tracked through the block instead: loaded from the field, adjusted, then
//! stored back. Any other write to that register abandons the tracking, so an unrelated value
//! reaching the field is reported rather than counted.
//!
//! Counting those commits recovers stack effect. **The count is a static site count, not a proven
//! arity.** A function whose commits all lie on one path through the body consumes exactly that
//! many operands; a function that pops different amounts on different branches does not, and is
//! reported as branching so the distinction is never silently lost.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use iced_x86::{
    Code, Decoder, DecoderOptions, Instruction, InstructionInfoFactory, Mnemonic, OpAccess, OpKind,
    Register,
};

use crate::native_table::{NativeTableError, PeImage};

/// Context field holding the operand-stack index.
const STACK_INDEX_FIELD: u64 = 0x54;

/// Upper bound on the instructions decoded for one operator. Several operators are thunks that tail
/// call their implementation far away in the section, so the budget is on work done rather than on
/// distance from the entry point. Reaching it is reported rather than hidden.
const MAXIMUM_INSTRUCTIONS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackEffect {
    /// Sites that commit a pop. Equal to the operand count when `branching` is false.
    pub pops: usize,
    /// Sites that commit a push. Equal to the result count when `branching` is false.
    pub pushes: usize,
    /// Stores to the stack index that matched neither idiom. Any nonzero value means the two
    /// patterns above did not describe this function, so its counts should not be trusted.
    pub unclassified: usize,
    /// Whether any conditional branch was walked. Nearly every operator branches, because the
    /// pop idiom itself contains an underflow check, so this is descriptive rather than a
    /// confidence signal.
    pub branching: bool,
    /// Whether the walk hit `MAXIMUM_INSTRUCTIONS` rather than exhausting the body.
    pub truncated: bool,
    /// Instructions decoded while walking.
    pub instructions: usize,
}

impl StackEffect {
    /// Whether the walk completed and recognised every store it saw.
    ///
    /// This says the analysis did not give up, **not** that the counts are the operator's arity.
    /// A body that pops different amounts on different paths still produces a site count.
    pub fn is_well_formed(&self) -> bool {
        !self.truncated && self.unclassified == 0
    }
}

/// How many levels of `call` are followed. Operators routinely push their result through a shared
/// helper rather than inline, so a body-only walk undercounts pushes systematically. One level
/// reaches those helpers; going deeper would start attributing unrelated engine work to the
/// operator.
const CALL_DEPTH: usize = 1;

/// Walk one operator body from its entry point and count its operand-stack traffic.
pub fn stack_effect(image: &PeImage<'_>, entry_point: u32) -> Result<StackEffect, NativeTableError> {
    stack_effect_to_depth(image, entry_point, CALL_DEPTH)
}

fn stack_effect_to_depth(
    image: &PeImage<'_>,
    entry_point: u32,
    depth: usize,
) -> Result<StackEffect, NativeTableError> {
    if !image.is_code_address(entry_point) {
        return Err(NativeTableError::new(format!(
            "operator entry point {entry_point:#010x} is not inside a code section"
        )));
    }

    let mut visited = BTreeSet::<u32>::new();
    let mut queue = VecDeque::from([entry_point]);
    let mut effect = StackEffect {
        pops: 0,
        pushes: 0,
        unclassified: 0,
        branching: false,
        truncated: false,
        instructions: 0,
    };
    // Commits are counted per address so that a block reached from two predecessors is not
    // double-counted.
    let mut counted = BTreeSet::<u32>::new();
    // Each distinct callee contributes once, however many call sites reach it.
    let mut callees = BTreeSet::<u32>::new();

    while let Some(block_start) = queue.pop_front() {
        if !visited.insert(block_start) {
            continue;
        }
        let Some(offset) = image.file_offset(block_start) else {
            continue;
        };
        let Some(bytes) = image.bytes().get(offset..) else {
            continue;
        };
        let mut decoder = Decoder::with_ip(
            32,
            bytes,
            u64::from(block_start),
            DecoderOptions::NONE,
        );

        // Per-block tracking of registers that hold an adjusted copy of the stack index.
        let mut tracked = BTreeMap::<Register, Adjustment>::new();
        let mut info_factory = InstructionInfoFactory::new();
        while decoder.can_decode() {
            let instruction = decoder.decode();
            let address = instruction.ip() as u32;
            if instruction.is_invalid() {
                break;
            }
            if effect.instructions >= MAXIMUM_INSTRUCTIONS {
                effect.truncated = true;
                break;
            }
            effect.instructions += 1;

            if is_stack_index_store(&instruction) {
                if counted.insert(address) {
                    match tracked.get(&instruction.op1_register()) {
                        Some(Adjustment::Increment) => effect.pops += 1,
                        Some(Adjustment::Decrement) => effect.pushes += 1,
                        Some(Adjustment::Loaded) | None => effect.unclassified += 1,
                    }
                }
            } else if is_stack_index_load(&instruction) {
                tracked.insert(instruction.op0_register(), Adjustment::Loaded);
            } else if let Some((destination, adjusted)) = lea_adjustment(&instruction, &tracked) {
                // The compiler also writes the adjustment as `lea dst,[src+1]`, which computes the
                // same value into a different register.
                tracked.insert(destination, adjusted);
            } else if let Some(adjusted) = adjustment(&instruction) {
                let register = instruction.op0_register();
                if matches!(tracked.get(&register), Some(Adjustment::Loaded)) {
                    tracked.insert(register, adjusted);
                } else {
                    tracked.remove(&register);
                }
            } else {
                // Any other write to a tracked register means it no longer holds the index.
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

            if instruction.mnemonic() == Mnemonic::Call
                && depth > 0
                && let Some(target) = branch_target(&instruction)
                && target != entry_point
                && callees.insert(target)
                && image.is_code_address(target)
                && let Ok(callee) = stack_effect_to_depth(image, target, depth - 1)
            {
                effect.pops += callee.pops;
                effect.pushes += callee.pushes;
                effect.unclassified += callee.unclassified;
                effect.instructions += callee.instructions;
                effect.truncated |= callee.truncated;
            }

            match instruction.mnemonic() {
                Mnemonic::Ret => break,
                Mnemonic::Jmp => {
                    if let Some(target) = branch_target(&instruction) {
                        queue.push_back(target);
                    }
                    break;
                }
                _ => {}
            }
            if instruction.is_jcc_short_or_near() {
                effect.branching = true;
                if let Some(target) = branch_target(&instruction) {
                    queue.push_back(target);
                }
            }
        }
    }
    Ok(effect)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Adjustment {
    /// Freshly loaded from the stack-index field, not yet adjusted.
    Loaded,
    Increment,
    Decrement,
}

/// Whether an instruction is `inc`/`dec` of a 32-bit register, which is how the index is adjusted
/// before being committed.
fn adjustment(instruction: &Instruction) -> Option<Adjustment> {
    if instruction.op0_kind() != OpKind::Register {
        return None;
    }
    match instruction.mnemonic() {
        Mnemonic::Inc => Some(Adjustment::Increment),
        Mnemonic::Dec => Some(Adjustment::Decrement),
        // `add r32, 1` and `sub r32, 1` are the same operation written differently.
        Mnemonic::Add if instruction.immediate(1) == 1 => Some(Adjustment::Increment),
        Mnemonic::Sub if instruction.immediate(1) == 1 => Some(Adjustment::Decrement),
        _ => None,
    }
}

/// Recognise `lea dst,[tracked+1]` and `lea dst,[tracked-1]`, the compiler's other spelling of the
/// index adjustment. Returns the destination register and which way the index moved.
fn lea_adjustment(
    instruction: &Instruction,
    tracked: &BTreeMap<Register, Adjustment>,
) -> Option<(Register, Adjustment)> {
    if instruction.mnemonic() != Mnemonic::Lea || instruction.memory_index() != Register::None {
        return None;
    }
    if !matches!(
        tracked.get(&instruction.memory_base()),
        Some(Adjustment::Loaded)
    ) {
        return None;
    }
    match instruction.memory_displacement64() as i64 {
        1 => Some((instruction.op0_register(), Adjustment::Increment)),
        -1 => Some((instruction.op0_register(), Adjustment::Decrement)),
        _ => None,
    }
}

/// Whether an instruction loads the context's stack-index field into a register.
fn is_stack_index_load(instruction: &Instruction) -> bool {
    instruction.code() == Code::Mov_r32_rm32
        && instruction.op1_kind() == OpKind::Memory
        && instruction.memory_base() != Register::None
        && instruction.memory_index() == Register::None
        && instruction.memory_displacement64() == STACK_INDEX_FIELD
}

/// Whether an instruction stores a register into the context's stack-index field.
fn is_stack_index_store(instruction: &Instruction) -> bool {
    instruction.code() == Code::Mov_rm32_r32
        && instruction.op0_kind() == OpKind::Memory
        && instruction.memory_base() != Register::None
        && instruction.memory_index() == Register::None
        && instruction.memory_displacement64() == STACK_INDEX_FIELD
}

fn branch_target(instruction: &Instruction) -> Option<u32> {
    matches!(
        instruction.op0_kind(),
        OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64
    )
    .then(|| instruction.near_branch32())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_table::PeImage;

    /// Wrap a run of machine code in a minimal PE so the walker can be exercised without the
    /// proprietary binary.
    fn image_with_code(code: &[u8]) -> Vec<u8> {
        const PE_OFFSET: usize = 0x80;
        const IMAGE_BASE: u32 = 0x0040_0000;
        const CODE_VA: u32 = 0x1000;
        const CODE_RAW: u32 = 0x200;
        const CODE_SIZE: u32 = 0x400;

        let mut image = vec![0_u8; (CODE_RAW + CODE_SIZE) as usize];
        image[0x3c..0x40].copy_from_slice(&(PE_OFFSET as u32).to_le_bytes());
        image[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        image[PE_OFFSET + 6..PE_OFFSET + 8].copy_from_slice(&1_u16.to_le_bytes());
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

        image[CODE_RAW as usize..CODE_RAW as usize + code.len()].copy_from_slice(code);
        image
    }

    const ENTRY: u32 = 0x0040_1000;
    /// `mov eax,[esi+0x54]`
    const LOAD_INDEX: [u8; 3] = [0x8b, 0x46, 0x54];
    /// `inc eax`
    const INC_EAX: [u8; 1] = [0x40];
    /// `dec eax`
    const DEC_EAX: [u8; 1] = [0x48];
    /// `mov [esi+0x54],eax`
    const STORE_INDEX: [u8; 3] = [0x89, 0x46, 0x54];
    /// `ret`
    const RET: [u8; 1] = [0xc3];

    fn assemble(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    #[test]
    fn counts_a_single_pop_as_one_operand() {
        let code = assemble(&[&LOAD_INDEX, &INC_EAX, &STORE_INDEX, &RET]);
        let bytes = image_with_code(&code);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let effect = stack_effect(&image, ENTRY).expect("entry is code");
        assert_eq!(effect.pops, 1);
        assert_eq!(effect.pushes, 0);
        assert_eq!(effect.unclassified, 0);
        assert!(effect.is_well_formed());
    }

    #[test]
    fn counts_a_push_separately_from_a_pop() {
        // The shape of `dup`: take one operand, return two.
        let code = assemble(&[
            &LOAD_INDEX,
            &INC_EAX,
            &STORE_INDEX,
            &LOAD_INDEX,
            &DEC_EAX,
            &STORE_INDEX,
            &LOAD_INDEX,
            &DEC_EAX,
            &STORE_INDEX,
            &RET,
        ]);
        let bytes = image_with_code(&code);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let effect = stack_effect(&image, ENTRY).expect("entry is code");
        assert_eq!((effect.pops, effect.pushes), (1, 2));
        assert!(effect.is_well_formed());
    }

    #[test]
    fn a_store_without_an_adjustment_is_reported_rather_than_guessed() {
        // `mov [esi+0x54],eax` with no preceding inc/dec: the idiom does not apply, and the
        // function must not be reported as if it did.
        let code = assemble(&[&LOAD_INDEX, &STORE_INDEX, &RET]);
        let bytes = image_with_code(&code);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let effect = stack_effect(&image, ENTRY).expect("entry is code");
        assert_eq!(effect.unclassified, 1);
        assert_eq!((effect.pops, effect.pushes), (0, 0));
        assert!(!effect.is_well_formed(), "an unclassified store is not well formed");
    }

    #[test]
    fn a_conditional_branch_marks_the_result_inexact_and_both_paths_are_walked() {
        // Two complete pop sequences, one on each side of a branch. The displacement of 5 from
        // the instruction after the two-byte jcc at offset 3 targets offset 10.
        let code = assemble(&[
            &LOAD_INDEX,
            &[0x7d, 0x05],
            &INC_EAX,
            &STORE_INDEX,
            &RET,
            &LOAD_INDEX,
            &INC_EAX,
            &STORE_INDEX,
            &RET,
        ]);
        let bytes = image_with_code(&code);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let effect = stack_effect(&image, ENTRY).expect("entry is code");
        assert!(effect.branching, "a jcc was walked");

        assert_eq!(effect.pops, 2, "both sides of the branch are counted");
        assert_eq!(effect.unclassified, 0);
    }

    #[test]
    fn a_block_reached_twice_is_counted_once() {
        // Two conditional branches into the same complete pop sequence.
        let code = assemble(&[
            &[0x7d, 0x02],
            &[0x7c, 0x00],
            &LOAD_INDEX,
            &INC_EAX,
            &STORE_INDEX,
            &RET,
        ]);
        let bytes = image_with_code(&code);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let effect = stack_effect(&image, ENTRY).expect("entry is code");
        assert_eq!(effect.pops, 1, "the shared commit is counted once");
    }

    #[test]
    fn tolerates_an_unrelated_store_between_the_adjustment_and_the_commit() {
        // The shipped `pop` operator writes an error slot between `inc eax` and the commit, so
        // adjacency alone does not recognise it.
        const STORE_OTHER_FIELD: [u8; 6] = [0x89, 0xbe, 0x90, 0x00, 0x00, 0x00];
        let code = assemble(&[
            &LOAD_INDEX,
            &INC_EAX,
            &STORE_OTHER_FIELD,
            &STORE_INDEX,
            &RET,
        ]);
        let bytes = image_with_code(&code);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let effect = stack_effect(&image, ENTRY).expect("entry is code");
        assert_eq!((effect.pops, effect.pushes, effect.unclassified), (1, 0, 0));
    }

    #[test]
    fn an_overwritten_register_is_no_longer_the_stack_index() {
        // `xor eax,eax` between the load and the commit means the stored value is not the index.
        let code = assemble(&[&LOAD_INDEX, &INC_EAX, &[0x33, 0xc0], &STORE_INDEX, &RET]);
        let bytes = image_with_code(&code);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let effect = stack_effect(&image, ENTRY).expect("entry is code");
        assert_eq!(effect.unclassified, 1, "the commit is reported, not counted");
        assert_eq!((effect.pops, effect.pushes), (0, 0));
    }

    #[test]
    fn recognises_the_lea_spelling_of_the_adjustment() {
        // The comparison operators write `lea ecx,[eax+1]` rather than `inc eax`, computing the
        // same value into a different register.
        const LEA_ECX_EAX_PLUS_1: [u8; 3] = [0x8d, 0x48, 0x01];
        const STORE_INDEX_ECX: [u8; 3] = [0x89, 0x4e, 0x54];
        let code = assemble(&[&LOAD_INDEX, &LEA_ECX_EAX_PLUS_1, &STORE_INDEX_ECX, &RET]);
        let bytes = image_with_code(&code);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let effect = stack_effect(&image, ENTRY).expect("entry is code");
        assert_eq!((effect.pops, effect.pushes, effect.unclassified), (1, 0, 0));
    }

    #[test]
    fn counts_a_shared_push_helper_once_however_many_sites_call_it() {
        // Operators push their result through a helper. Two call sites on exclusive paths are one
        // result, so the callee contributes once.
        const HELPER_OFFSET: usize = 0x40;
        let helper_va = ENTRY + HELPER_OFFSET as u32;
        // call rel32 to the helper, twice, then ret.
        let mut code = Vec::new();
        code.extend_from_slice(&[0xe8]);
        let first_call_end = 5_u32;
        code.extend_from_slice(&((HELPER_OFFSET as u32 - first_call_end).to_le_bytes()));
        code.extend_from_slice(&[0xe8]);
        let second_call_end = 10_u32;
        code.extend_from_slice(&((HELPER_OFFSET as u32 - second_call_end).to_le_bytes()));
        code.extend_from_slice(&RET);
        code.resize(HELPER_OFFSET, 0x90);
        code.extend_from_slice(&LOAD_INDEX);
        code.extend_from_slice(&DEC_EAX);
        code.extend_from_slice(&STORE_INDEX);
        code.extend_from_slice(&RET);

        let bytes = image_with_code(&code);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let effect = stack_effect(&image, ENTRY).expect("entry is code");
        assert_eq!(effect.pushes, 1, "the helper contributes once, not per call site");
        assert!(helper_va > ENTRY);
    }

    #[test]
    fn rejects_an_entry_point_outside_the_code_section() {
        let bytes = image_with_code(&assemble(&[&RET]));
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let error = stack_effect(&image, 0x0000_1234).expect_err("address is not code");
        assert!(error.to_string().contains("not inside a code section"), "{error}");
    }
}
