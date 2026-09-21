//! Write a savegame back out through our own encoder, optionally with one field changed.
//!
//! This exists for the C6 attended run: **no save this project has written has ever been loaded
//! by the engine.** Everything the save work claims is composable offline, and offline is where it
//! has stayed.
//!
//! Two modes, which are the two rungs of that run:
//!
//! ```text
//!   save_roundtrip IN.sav OUT.sav                      # rung 0: the control
//!   save_roundtrip IN.sav OUT.sav --set-name SLOT=NAME # rung 1: one visible change
//! ```
//!
//! Rung 0 re-encodes and **asserts the result is byte-identical to the input** before writing it.
//! That makes the file the engine is asked to load a file this encoder produced, while removing
//! "our encoder changed something" as an explanation for anything that goes wrong. A control whose
//! expected result is the shipped file cannot, on its own, show the engine read *our* copy -- which
//! is why rung 1 exists and why the two are read together.
//!
//! Rung 1 changes the player name: 31 bytes at `+0x44` of a `LS_PLR_` record, NUL-terminated
//! inside a **fixed-width field**. It is the only field in that section whose meaning is
//! established rather than Unknown, and it is the only one that shows up on screen without
//! interpreting anything. Because the field is fixed width the edit is length-preserving, so a
//! failure cannot be blamed on the record's shape.
//!
//! The name is written through the decoded `PlayerRecord`, not patched into the payload bytes.
//! `PlayerSection::encode` re-serialises every record rather than replaying `records_raw`, so the
//! edit travels the same path a real authored save would.

use std::path::Path;

use lom_asset_viewer::save::{LordSlot, PlayerRecord, SaveFile};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!(
            "usage: save_roundtrip IN.sav OUT.sav [--set-name SLOT=NAME]\n\
             \n\
             With no --set-name this is the control: the re-encode must be byte-identical to the\n\
             input or nothing is written."
        );
        std::process::exit(2);
    }
    let input_path = Path::new(&args[0]);
    let output_path = Path::new(&args[1]);

    // Repeatable. `quickstart` seats eight lords and nothing here says which one the human
    // plays, so the run renames all of them: any screen that lists a lord then shows one,
    // and the slot-to-lord mapping falls out as a by-product. The control is the unedited
    // file, not an unedited slot, so uniform renaming costs no attribution.
    let mut set_names: Vec<(u32, String)> = Vec::new();
    // `LS_MULT`'s sixteen lord slots carry a name too, in a 32-byte field. A lord's name is
    // stored in at least three places in a save and renaming one of them is how we found that
    // out: the 2026-09-20 run renamed the `LS_PLR_` copy, the Party Roster followed it, and the
    // overworld panel went on showing the original.
    let mut set_lord_names: Vec<(usize, String)> = Vec::new();
    // A raw patch, for a name that lives inside a section this encoder REPLAYS rather than
    // re-serialises -- `LS_SPR_` record bodies are carried verbatim, so there is no decoded field
    // to assign to. Stated as its own flag rather than hidden inside --set-name because the two
    // are different kinds of claim: one authors a field through the model, the other overwrites
    // bytes at an offset. Both still travel through the container encoder.
    let mut patches: Vec<(usize, String)> = Vec::new();
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--set-name" => {
                let spec = args.get(index + 1).unwrap_or_else(|| {
                    eprintln!("--set-name needs SLOT=NAME");
                    std::process::exit(2);
                });
                let (slot, name) = spec.split_once('=').unwrap_or_else(|| {
                    eprintln!("--set-name needs SLOT=NAME, got {spec:?}");
                    std::process::exit(2);
                });
                let slot: u32 = slot.parse().unwrap_or_else(|error| {
                    eprintln!("{slot:?} is not a slot index: {error}");
                    std::process::exit(2);
                });
                // The field is 31 bytes and the engine reads it NUL-terminated, so a name that
                // exactly fills it would be unterminated. Refuse rather than truncate: a silently
                // shortened name is a change nobody asked for arriving in the middle of a
                // measurement.
                if name.len() >= PlayerRecord::NAME_LEN {
                    eprintln!(
                        "{name:?} is {} bytes; the field is {} and needs room for a terminator",
                        name.len(),
                        PlayerRecord::NAME_LEN
                    );
                    std::process::exit(2);
                }
                if !name.is_ascii() {
                    eprintln!("{name:?} is not ASCII; the engine's font is not being tested here");
                    std::process::exit(2);
                }
                set_names.push((slot, name.to_string()));
                index += 2;
            }
            "--set-lord-name" | "--patch-name" => {
                let flag = args[index].clone();
                let spec = args.get(index + 1).unwrap_or_else(|| {
                    eprintln!("{flag} needs KEY=NAME");
                    std::process::exit(2);
                });
                let (key, name) = spec.split_once('=').unwrap_or_else(|| {
                    eprintln!("{flag} needs KEY=NAME, got {spec:?}");
                    std::process::exit(2);
                });
                if !name.is_ascii() {
                    eprintln!("{name:?} is not ASCII");
                    std::process::exit(2);
                }
                if flag == "--set-lord-name" {
                    let slot: usize = key.parse().unwrap_or_else(|error| {
                        eprintln!("{key:?} is not a slot index: {error}");
                        std::process::exit(2);
                    });
                    if name.len() >= LordSlot::NAME_LEN {
                        eprintln!("{name:?} does not leave room for a terminator");
                        std::process::exit(2);
                    }
                    set_lord_names.push((slot, name.to_string()));
                } else {
                    let offset = key
                        .strip_prefix("0x")
                        .map(|hex| usize::from_str_radix(hex, 16))
                        .unwrap_or_else(|| key.parse())
                        .unwrap_or_else(|error| {
                            eprintln!("{key:?} is not an offset: {error}");
                            std::process::exit(2);
                        });
                    patches.push((offset, name.to_string()));
                }
                index += 2;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    let source = std::fs::read(input_path).unwrap_or_else(|error| {
        eprintln!("could not read {}: {error}", input_path.display());
        std::process::exit(1);
    });
    let mut save = SaveFile::parse(&source).unwrap_or_else(|error| {
        eprintln!("could not parse {}: {error}", input_path.display());
        std::process::exit(1);
    });

    // The control assertion runs in BOTH modes. In rung 1 it establishes that the only difference
    // between what we are about to ship and the shipped file is the edit -- without it, "the name
    // changed" and "the encoder rewrote something else too" are the same observation.
    let reencoded = save.encode().unwrap_or_else(|error| {
        eprintln!("could not re-encode {}: {error}", input_path.display());
        std::process::exit(1);
    });
    if reencoded != source {
        eprintln!(
            "REFUSED: the re-encode is not byte-identical to the input ({} bytes in, {} out).\n\
             Nothing was written. This file is not a safe basis for an engine run.",
            source.len(),
            reencoded.len()
        );
        std::process::exit(1);
    }
    println!(
        "control: {} re-encodes byte-identically ({} bytes)",
        input_path.display(),
        source.len()
    );

    let slots: Vec<u32> = save.players.records.iter().map(|r| r.slot_index).collect();
    println!("  player slots present: {slots:?}");
    for record in &save.players.records {
        if let Some(name) = record.name_lossy() {
            println!("    slot {:>2}  name {:?}", record.slot_index, name);
        }
    }

    let output = if set_names.is_empty() && set_lord_names.is_empty() && patches.is_empty() {
        reencoded
    } else {
        for (slot, name) in &set_names {
            let (slot, name) = (*slot, name.as_str());
            let mut hits = 0;
            for record in &mut save.players.records {
                if record.slot_index != slot {
                    continue;
                }
                let field = record.name_raw.as_mut().unwrap_or_else(|| {
                    eprintln!(
                        "slot {slot} carries no name field at this format version; \
                         nothing to change"
                    );
                    std::process::exit(1);
                });
                let before = String::from_utf8_lossy(
                    &field[..field.iter().position(|b| *b == 0).unwrap_or(field.len())],
                )
                .to_string();
                // Whole-field rewrite, not an overwrite of the leading bytes: leaving the old
                // tail in place behind a new terminator would ship bytes nobody chose, and the
                // engine is not the only reader of this file.
                field.fill(0);
                field[..name.len()].copy_from_slice(name.as_bytes());
                println!("  slot {slot}: name {before:?} -> {name:?}");
                hits += 1;
            }
            if hits == 0 {
                eprintln!("no player record carries slot index {slot}; nothing was changed");
                std::process::exit(1);
            }
        }
        for (slot, name) in &set_lord_names {
            let slots = save.multiplayer.slots.as_mut().unwrap_or_else(|| {
                eprintln!("this save's format version stores no lord-slot block");
                std::process::exit(1);
            });
            let entry = slots.get_mut(*slot).unwrap_or_else(|| {
                eprintln!("slot {slot} is outside the sixteen lord slots");
                std::process::exit(1);
            });
            // Occupancy is the lord CODE, never the name: a populated slot may carry an empty
            // one. Renaming an unoccupied slot would write a name into a slot nobody plays.
            if !entry.is_occupied() {
                eprintln!("lord slot {slot} is unoccupied (code 0xFFFFFFFF); refusing to name it");
                std::process::exit(1);
            }
            let before = String::from_utf8_lossy(entry.name()).to_string();
            entry.name_field.fill(0);
            entry.name_field[..name.len()].copy_from_slice(name.as_bytes());
            println!("  lord slot {slot}: name {before:?} -> {name:?}");
        }
        {
            let edited = save.encode().unwrap_or_else(|error| {
                eprintln!("could not encode the edited save: {error}");
                std::process::exit(1);
            });
            if edited.len() != source.len() {
                eprintln!(
                    "REFUSED: the name field is fixed width so the file length must not move, \
                     but it went {} -> {}",
                    source.len(),
                    edited.len()
                );
                std::process::exit(1);
            }
            let differing = source
                .iter()
                .zip(&edited)
                .filter(|(before, after)| before != after)
                .count();
            println!(
                "  edited file differs from the shipped one in {differing} byte(s), same length"
            );
            edited
        }
    };

    // Raw patches land on the ENCODED bytes, after every structural check above has passed and
    // after the length assertion. They are only sound because the encode is byte-identical to the
    // input, which makes an offset measured in the shipped file valid in this one -- so the
    // assertion above is load-bearing for this step, not decoration.
    let mut output = output;
    for (offset, name) in &patches {
        let end = offset + name.len();
        if end > output.len() {
            eprintln!("patch at {offset:#x} runs past the end of the file");
            std::process::exit(1);
        }
        // The old bytes have to be a plausible name, not whatever happens to be there. A
        // mistyped offset otherwise overwrites a length word or a count and ships silently.
        let existing = &output[*offset..end];
        if !existing.iter().all(|byte| byte.is_ascii_alphanumeric()) {
            eprintln!(
                "REFUSED: {:#x} holds {existing:?}, which is not the ASCII text a name patch \
                 expects. Nothing was written.",
                offset
            );
            std::process::exit(1);
        }
        let before = String::from_utf8_lossy(existing).to_string();
        // Equal lengths only. A shorter replacement would need a terminator and this is inside a
        // replayed blob whose field width is not modelled -- so there is nothing here that knows
        // where the field ends.
        if name.len() != existing.len() {
            eprintln!(
                "REFUSED: patch at {offset:#x} is {} bytes over a {}-byte run; \
                 a raw patch must be length-for-length",
                name.len(),
                existing.len()
            );
            std::process::exit(1);
        }
        output[*offset..end].copy_from_slice(name.as_bytes());
        println!("  raw patch {offset:#x}: {before:?} -> {name:?}");
    }

    // Re-parse what we are about to write. An encoder that produces something its own reader
    // rejects is a thing to find here, not in front of the engine with a person waiting.
    let check = SaveFile::parse(&output).unwrap_or_else(|error| {
        eprintln!("REFUSED: the output does not parse back: {error}");
        std::process::exit(1);
    });
    for record in &check.players.records {
        if let Some(name) = record.name_lossy() {
            println!("    readback slot {:>2}  name {:?}", record.slot_index, name);
        }
    }

    std::fs::write(output_path, &output).unwrap_or_else(|error| {
        eprintln!("could not write {}: {error}", output_path.display());
        std::process::exit(1);
    });
    println!("wrote {} ({} bytes)", output_path.display(), output.len());
}
