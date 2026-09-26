//! Report, or change, which player each `LS_USER` record is bound to. Marauder probe, rung 2.
//!
//! ```text
//!   save_bind_user --check IN
//!   save_bind_user IN OUT --record R --player P --expect E
//! ```
//!
//! `--check` prints one line per user record: its index, the file offset of its `+0` word and the
//! word's value. `docs/save-format.md` records that word as "the record's own index"; read in the
//! engine it is the **player the user is bound to** -- `currentuser` (`0x004E3DF0`) returns it for
//! the current user record, `setuserforplayer` (`0x0052CEF0`) searches the eight records for it,
//! and the load routine (`0x0052D090`) `fread`s all 784 bytes of each record straight into the
//! in-memory user table, so the word survives a load. See `docs/marauder-probe.md`.
//!
//! The edit goes through the save model, like `save_roundtrip`: parse, require the re-encode to
//! be byte-identical to the input, change the one word in the decoded `UserRecord`, encode, and
//! then require that the output differs from the input **only inside those four bytes**, has the
//! same length, and parses back with the new binding and every other binding unchanged. The format
//! has no checksum and no compression (`docs/save-format.md`), and `LS_USER` is replayed verbatim,
//! so nothing else needs to move. Refuses rather than writes on anything unexpected.
//!
//! Writes nothing but OUT, and refuses if OUT exists. Keeping a backup of IN is the caller's job
//! (`tools/marauder_save_bind.py` does it).

use std::path::Path;

use lom_asset_viewer::save::{SaveFile, SectionTag, UserRecord, UserSection};

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("REFUSED: {message}");
    std::process::exit(1);
}

fn parse_checked(source: &[u8], label: &str) -> SaveFile {
    let save = SaveFile::parse(source).unwrap_or_else(|error| fail(format!("{label}: {error}")));
    let reencoded = save
        .encode()
        .unwrap_or_else(|error| fail(format!("{label}: could not re-encode: {error}")));
    if reencoded != source {
        fail(format!(
            "{label}: the re-encode is not byte-identical to the file ({} bytes in, {} out)",
            source.len(),
            reencoded.len()
        ));
    }
    if save.users.records.len() != UserSection::RECORD_COUNT {
        fail(format!("{label}: {} user records, expected 8", save.users.records.len()));
    }
    save
}

fn binding(record: &UserRecord) -> u32 {
    u32::from_le_bytes(record.raw[0..4].try_into().expect("four bytes"))
}

fn bindings(save: &SaveFile) -> Vec<u32> {
    save.users.records.iter().map(binding).collect()
}

fn word_offset(save: &SaveFile, record: usize) -> usize {
    save.container.location(SectionTag::User).payload_offset + record * UserRecord::LEN
}

fn print_bindings(save: &SaveFile) {
    for (index, record) in save.users.records.iter().enumerate() {
        let value = binding(record);
        println!(
            "record {index}  offset {:#x}  player {}",
            word_offset(save, index),
            value as i32
        );
    }
}

fn number(args: &[String], flag: &str) -> u32 {
    let position = args
        .iter()
        .position(|arg| arg == flag)
        .unwrap_or_else(|| fail(format!("missing {flag}")));
    let text = args
        .get(position + 1)
        .unwrap_or_else(|| fail(format!("{flag} needs a value")));
    text.parse()
        .unwrap_or_else(|error| fail(format!("{flag} {text:?}: {error}")))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 2 && args[0] == "--check" {
        let source = std::fs::read(&args[1])
            .unwrap_or_else(|error| fail(format!("could not read {}: {error}", args[1])));
        let save = parse_checked(&source, &args[1]);
        println!("file {} ({} bytes)", args[1], source.len());
        print_bindings(&save);
        return;
    }
    if args.len() != 8 {
        eprintln!(
            "usage: save_bind_user --check IN\n       \
             save_bind_user IN OUT --record R --player P --expect E"
        );
        std::process::exit(2);
    }
    let input = Path::new(&args[0]);
    let output = Path::new(&args[1]);
    let record = number(&args, "--record") as usize;
    let player = number(&args, "--player");
    let expect = number(&args, "--expect");
    if record >= UserSection::RECORD_COUNT {
        fail(format!("record {record} is outside the eight user records"));
    }
    if player >= 16 {
        fail(format!("player {player} is outside the sixteen player slots"));
    }
    if output.exists() {
        fail(format!("{} already exists", output.display()));
    }

    let source =
        std::fs::read(input).unwrap_or_else(|error| fail(format!("could not read input: {error}")));
    let mut save = parse_checked(&source, &args[0]);
    let before = bindings(&save);
    println!("before: {before:?}");
    if before[record] != expect {
        fail(format!(
            "record {record} is bound to {}, not the expected {expect}",
            before[record] as i32
        ));
    }
    if before.contains(&player) {
        fail(format!("player {player} already has a user record: {before:?}"));
    }

    save.users.records[record].raw[0..4].copy_from_slice(&player.to_le_bytes());
    let edited = save
        .encode()
        .unwrap_or_else(|error| fail(format!("could not encode the edit: {error}")));

    if edited.len() != source.len() {
        fail(format!("length moved {} -> {}", source.len(), edited.len()));
    }
    let offset = word_offset(&save, record);
    let outside: Vec<usize> = source
        .iter()
        .zip(&edited)
        .enumerate()
        .filter(|(at, (a, b))| a != b && !(offset..offset + 4).contains(at))
        .map(|(at, _)| at)
        .collect();
    if !outside.is_empty() {
        fail(format!("bytes changed outside the word at {offset:#x}: {outside:?}"));
    }
    if edited[offset..offset + 4] != player.to_le_bytes() {
        fail(format!("the word at {offset:#x} does not hold {player}"));
    }

    let check = parse_checked(&edited, "output");
    let after = bindings(&check);
    let mut wanted = before.clone();
    wanted[record] = player;
    if after != wanted {
        fail(format!("readback {after:?}, expected {wanted:?}"));
    }

    std::fs::write(output, &edited)
        .unwrap_or_else(|error| fail(format!("could not write {}: {error}", output.display())));
    println!("after:  {after:?}");
    println!("word    {offset:#x}: {expect} -> {player}");
    println!("wrote   {} ({} bytes)", output.display(), edited.len());
}
