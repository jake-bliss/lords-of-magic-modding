//! Survey the engine's multiplayer layer: which operators reach it, which class serves them, and
//! which virtual slots no script-visible name can reach.
//!
//! The question this exists to answer is "is this game lockstep-deterministic or
//! host-authoritative?", and the honest way in is the operator table. Every multiplayer operator
//! in `lomse.exe` is a shim over one polymorphic object, so the interesting code is behind a
//! vtable slot number and not behind a name. This tool recovers the mapping in both directions:
//! operator to slot, and slot to the operators that reach it -- so that the slots reached by *no*
//! operator, which is where a turn-synchronisation method would have to live, are named as a set
//! rather than noticed by accident.
//!
//! Nothing here is hardcoded to the addresses this branch happened to find. The provider pointer
//! comes out of `netlockgame`'s own body, the vtables come from the constructor stores that install
//! them, and the slot span comes from the operators. Change the executable and the report changes
//! with it.
//!
//!     cargo run --release --example multiplayer_survey -- <path to lomse.exe>
//!
//! Optionally pass `--imports` to list the imported module for every call site in the networking
//! DLLs, which is what distinguishes "the transport is DirectPlay" from "the transport is Storm".

use std::collections::BTreeMap;

use lom_asset_viewer::native_dispatch::{
    self, Dispatch, installed_vtable_addresses, read_vtable, resolve_dispatch,
};
use lom_asset_viewer::native_table::{self, PeImage};

/// Slots read out of a candidate vtable. Every vtable in this engine's provider hierarchy is 29
/// slots, so a limit of 64 is slack and still bounds a run of unrelated code pointers.
const VTABLE_SLOT_LIMIT: usize = 64;

/// The operator whose body names the provider pointer. It is the shortest shim in the family -- a
/// null check and a tail call and nothing else -- which is exactly why it is the reliable seed.
const SEED_OPERATOR: &str = "netlockgame";

/// Modules whose imports decide what the transport actually is.
const TRANSPORT_MODULES: [&str; 5] = ["DPLAYX", "STORM", "WSOCK32", "WS2_32", "DPNET"];

fn main() {
    let mut arguments = std::env::args().skip(1);
    let path = arguments.next().unwrap_or_else(|| {
        eprintln!("usage: multiplayer_survey <lomse.exe> [--imports]");
        std::process::exit(2);
    });
    let show_imports = arguments.any(|argument| argument == "--imports");

    let bytes = std::fs::read(&path).expect("read executable");
    let image = PeImage::parse(&bytes).expect("parse 32-bit PE");
    let runs = native_table::extract(&bytes).expect("recover operator tables");
    let operators: Vec<(String, u32)> = runs
        .iter()
        .flat_map(|run| run.entries.iter())
        .map(|entry| (entry.name.to_ascii_lowercase(), entry.entry_point))
        .collect();

    println!("# Multiplayer survey of {path}");
    println!();
    println!(
        "{} operators in {} table run(s)",
        operators.len(),
        runs.len()
    );

    report_imports(&bytes, &image, show_imports);

    // Resolve every operator's dispatch once. Most are ordinary functions and resolve to
    // `Unrecognised`; that is not a failure, it is the absence of a shim.
    let dispatches: Vec<(String, u32, Dispatch)> = operators
        .iter()
        .map(|(name, entry)| (name.clone(), *entry, resolve_dispatch(&image, *entry)))
        .collect();

    let Some(pointer) = seed_pointer(&dispatches) else {
        println!();
        println!("`{SEED_OPERATOR}` did not resolve to a virtual dispatch: nothing to survey.");
        return;
    };
    println!();
    println!("## The provider pointer");
    println!();
    println!("`{SEED_OPERATOR}` dispatches virtually through the pointer at {pointer:#010x}.");

    report_singletons(&dispatches, pointer);
    report_base_members(&dispatches, pointer);
    let slots = report_operator_slots(&dispatches, pointer);
    report_vtables(&image, &slots);
}

/// The provider pointer, taken from the seed operator rather than from a constant.
fn seed_pointer(dispatches: &[(String, u32, Dispatch)]) -> Option<u32> {
    dispatches
        .iter()
        .find_map(|(name, _, dispatch)| match (name.as_str(), dispatch) {
            (SEED_OPERATOR, Dispatch::VirtualOnGlobalPointer { pointer, .. }) => Some(*pointer),
            _ => None,
        })
}

/// Which DLLs the executable imports, and how many call sites reach each networking one.
///
/// A game that plays over Storm's own network layer calls Storm's session functions; a game that
/// plays over DirectPlay calls `DPLAYX`. Counting the call sites is what separates those two
/// readings from the mere presence of a file on disk.
fn report_imports(bytes: &[u8], image: &PeImage<'_>, verbose: bool) {
    println!();
    println!("## Imports");
    println!();
    for (module, functions) in imports(bytes, image) {
        let interesting = TRANSPORT_MODULES
            .iter()
            .any(|candidate| module.to_ascii_uppercase().starts_with(candidate));
        if !interesting && !verbose {
            println!("- {module}: {} function(s)", functions.len());
            continue;
        }
        println!("- **{module}**: {} function(s)", functions.len());
        for (name, thunk, callers) in &functions {
            println!(
                "    - {name} (thunk {thunk:#010x}): {} direct caller(s){}",
                callers.len(),
                if callers.is_empty() {
                    String::new()
                } else {
                    format!(
                        " -- {}",
                        callers
                            .iter()
                            .take(6)
                            .map(|site| format!("{site:#010x}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            );
        }
    }
}

/// Operators that call a statically allocated singleton directly, grouped by object.
///
/// These are the siblings of the virtual family: the network module holds more than one object,
/// and separating them stops "the network state" being treated as one thing when it is several.
fn report_singletons(dispatches: &[(String, u32, Dispatch)], pointer: u32) {
    let mut by_object: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
    for (name, _, dispatch) in dispatches {
        if let Dispatch::StaticObjectCall { object, .. } = dispatch {
            by_object.entry(*object).or_default().push(name);
        }
    }
    // The provider pointer belongs to exactly one of these objects: the nearest base below it.
    // Annotating every object whose address is merely lower would name a dozen owners for one
    // field, which is how an absolute-addressing artefact gets read as a separate global.
    let owner = by_object.keys().copied().rfind(|object| *object <= pointer);

    println!();
    println!("## Statically allocated singletons reached by operators");
    println!();
    for (object, names) in &by_object {
        let containing = (Some(*object) == owner)
            .then(|| native_dispatch::static_object_field(*object, u32::MAX, pointer))
            .flatten()
            .map(|field| format!(" -- **nearest base below the provider pointer, which is its field +{field:#x}**"))
            .unwrap_or_default();
        println!(
            "- {object:#010x} ({} operator(s)){containing}: {}",
            names.len(),
            names.join(", ")
        );
    }
}

/// Operators that call a non-virtual member on the provider pointer.
///
/// Base-class behaviour: the same code runs whichever transport is active. Worth separating from
/// the virtual family because a reader chasing "what does this operator do under DirectPlay?" gets
/// the answer "the same as under every other transport" for free.
fn report_base_members(dispatches: &[(String, u32, Dispatch)], pointer: u32) {
    let mut by_target: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for (name, _, dispatch) in dispatches {
        if let Dispatch::MemberOnGlobalPointer {
            pointer: p, target, ..
        } = dispatch
            && *p == pointer
        {
            by_target.entry(*target).or_default().push(name.clone());
        }
    }
    println!();
    println!("## Operators that call a non-virtual member on the provider pointer");
    println!();
    if by_target.is_empty() {
        println!("None.");
        return;
    }
    println!("| Member | Operators |");
    println!("| ---: | --- |");
    for (target, names) in &by_target {
        println!("| {target:#010x} | {} |", names.join(", "));
    }
}

/// Operator to vtable slot, and the slot span the operators cover.
fn report_operator_slots(
    dispatches: &[(String, u32, Dispatch)],
    pointer: u32,
) -> BTreeMap<u32, Vec<String>> {
    let mut slots: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut unguarded = Vec::new();
    for (name, _, dispatch) in dispatches {
        if let Dispatch::VirtualOnGlobalPointer {
            pointer: p,
            slot,
            guarded,
            ..
        } = dispatch
        {
            if *p != pointer {
                continue;
            }
            slots.entry(*slot).or_default().push(name.clone());
            if !guarded {
                unguarded.push(name.clone());
            }
        }
    }
    println!();
    println!("## Operators by virtual slot");
    println!();
    println!("| Slot | Offset | Operators |");
    println!("| ---: | ---: | --- |");
    for (slot, names) in &slots {
        println!("| {} | +{slot:#04x} | {} |", slot / 4, names.join(", "));
    }
    println!();
    println!(
        "{} operator(s) across {} distinct slot(s).",
        slots.values().map(Vec::len).sum::<usize>(),
        slots.len()
    );
    if unguarded.is_empty() {
        println!(
            "Every one of them null-checks the pointer first, so with no session they are no-ops."
        );
    } else {
        println!("**Not null-checked: {}**", unguarded.join(", "));
    }
    slots
}

/// Candidate vtables, their slots, and which slots no operator can reach.
fn report_vtables(image: &PeImage<'_>, operator_slots: &BTreeMap<u32, Vec<String>>) {
    let highest = operator_slots.keys().copied().max().unwrap_or(0);
    let required = (highest / 4 + 1) as usize;

    let candidates: Vec<(u32, Vec<u32>, usize)> = installed_vtable_addresses(image)
        .into_iter()
        .filter_map(|(address, sites)| {
            let slots = read_vtable(image, address, VTABLE_SLOT_LIMIT);
            (slots.len() >= required).then_some((address, slots, sites.len()))
        })
        .collect();

    println!();
    println!("## Candidate vtables");
    println!();
    println!(
        "Operators reach at most slot {} (+{highest:#04x}), so a vtable for this hierarchy needs \
         at least {required} slots. {} installed vtable(s) qualify.",
        highest / 4,
        candidates.len()
    );
    println!();
    if candidates.is_empty() {
        return;
    }

    let width = candidates.len();
    println!(
        "| Slot | Offset | Operators | {} |",
        candidates
            .iter()
            .map(|(address, _, _)| format!("{address:#010x}"))
            .collect::<Vec<_>>()
            .join(" | ")
    );
    println!("| ---: | ---: | --- |{}", " ---: |".repeat(width));

    // A function that appears at more than one slot, or in more than one class at the same slot,
    // is a shared stub. Naming them stops a shared "return failure" body being read as an
    // implementation.
    let mut occurrences: BTreeMap<u32, usize> = BTreeMap::new();
    for (_, slots, _) in &candidates {
        for slot in slots.iter().take(required) {
            *occurrences.entry(*slot).or_default() += 1;
        }
    }

    let mut unreached = Vec::new();
    for slot in 0..required {
        let offset = (slot * 4) as u32;
        let names = operator_slots
            .get(&offset)
            .map(|names| names.join(", "))
            .unwrap_or_else(|| {
                unreached.push(slot);
                String::from("--")
            });
        let cells: Vec<String> = candidates
            .iter()
            .map(|(_, slots, _)| match slots.get(slot) {
                Some(target) => {
                    let shared = occurrences.get(target).copied().unwrap_or(0) > 1;
                    format!("{target:#010x}{}", if shared { " *" } else { "" })
                }
                None => String::from("--"),
            })
            .collect();
        println!(
            "| {slot} | +{offset:#04x} | {names} | {} |",
            cells.join(" | ")
        );
    }
    println!();
    println!(
        "`*` marks an implementation that appears at more than one (class, slot) pair: a shared stub."
    );
    println!();
    println!(
        "Slots reached by no operator: {}",
        unreached
            .iter()
            .map(|slot| format!("{slot} (+{:#04x})", slot * 4))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!();
    println!("Disassemble those with `cargo run --release --example disasm -- <exe> <address>`.");
}

/// Imported module, function name, import thunk address, and the direct callers of that thunk.
///
/// The linker gives each import a one-instruction `jmp [slot]` thunk, so a call to an imported
/// function is a `call rel32` to the thunk. Counting those callers is how a 250-caller allocator
/// is told apart from a 1-caller session function.
type Imports = Vec<(String, Vec<(String, u32, Vec<u32>)>)>;

fn imports(bytes: &[u8], image: &PeImage<'_>) -> Imports {
    let Some(pe) = read_u32(bytes, 0x3c).map(|value| value as usize) else {
        return Vec::new();
    };
    let Some(base) = read_u32(bytes, pe + 52) else {
        return Vec::new();
    };
    let Some(directory) = read_u32(bytes, pe + 24 + 96 + 8) else {
        return Vec::new();
    };
    let Some(mut cursor) = image.file_offset(base + directory) else {
        return Vec::new();
    };

    // Thunk address to (module, function), built first so that caller discovery is one sweep.
    let mut thunks: BTreeMap<u32, (String, String)> = BTreeMap::new();
    let mut modules: Vec<(String, Vec<(String, u32)>)> = Vec::new();
    while let (Some(lookup), Some(name), Some(address)) = (
        read_u32(bytes, cursor),
        read_u32(bytes, cursor + 12),
        read_u32(bytes, cursor + 16),
    ) {
        if name == 0 && address == 0 {
            break;
        }
        let Some(module) = image.file_offset(base + name).map(|at| c_string(bytes, at)) else {
            break;
        };
        let table = if lookup != 0 { lookup } else { address };
        let mut functions = Vec::new();
        if let Some(mut at) = image.file_offset(base + table) {
            let mut slot = base + address;
            while let Some(value) = read_u32(bytes, at) {
                if value == 0 {
                    break;
                }
                let function = if value & 0x8000_0000 != 0 {
                    format!("#{}", value & 0xffff)
                } else {
                    image
                        .file_offset(base + value)
                        .map(|at| c_string(bytes, at + 2))
                        .unwrap_or_default()
                };
                functions.push((function.clone(), slot));
                thunks.insert(slot, (module.clone(), function));
                at += 4;
                slot += 4;
            }
        }
        modules.push((module, functions));
        cursor += 20;
    }

    // One sweep for the `jmp [slot]` thunk bodies, then one for the `call rel32` sites.
    let mut thunk_address: BTreeMap<u32, u32> = BTreeMap::new();
    let mut callers: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (virtual_address, raw_offset, raw_size) in image.executable_ranges() {
        let Some(code) = bytes.get(raw_offset..raw_offset + raw_size) else {
            continue;
        };
        for index in 0..code.len().saturating_sub(6) {
            if code[index] == 0xff
                && code[index + 1] == 0x25
                && let Some(slot) = read_u32(code, index + 2)
                && thunks.contains_key(&slot)
            {
                thunk_address.insert(slot, virtual_address + index as u32);
            }
        }
        for index in 0..code.len().saturating_sub(5) {
            if code[index] != 0xe8 {
                continue;
            }
            let Some(relative) = read_u32(code, index + 1) else {
                continue;
            };
            let site = virtual_address + index as u32;
            let target = site.wrapping_add(5).wrapping_add(relative);
            callers.entry(target).or_default().push(site);
        }
    }

    modules
        .into_iter()
        .map(|(module, functions)| {
            let resolved = functions
                .into_iter()
                .map(|(function, slot)| {
                    let thunk = thunk_address.get(&slot).copied().unwrap_or(0);
                    let sites = callers.get(&thunk).cloned().unwrap_or_default();
                    (function, thunk, sites)
                })
                .collect();
            (module, resolved)
        })
        .collect()
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn c_string(bytes: &[u8], offset: usize) -> String {
    bytes[offset..]
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as char)
        .collect()
}
