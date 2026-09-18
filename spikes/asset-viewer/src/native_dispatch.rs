//! How a native operator reaches the engine object that implements it.
//!
//! The arity walker in [`crate::operator_arity`] answers "how many operands does this operator
//! move?". It deliberately gives up on indirect branches, and one operator in the whole table
//! defeated it outright: `netlockgame`, whose entire body is
//!
//! ```text
//! mov ecx,[5D1E84h]   ; the active network-provider pointer
//! test ecx,ecx
//! je  short ret       ; no provider: do nothing
//! mov eax,[ecx]       ; vtable
//! jmp dword [eax+58h] ; tail call slot 22
//! ret
//! ```
//!
//! That is not a failure of the walker, it is the answer: the operator is one virtual call and the
//! work lives in a C++ class chosen at run time. Recovering *which* object and *which slot* turns
//! a dead end into a map — several operator families in this engine are thin shims over a handful
//! of singletons, and the slot number is the only thing that ties a script-visible name to the
//! implementation that serves it.
//!
//! Two shapes are recognised, and nothing else is guessed at:
//!
//! | Shape | Assembly | Meaning |
//! | --- | --- | --- |
//! | [`Dispatch::VirtualOnGlobalPointer`] | `mov ecx,[abs]` … `mov r,[ecx]` … `call/jmp [r+slot]` | a polymorphic object whose address lives in a global |
//! | [`Dispatch::StaticObjectCall`] | `mov ecx,imm32` … `call/jmp rel32` | a `thiscall` on a statically allocated singleton |
//!
//! Anything else is [`Dispatch::Unrecognised`]. **A recognised dispatch names the object and the
//! slot; it says nothing about what the slot does.** Reading the slot's implementation is a
//! separate act, and the vtables are exposed by [`read_vtable`] and [`constructor_vtable`] so that
//! reading can be reproduced rather than retyped.
//!
//! One trap this encodes, because it already cost a wrong conclusion on this branch: a global that
//! is *read* at a hundred call sites and *written* at none is not necessarily an unassigned
//! pointer. `0x005D1E84` looked like exactly that, and is in fact field `+0x4B2C` of the singleton
//! at `0x005CD358` — the compiler addresses a static object's fields absolutely, so the writer
//! uses a base register and no absolute store exists to find. [`static_object_field`] exists so
//! that check is one call rather than an inference.

use std::collections::BTreeMap;

use iced_x86::{Decoder, DecoderOptions, Instruction, Mnemonic, OpKind, Register};

use crate::native_table::PeImage;

/// Instructions decoded from one entry point before giving up. Dispatch shims are short; the
/// longest in this engine's network module reaches its `call` within a dozen instructions. A
/// generous budget costs nothing and keeps a shim that checks several flags first in scope.
const MAXIMUM_INSTRUCTIONS: usize = 64;

/// How an operator body reaches its implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// A virtual call on an object whose address is held in an absolute global.
    VirtualOnGlobalPointer {
        /// The global holding the object pointer.
        pointer: u32,
        /// Byte offset into the vtable. Slot number is this divided by four.
        slot: u32,
        /// Whether the dispatch was a `jmp` (tail call) rather than a `call`.
        tail_call: bool,
        /// Whether a null check on the pointer was seen before the dispatch. Every network
        /// operator in this engine has one; an operator without one would crash before a session
        /// exists, which is worth seeing rather than assuming.
        guarded: bool,
    },
    /// A non-virtual `thiscall` on the object whose address is held in an absolute global.
    ///
    /// Distinct from [`Dispatch::VirtualOnGlobalPointer`] in exactly the way that matters here:
    /// the implementation is fixed at link time, so it is base-class behaviour that every
    /// transport shares rather than behaviour the active transport chooses.
    MemberOnGlobalPointer {
        /// The global holding the object pointer.
        pointer: u32,
        /// The member function's entry point.
        target: u32,
        /// Whether the dispatch was a `jmp` (tail call) rather than a `call`.
        tail_call: bool,
        /// Whether a null check on the pointer was seen before the dispatch.
        guarded: bool,
    },
    /// A `thiscall` on a statically allocated object, reached by a direct branch.
    StaticObjectCall {
        /// The object's address, materialised as an immediate.
        object: u32,
        /// The member function's entry point.
        target: u32,
        /// Whether the dispatch was a `jmp` (tail call) rather than a `call`.
        tail_call: bool,
    },
    /// Neither shape was found within the budget.
    Unrecognised,
}

impl Dispatch {
    /// A one-word label, so a report and a reader never disagree about the same operator.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::VirtualOnGlobalPointer { .. } => "virtual",
            Self::MemberOnGlobalPointer { .. } => "member-on-pointer",
            Self::StaticObjectCall { .. } => "static-thiscall",
            Self::Unrecognised => "unrecognised",
        }
    }
}

/// Resolve how the operator body at `entry` reaches its implementation.
///
/// The walk is linear and stops at the first `ret` it cannot fall past, so a shim whose dispatch
/// sits behind a taken branch is reported as unrecognised rather than mis-attributed.
pub fn resolve_dispatch(image: &PeImage<'_>, entry: u32) -> Dispatch {
    let Some(offset) = image.file_offset(entry) else {
        return Dispatch::Unrecognised;
    };
    let mut decoder = Decoder::with_ip(
        32,
        &image.bytes()[offset..],
        u64::from(entry),
        DecoderOptions::NONE,
    );

    // `mov ecx,[abs]`: the candidate object pointer, and whether it was null-checked since.
    let mut pointer: Option<u32> = None;
    let mut guarded = false;
    // `mov ecx,imm32`: the candidate statically allocated object.
    let mut static_object: Option<u32> = None;
    // Whether the last write to `ecx` was the global load rather than the immediate.
    let mut ecx_is_pointer = false;
    // `mov r,[ecx]`: the vtable, loaded out of the object.
    let mut vtable_register = Register::None;

    for _ in 0..MAXIMUM_INSTRUCTIONS {
        let instruction = decoder.decode();
        if instruction.is_invalid() {
            return Dispatch::Unrecognised;
        }
        match instruction.mnemonic() {
            Mnemonic::Mov => {
                if let Some(address) = absolute_load_into(&instruction, Register::ECX) {
                    pointer = Some(address);
                    guarded = false;
                    ecx_is_pointer = true;
                    vtable_register = Register::None;
                } else if instruction.op0_kind() == OpKind::Register
                    && instruction.op0_register() == Register::ECX
                    && instruction.op1_kind() == OpKind::Immediate32
                {
                    static_object = Some(instruction.immediate32());
                    ecx_is_pointer = false;
                } else if instruction.op0_kind() == OpKind::Register
                    && instruction.op1_kind() == OpKind::Memory
                    && instruction.memory_base() == Register::ECX
                    && instruction.memory_index() == Register::None
                    && instruction.memory_displacement64() == 0
                {
                    vtable_register = instruction.op0_register();
                }
            }
            // The null check: `test ecx,ecx` or `cmp ecx,reg`, immediately before a conditional
            // branch. Recording the test alone is enough — the branch is what the `je` in every
            // one of these shims does with it, and demanding a specific jcc would reject
            // `cmp ecx,ebx` / `jne` spellings that mean the same thing.
            Mnemonic::Test | Mnemonic::Cmp
                if instruction.op0_kind() == OpKind::Register
                    && instruction.op0_register() == Register::ECX
                    && pointer.is_some() =>
            {
                guarded = true;
            }
            Mnemonic::Call | Mnemonic::Jmp => {
                if instruction.op0_kind() == OpKind::Memory
                    && instruction.memory_index() == Register::None
                    && instruction.memory_base() == vtable_register
                    && vtable_register != Register::None
                    && let Some(pointer) = pointer
                {
                    return Dispatch::VirtualOnGlobalPointer {
                        pointer,
                        slot: instruction.memory_displacement32(),
                        tail_call: instruction.mnemonic() == Mnemonic::Jmp,
                        guarded,
                    };
                }
                if matches!(
                    instruction.op0_kind(),
                    OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64
                ) {
                    let tail_call = instruction.mnemonic() == Mnemonic::Jmp;
                    let target = instruction.near_branch32();
                    // A direct branch with `ecx` holding an immediate is a singleton; with `ecx`
                    // holding a global's value it is a non-virtual member on whatever that global
                    // points at. Whichever `mov` into `ecx` came last is the one in force, so the
                    // later of the two wins rather than a fixed preference deciding it.
                    match (static_object, pointer, ecx_is_pointer) {
                        (Some(object), _, false) => {
                            return Dispatch::StaticObjectCall {
                                object,
                                target,
                                tail_call,
                            };
                        }
                        (_, Some(pointer), true) => {
                            return Dispatch::MemberOnGlobalPointer {
                                pointer,
                                target,
                                tail_call,
                                guarded,
                            };
                        }
                        _ => {}
                    }
                }
            }
            Mnemonic::Ret => return Dispatch::Unrecognised,
            _ => {}
        }
    }
    Dispatch::Unrecognised
}

/// `mov <register>,[absolute]`, with no base or index, or `None` if that is not this instruction.
fn absolute_load_into(instruction: &Instruction, register: Register) -> Option<u32> {
    (instruction.op0_kind() == OpKind::Register
        && instruction.op0_register() == register
        && instruction.op1_kind() == OpKind::Memory
        && instruction.memory_base() == Register::None
        && instruction.memory_index() == Register::None)
        .then(|| instruction.memory_displacement32())
}

/// Read a run of code pointers at `address`, stopping at the first entry that is not code.
///
/// A vtable has no length in the image, so its end can only be inferred. Stopping at the first
/// non-code word is the same rule [`crate::native_table`] uses to bound the operator tables, and
/// it is reported as a length rather than trusted as one.
pub fn read_vtable(image: &PeImage<'_>, address: u32, limit: usize) -> Vec<u32> {
    let Some(mut offset) = image.file_offset(address) else {
        return Vec::new();
    };
    let bytes = image.bytes();
    let mut slots = Vec::new();
    while slots.len() < limit {
        let Some(word) = bytes.get(offset..offset + 4) else {
            break;
        };
        let value = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        if !image.is_code_address(value) {
            break;
        }
        slots.push(value);
        offset += 4;
    }
    slots
}

/// The vtable a constructor installs: the first `mov dword [reg],imm32` in its body.
///
/// This is how a concrete class is identified without guessing. A constructor that chains to a
/// base constructor first still installs its own vtable afterwards, so the *first* such store in
/// the derived constructor is the derived vtable — but only because the chained call is a `call`,
/// not an inlined body. A constructor whose base is inlined would return the base's vtable, so the
/// result is a candidate to be checked against the operators that dispatch through it.
pub fn constructor_vtable(image: &PeImage<'_>, entry: u32) -> Option<u32> {
    let offset = image.file_offset(entry)?;
    let mut decoder = Decoder::with_ip(
        32,
        &image.bytes()[offset..],
        u64::from(entry),
        DecoderOptions::NONE,
    );
    for _ in 0..MAXIMUM_INSTRUCTIONS {
        let instruction = decoder.decode();
        if instruction.is_invalid() || instruction.mnemonic() == Mnemonic::Ret {
            return None;
        }
        if instruction.mnemonic() == Mnemonic::Mov
            && instruction.op0_kind() == OpKind::Memory
            && instruction.memory_index() == Register::None
            && instruction.memory_base() != Register::None
            && instruction.memory_displacement64() == 0
            && instruction.op1_kind() == OpKind::Immediate32
        {
            let candidate = instruction.immediate32();
            if image.file_offset(candidate).is_some() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Every vtable address that some constructor installs, with the sites that install it.
///
/// A vtable has no symbol and no length, so the only structural evidence that a word in read-only
/// data *is* a vtable is that code stores its address into an object's first word. Sweeping the
/// executable sections for `mov dword [reg],imm32` where the immediate resolves to raw data finds
/// those stores. Callers should still require the candidate to be long enough to contain the slots
/// they care about -- this function deliberately does not filter, so the caller's threshold is
/// visible in the caller rather than hidden here.
pub fn installed_vtable_addresses(image: &PeImage<'_>) -> BTreeMap<u32, Vec<u32>> {
    let mut installations: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (virtual_address, raw_offset, raw_size) in image.executable_ranges() {
        let Some(code) = image.bytes().get(raw_offset..raw_offset + raw_size) else {
            continue;
        };
        let mut decoder =
            Decoder::with_ip(32, code, u64::from(virtual_address), DecoderOptions::NONE);
        for instruction in decoder.iter() {
            if instruction.mnemonic() != Mnemonic::Mov
                || instruction.op0_kind() != OpKind::Memory
                || instruction.memory_base() == Register::None
                || instruction.memory_index() != Register::None
                || instruction.memory_displacement64() != 0
                || instruction.op1_kind() != OpKind::Immediate32
            {
                continue;
            }
            let candidate = instruction.immediate32();
            if image.file_offset(candidate).is_some() && !image.is_code_address(candidate) {
                installations
                    .entry(candidate)
                    .or_default()
                    .push(instruction.ip() as u32);
            }
        }
    }
    installations
}

/// Whether `address` falls inside the object that begins at `object_base` and is known to extend
/// at least `known_extent` bytes, and if so which field it is.
///
/// The check that stops an absolute-addressing artefact from being read as a separate global. An
/// address with many reads and no writes is the signature of *either* an unassigned pointer *or* a
/// field of a static object whose writer uses a base register; only the arithmetic distinguishes
/// them.
pub fn static_object_field(object_base: u32, known_extent: u32, address: u32) -> Option<u32> {
    let field = address.checked_sub(object_base)?;
    (field < known_extent).then_some(field)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_table::PeImage;

    const ENTRY: u32 = 0x0040_1000;

    /// A minimal 32-bit PE with one executable section holding `code` at [`ENTRY`], and a second,
    /// non-executable section so that `is_code_address` can actually answer false.
    fn image_with_code(code: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0_u8; 0x1000];
        bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
        let pe = 0x80_usize;
        bytes[pe..pe + 4].copy_from_slice(b"PE\0\0");
        bytes[pe + 6..pe + 8].copy_from_slice(&2_u16.to_le_bytes());
        bytes[pe + 20..pe + 22].copy_from_slice(&0xe0_u16.to_le_bytes());
        bytes[pe + 24..pe + 26].copy_from_slice(&0x10b_u16.to_le_bytes());
        bytes[pe + 52..pe + 56].copy_from_slice(&0x0040_0000_u32.to_le_bytes());

        let table = pe + 24 + 0xe0;
        // .text: virtual 0x1000, raw 0x400, executable.
        bytes[table + 12..table + 16].copy_from_slice(&0x1000_u32.to_le_bytes());
        bytes[table + 16..table + 20].copy_from_slice(&0x200_u32.to_le_bytes());
        bytes[table + 20..table + 24].copy_from_slice(&0x400_u32.to_le_bytes());
        bytes[table + 36..table + 40].copy_from_slice(&0x6000_0020_u32.to_le_bytes());
        // .data: virtual 0x2000, raw 0x600, not executable.
        let second = table + 40;
        bytes[second + 12..second + 16].copy_from_slice(&0x2000_u32.to_le_bytes());
        bytes[second + 16..second + 20].copy_from_slice(&0x200_u32.to_le_bytes());
        bytes[second + 20..second + 24].copy_from_slice(&0x600_u32.to_le_bytes());
        bytes[second + 36..second + 40].copy_from_slice(&0x4000_0040_u32.to_le_bytes());

        bytes[0x400..0x400 + code.len()].copy_from_slice(code);
        bytes
    }

    /// `mov ecx,[0x00402100]`
    const LOAD_POINTER: [u8; 6] = [0x8b, 0x0d, 0x00, 0x21, 0x40, 0x00];
    /// `test ecx,ecx`
    const TEST_ECX: [u8; 2] = [0x85, 0xc9];
    /// `je short +2`
    const JE_SHORT: [u8; 2] = [0x74, 0x02];
    /// `mov eax,[ecx]`
    const LOAD_VTABLE: [u8; 2] = [0x8b, 0x01];
    /// `jmp dword [eax+0x58]`
    const TAIL_SLOT_58: [u8; 3] = [0xff, 0x60, 0x58];
    /// `call dword [eax+0x48]`
    const CALL_SLOT_48: [u8; 3] = [0xff, 0x50, 0x48];
    /// `mov ecx,0x005cd358`
    const LOAD_STATIC: [u8; 5] = [0xb9, 0x58, 0xd3, 0x5c, 0x00];
    /// `ret`
    const RET: [u8; 1] = [0xc3];

    fn resolve(code: &[u8]) -> Dispatch {
        let bytes = image_with_code(code);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        resolve_dispatch(&image, ENTRY)
    }

    #[test]
    fn a_guarded_virtual_tail_call_reports_its_pointer_and_slot() {
        // The shape of `netlockgame`.
        let dispatch = resolve(
            &[
                LOAD_POINTER.as_slice(),
                &TEST_ECX,
                &JE_SHORT,
                &LOAD_VTABLE,
                &TAIL_SLOT_58,
                &RET,
            ]
            .concat(),
        );
        assert_eq!(
            dispatch,
            Dispatch::VirtualOnGlobalPointer {
                pointer: 0x0040_2100,
                slot: 0x58,
                tail_call: true,
                guarded: true,
            }
        );
    }

    #[test]
    fn a_call_is_distinguished_from_a_tail_call_and_the_slot_is_read_from_the_displacement() {
        let dispatch = resolve(
            &[
                LOAD_POINTER.as_slice(),
                &TEST_ECX,
                &JE_SHORT,
                &LOAD_VTABLE,
                &CALL_SLOT_48,
                &RET,
            ]
            .concat(),
        );
        assert_eq!(
            dispatch,
            Dispatch::VirtualOnGlobalPointer {
                pointer: 0x0040_2100,
                slot: 0x48,
                tail_call: false,
                guarded: true,
            }
        );
    }

    #[test]
    fn an_unguarded_virtual_call_is_reported_as_unguarded_rather_than_assumed_safe() {
        let dispatch =
            resolve(&[LOAD_POINTER.as_slice(), &LOAD_VTABLE, &TAIL_SLOT_58, &RET].concat());
        assert_eq!(
            dispatch,
            Dispatch::VirtualOnGlobalPointer {
                pointer: 0x0040_2100,
                slot: 0x58,
                tail_call: true,
                guarded: false,
            }
        );
    }

    #[test]
    fn an_indirect_call_without_a_vtable_load_is_not_read_as_a_virtual_dispatch() {
        // `mov ecx,[abs]` then `jmp [eax+0x58]` with nothing having loaded `eax` from `[ecx]`.
        // Reporting slot 0x58 here would invent a class relationship that the code does not have.
        let dispatch = resolve(&[LOAD_POINTER.as_slice(), &TAIL_SLOT_58, &RET].concat());
        assert_eq!(dispatch, Dispatch::Unrecognised);
    }

    #[test]
    fn a_thiscall_on_a_static_object_reports_the_object_and_the_member() {
        // The shape of `netgamestarted`: `mov ecx,<object>` / `jmp <member>`.
        // `jmp rel32` from 0x00401005 with displacement 6 targets 0x0040100f.
        let dispatch = resolve(
            &[
                LOAD_STATIC.as_slice(),
                &[0xe9, 0x05, 0x00, 0x00, 0x00],
                &RET,
            ]
            .concat(),
        );
        assert_eq!(
            dispatch,
            Dispatch::StaticObjectCall {
                object: 0x005c_d358,
                target: 0x0040_100f,
                tail_call: true,
            }
        );
    }

    #[test]
    fn a_direct_call_on_a_global_pointer_is_a_non_virtual_member_not_a_singleton() {
        // The shape of `thiscomputer`: `mov ecx,[abs]` / null check / `call <base member>`.
        // Reporting this as `StaticObjectCall` would name the *global's address* as the object,
        // which is the pointer's home, not the object.
        // `call rel32` at 0x0040100a with displacement 5 targets 0x00401014.
        let dispatch = resolve(
            &[
                LOAD_POINTER.as_slice(),
                &TEST_ECX,
                &JE_SHORT,
                &[0xe8, 0x05, 0x00, 0x00, 0x00],
                &RET,
            ]
            .concat(),
        );
        assert_eq!(
            dispatch,
            Dispatch::MemberOnGlobalPointer {
                pointer: 0x0040_2100,
                target: 0x0040_1014,
                tail_call: false,
                guarded: true,
            }
        );
    }

    #[test]
    fn the_later_write_to_ecx_decides_which_object_a_direct_branch_belongs_to() {
        // `mov ecx,[abs]` then `mov ecx,imm` then `jmp`: the immediate is in force, so this is a
        // singleton call even though a pointer load was also seen.
        let dispatch = resolve(
            &[
                LOAD_POINTER.as_slice(),
                &LOAD_STATIC,
                &[0xe9, 0x05, 0x00, 0x00, 0x00],
                &RET,
            ]
            .concat(),
        );
        assert_eq!(
            dispatch,
            Dispatch::StaticObjectCall {
                object: 0x005c_d358,
                target: 0x0040_1015,
                tail_call: true,
            }
        );
    }

    #[test]
    fn a_body_that_returns_before_dispatching_is_unrecognised() {
        let dispatch = resolve(&[LOAD_POINTER.as_slice(), &RET, &TAIL_SLOT_58].concat());
        assert_eq!(dispatch, Dispatch::Unrecognised);
    }

    #[test]
    fn a_vtable_ends_at_the_first_word_that_is_not_code() {
        let mut bytes = image_with_code(&RET);
        // Three code pointers then a data pointer, written into .data at virtual 0x00402000.
        let at = 0x600_usize;
        for (index, value) in [0x0040_1000_u32, 0x0040_1010, 0x0040_1020, 0x0040_2500]
            .iter()
            .enumerate()
        {
            bytes[at + index * 4..at + index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        let slots = read_vtable(&image, 0x0040_2000, 40);
        assert_eq!(slots, vec![0x0040_1000, 0x0040_1010, 0x0040_1020]);
    }

    #[test]
    fn a_constructor_reports_the_vtable_it_installs() {
        // `mov esi,ecx` / `mov dword [esi],0x00402000` / `ret`
        let code = [
            [0x8b, 0xf1].as_slice(),
            &[0xc7, 0x06, 0x00, 0x20, 0x40, 0x00],
            &RET,
        ]
        .concat();
        let bytes = image_with_code(&code);
        let image = PeImage::parse(&bytes).expect("synthetic image parses");
        assert_eq!(constructor_vtable(&image, ENTRY), Some(0x0040_2000));
    }

    #[test]
    fn an_address_inside_a_static_object_is_named_as_its_field_not_a_separate_global() {
        // The correction this function exists for: 0x005D1E84 is field +0x4B2C of the singleton at
        // 0x005CD358, whose body is known to reach at least +0x4B78.
        assert_eq!(
            static_object_field(0x005c_d358, 0x4b78, 0x005d_1e84),
            Some(0x4b2c)
        );
        // And one word past the known extent is not claimed.
        assert_eq!(static_object_field(0x005c_d358, 0x4b78, 0x005d_1ed0), None);
    }
}
