//! Reproduce the IMP animation-metadata analysis: read the rules out of `lomse.exe`, then check
//! them against every `.imp` in `imp.mpq`.
//!
//! Usage:
//!   cargo run --release --example imp_anim_survey -- lomse.exe imp.mpq LISTFILE
//!
//! The binary half recovers every rule from the instruction stream -- the cycle-mode dispatch, the
//! mode the two ping-pong sites special-case, the mirror bit, and the width parity the mirrored
//! placement corrects for -- and refuses if the binary disagrees with the module it is checking.
//! It then bounds the negative that the timing result rests on, by following sequence-record
//! pointers from the two places they are created and reporting every field read through them. The
//! corpus half reports what the fields actually hold.
use std::collections::{BTreeMap, BTreeSet};

use iced_x86::{Formatter, Instruction, NasmFormatter};
use lom_asset_viewer::imp::{ImpHeaderStats, ImpSprite};
use lom_asset_viewer::imp_anim::{
    CycleEnd, EngineAddresses, HEADER_SEQUENCE_TABLE, IMP_FILE_IMAGE, PLAYER_CACHED_SEQUENCE,
    PointerSource, UNEXPLAINED_SEQUENCE_BYTES, absolute_references, call_sites, cycle_mode,
    direction_count, field_reads, mirrored_anchor_x, mirrors_facings, record_pointer_reads,
    recover, sequence_record_sites,
};
use lom_asset_viewer::mpq::Archive;
use lom_asset_viewer::native_table::PeImage;

/// `Imp::GetSequence(action)`: the only function that returns a sequence-record pointer.
const GET_SEQUENCE: u32 = 0x0049_ADB0;

/// Rendering lives here, not in the library: nothing in `imp_anim` consumes prose, and keeping
/// `iced-x86`'s `nasm` tables a dev-dependency keeps them out of the SDL viewer and the map editor.
fn render(formatter: &mut NasmFormatter, instruction: &Instruction) -> String {
    let mut text = String::new();
    formatter.format(instruction, &mut text);
    text
}

fn main() {
    let mut args = std::env::args().skip(1);
    let exe_path = args.next().expect("lomse.exe path");
    let archive_path = args.next().expect("imp.mpq path");
    let listfile = args.next().expect("listfile path");

    let exe = std::fs::read(&exe_path).expect("read executable");
    let image = PeImage::parse(&exe).expect("parse executable");
    let mut formatter = NasmFormatter::new();
    let addresses = EngineAddresses::default();
    let rules = recover(&image, &addresses).expect("recover the animation rules");
    // Taken from the image rather than hardcoded, so "the whole of .text" means it. The size is
    // the section's file-backed extent; its 309-byte virtual tail is loader zero-fill.
    let (text_start, _text_offset, text_length) = *image
        .executable_ranges()
        .first()
        .expect("the image has a code section");
    // The module span bounds the *reporting*, not the negative. It is an operator entry-point
    // range, not a call-graph closure; part 1 below is what closes the graph.
    let module = addresses.module.clone();
    let module_length = (module.end - module.start) as usize;
    let in_module = |address: u32| module.contains(&address);

    println!("== rules recovered from the executable");
    let table = &rules.cycle_modes;
    println!(
        "advance={:#010x} dispatch={:#010x} table={:#010x} modes={}",
        table.advance,
        table.dispatch_site,
        table.table_address,
        table.modes.len()
    );
    for entry in &table.modes {
        let note = if entry.mode == rules.ping_pong_mode {
            "  <- ping-pong: doubled length + reflection"
        } else {
            ""
        };
        println!(
            "  mode {} -> {:#010x}  {}{note}",
            entry.mode, entry.target, entry.end
        );
    }
    println!(
        "ping-pong mode {} from the length site {:#010x} and the reflection {:#010x}",
        rules.ping_pong_mode, rules.ping_pong_length_site, rules.ping_pong_reflection_site
    );
    println!(
        "mirror bit {:#04x}, tested at {}",
        rules.mirror_bit,
        rules
            .mirror_test_sites
            .iter()
            .map(|site| format!("{site:#010x}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!(
        "mirrored placement subtracts one more pixel on {} widths ({:#010x})",
        rules.mirror_decrements_when, rules.mirror_parity_site
    );
    println!("  flipped anchor-relative x, worked through for a few widths:");
    for width in [7_u16, 8, 9, 32, 33] {
        println!(
            "    width {width:>3} placement -4  ->  {}",
            mirrored_anchor_x(&rules, width, -4)
        );
    }
    let ends: BTreeMap<u8, CycleEnd> = table
        .modes
        .iter()
        .map(|entry| (entry.mode, entry.end))
        .collect();

    println!();
    println!("== the negative, part 1: who can obtain a sequence-record pointer");
    println!("   `Imp::GetSequence` ({GET_SEQUENCE:#010x}) is the only function that returns one.");
    let producers =
        call_sites(&image, text_start, text_length, GET_SEQUENCE).expect("enumerate call sites");
    let outside: Vec<u32> = producers
        .iter()
        .copied()
        .filter(|site| !in_module(*site))
        .collect();
    for site in &producers {
        println!("   called from {site:#010x}");
    }
    println!(
        "   {} direct call sites, {} outside the IMP module",
        producers.len(),
        outside.len()
    );
    // `call_sites` only sees NearBranch32 operands, so on its own it leaves the indirect route
    // open. An address that never appears as a literal dword cannot be loaded into a register or
    // sit in a vtable slot, and that is what makes the direct enumeration exhaustive.
    let absolute = absolute_references(&image, GET_SEQUENCE);
    println!(
        "   absolute dword references to {GET_SEQUENCE:#010x} anywhere in the file: {}",
        absolute.len()
    );
    for offset in absolute.iter().take(8) {
        println!("     at file offset {offset:#x}");
    }
    println!(
        "   => {}",
        if absolute.is_empty() && outside.is_empty() {
            "closed: no indirect route exists and every direct caller is in-module"
        } else {
            "NOT closed: read the rows above"
        }
    );

    println!();
    println!("== the negative, part 2: every field read through a sequence-record pointer");
    println!("   sources: header[{HEADER_SEQUENCE_TABLE:#x}] as a table (an index must be added");
    println!(
        "            before a read counts) and player[{PLAYER_CACHED_SEQUENCE:#x}] as a record."
    );
    println!("   Displacements cannot type a struct, so out-of-module hits are offset collisions");
    println!("   on unrelated objects; part 1 is what rules them out. The in-module rows decide.");
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
    let mut by_displacement: BTreeMap<u64, Vec<&lom_asset_viewer::imp_anim::TaintedRead>> =
        BTreeMap::new();
    for read in &tainted {
        by_displacement
            .entry(read.displacement)
            .or_default()
            .push(read);
    }
    for (displacement, reads) in &by_displacement {
        let inside: Vec<_> = reads
            .iter()
            .filter(|read| in_module(read.address))
            .collect();
        let unexplained = if UNEXPLAINED_SEQUENCE_BYTES.contains(displacement) {
            "  <- an unexplained byte"
        } else {
            ""
        };
        println!(
            "  disp {displacement:>3}  total={:<4} in-module={:<3}{unexplained}",
            reads.len(),
            inside.len()
        );
        for read in inside {
            println!(
                "      {:#010x}  {:<34} via {:#010x} [{:#x}]",
                read.address,
                render(&mut formatter, &read.instruction),
                read.source,
                read.source_displacement
            );
        }
    }
    // Part 2b: typing the out-of-module hits instead of waving at them.
    //
    // A `[x+0x1C]` load is only an IMP header load if `x` is an IMP file image, and a file image is
    // only ever obtained from an `Imp` object's field 8 (`0x0049ADB7`). So follow that field and
    // collect the `[x+0x1C]` loads it reaches; any header load outside that set is a load on some
    // other struct, whatever its displacement happens to be.
    let header_loads: BTreeSet<u32> = record_pointer_reads(
        &image,
        text_start,
        text_length,
        &[PointerSource::Record {
            displacement: IMP_FILE_IMAGE,
        }],
        48,
    )
    .expect("follow imp file-image pointers")
    .iter()
    .filter(|read| read.displacement == HEADER_SEQUENCE_TABLE && read.operand_size == 4)
    .map(|read| read.address)
    .collect();
    println!();
    println!("== the negative, part 2b: which header loads are really on an IMP header");
    println!(
        "   `[x+{IMP_FILE_IMAGE:#x}]` reaches {} distinct `[x+{HEADER_SEQUENCE_TABLE:#x}]` loads",
        header_loads.len()
    );
    for address in &header_loads {
        println!(
            "     {address:#010x}{}",
            if in_module(*address) { "" } else { "  OUTSIDE" }
        );
    }
    let unexplained_from_a_real_header: Vec<_> = tainted
        .iter()
        .filter(|read| {
            UNEXPLAINED_SEQUENCE_BYTES.contains(&read.displacement)
                && header_loads.contains(&read.source)
        })
        .collect();
    println!(
        "   reads at the unexplained displacements whose source is a confirmed header load: {}",
        unexplained_from_a_real_header.len()
    );
    for read in &unexplained_from_a_real_header {
        println!(
            "     {:#010x}  {}",
            read.address,
            render(&mut formatter, &read.instruction)
        );
    }

    let unexplained_in_module = tainted
        .iter()
        .filter(|read| {
            UNEXPLAINED_SEQUENCE_BYTES.contains(&read.displacement) && in_module(read.address)
        })
        .count();
    println!(
        "  in-module reads at the unexplained displacements {:?}: {unexplained_in_module}",
        UNEXPLAINED_SEQUENCE_BYTES
    );

    println!();
    println!("== reporting scan: base-relative reads at displacements 0..15 in the IMP module");
    println!("   (indexed operands and `ebp` bases are INCLUDED; only absolute and esp-based");
    println!("    operands and stores are excluded)");
    let reads = field_reads(&image, module.start, module_length, 0..=15).expect("scan the module");
    let mut by_key: BTreeMap<(u64, usize, bool), usize> = BTreeMap::new();
    for read in &reads {
        *by_key
            .entry((read.displacement, read.operand_size, read.indexed))
            .or_default() += 1;
    }
    for ((displacement, size, indexed), count) in &by_key {
        let shape = if *indexed { "base+index" } else { "base only " };
        println!("  disp {displacement:>2}  size {size}  {shape}  n={count}");
    }
    println!("  byte-sized reads at each displacement, in full:");
    for displacement in 0..=15_u64 {
        let hits: Vec<&lom_asset_viewer::imp_anim::FieldRead> = reads
            .iter()
            .filter(|read| read.displacement == displacement && read.operand_size == 1)
            .collect();
        println!("    disp {displacement:>2}: {}", hits.len());
        for hit in hits {
            println!(
                "      {:#010x}  {}",
                hit.address,
                render(&mut formatter, &hit.instruction)
            );
        }
    }

    println!();
    println!("== sites that scale an index by 16 near a header sequence-table load");
    println!("   scanned over the whole of .text, not over the module: bounding this to the");
    println!("   module would make \"the inline formation sites are in-module\" true by");
    println!("   construction and therefore worthless as evidence.");
    let sites =
        sequence_record_sites(&image, text_start, text_length).expect("scan record addressing");
    let candidates: Vec<_> = sites.iter().filter(|site| site.near_header_load).collect();
    for site in &candidates {
        // The window cannot tell a sequence-table index from a frame-table index inside the same
        // function -- both stride by 16 and both sit near the header load. This narrows the set
        // that has to be read by hand; it is not by itself the negative.
        let inside = if in_module(site.address) {
            "in-module"
        } else {
            "OUTSIDE THE MODULE"
        };
        println!(
            "  {:#010x}  {:<16} {inside}",
            site.address,
            render(&mut formatter, &site.instruction)
        );
    }
    println!(
        "  {} sites stride by 16 in all of .text; {} sit near a header sequence-table load; {} of \
         those are outside the module",
        sites.len(),
        candidates.len(),
        candidates
            .iter()
            .filter(|site| !in_module(site.address))
            .count()
    );

    println!();
    println!("== corpus");
    let archive = Archive::open(std::path::Path::new(&archive_path)).expect("open archive");
    let names: Vec<String> = std::fs::read_to_string(&listfile)
        .expect("read listfile")
        .lines()
        .map(str::to_owned)
        .collect();

    let mut labels: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    for name in &names {
        if !name.to_ascii_lowercase().ends_with(".h") {
            continue;
        }
        let Ok(bytes) = archive.read(name) else {
            continue;
        };
        let Ok(stats) = ImpHeaderStats::parse(&bytes) else {
            continue;
        };
        labels.insert(
            stats.sequence_name.to_ascii_lowercase(),
            stats.sequence_labels,
        );
    }

    let mut members = 0_usize;
    let mut sequences = 0_usize;
    let mut mode_counts: BTreeMap<u8, usize> = BTreeMap::new();
    let mut mirror_set = 0_usize;
    let mut mirror_by_facings: BTreeMap<(bool, usize), usize> = BTreeMap::new();
    let mut directions: BTreeMap<usize, usize> = BTreeMap::new();
    let mut metadata_bytes: Vec<BTreeMap<u8, usize>> = vec![BTreeMap::new(); 11];
    let mut control_high_bits: BTreeMap<u8, usize> = BTreeMap::new();
    let mut facing_metadata: BTreeMap<u16, usize> = BTreeMap::new();
    let mut ragged_facings = Vec::new();
    let mut empty_facings = Vec::new();
    let mut frame_total_disagreements = Vec::new();
    let mut byte2_by_label: BTreeMap<String, BTreeMap<u8, usize>> = BTreeMap::new();
    let mut members_with_one_byte2 = 0_usize;
    let mut members_with_many_byte2 = 0_usize;
    let mut mode_by_label: BTreeMap<String, BTreeMap<u8, usize>> = BTreeMap::new();

    for name in &names {
        if !name.to_ascii_lowercase().ends_with(".imp") {
            continue;
        }
        let Ok(bytes) = archive.read(name) else {
            continue;
        };
        let Ok(sprite) = ImpSprite::parse(&bytes) else {
            continue;
        };
        members += 1;
        let stem = name
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(name)
            .trim_end_matches(".imp")
            .trim_end_matches(".IMP")
            .to_ascii_lowercase();
        let member_labels = labels.get(&stem);
        for (index, sequence) in sprite.sequences.iter().enumerate() {
            sequences += 1;
            let mode = cycle_mode(&sequence.metadata);
            let mirrored = mirrors_facings(rules.mirror_bit, &sequence.metadata);
            *mode_counts.entry(mode).or_default() += 1;
            if mirrored {
                mirror_set += 1;
            }
            // A standing structural cross-check: the decoder's per-sequence frame total has to be
            // the sum of its facings'. If the two levels ever disagree the record walk has slipped.
            let facing_frame_total: usize = sprite.facings
                [sequence.first_facing..sequence.first_facing + sequence.facing_count]
                .iter()
                .map(|facing| facing.frame_count)
                .sum();
            if facing_frame_total != sequence.frame_count {
                frame_total_disagreements.push(format!("{name} seq{index}"));
            }
            *mirror_by_facings
                .entry((mirrored, sequence.facing_count))
                .or_default() += 1;
            *directions
                .entry(direction_count(
                    rules.mirror_bit,
                    &sequence.metadata,
                    sequence.facing_count,
                ))
                .or_default() += 1;
            for (offset, value) in sequence.metadata.iter().enumerate() {
                *metadata_bytes[offset].entry(*value).or_default() += 1;
            }
            *control_high_bits
                .entry(sequence.metadata[0] & !0x07)
                .or_default() += 1;
            let facings = &sprite.facings
                [sequence.first_facing..sequence.first_facing + sequence.facing_count];
            for facing in facings {
                *facing_metadata.entry(facing.metadata).or_default() += 1;
            }
            // A sequence with no facings is refused by the decoder today, but surveying mods is
            // the stated purpose of this tool, so it reports the anomaly instead of panicking.
            match facings.first() {
                None => empty_facings.push(format!("{name} seq{index}")),
                Some(first) => {
                    if facings
                        .iter()
                        .any(|facing| facing.frame_count != first.frame_count)
                    {
                        ragged_facings.push(format!(
                            "{name} seq{index}: {:?}",
                            facings
                                .iter()
                                .map(|facing| facing.frame_count)
                                .collect::<Vec<_>>()
                        ));
                    }
                }
            }
            let label = member_labels
                .and_then(|table| table.get(index))
                .and_then(|names| names.first())
                .cloned()
                .unwrap_or_else(|| "(unlabelled)".to_owned());
            *byte2_by_label
                .entry(label.clone())
                .or_default()
                .entry(sequence.metadata[2])
                .or_default() += 1;
            *mode_by_label
                .entry(label)
                .or_default()
                .entry(mode)
                .or_default() += 1;
        }
        let distinct: BTreeSet<u8> = sprite
            .sequences
            .iter()
            .map(|sequence| sequence.metadata[2])
            .collect();
        if distinct.len() > 1 {
            members_with_many_byte2 += 1;
        } else {
            members_with_one_byte2 += 1;
        }
    }

    println!("members={members} sequences={sequences}");
    println!(
        "-- sequence frame total vs the sum over its facings: {} disagreements",
        frame_total_disagreements.len()
    );
    println!("-- cycle modes observed, against the dispatch recovered above");
    for (mode, count) in &mode_counts {
        let end = ends
            .get(mode)
            .map_or("NO JUMP-TABLE SLOT".to_owned(), |end| end.to_string());
        let note = if *mode == rules.ping_pong_mode {
            "  (ping-pong)"
        } else {
            ""
        };
        println!("  mode {mode}: n={count:<5} {end}{note}");
    }
    println!("  sequences with the mirror bit set: {mirror_set}");
    println!("-- mirror bit against facing count");
    for ((mirrored, facings), count) in &mirror_by_facings {
        println!(
            "  mirror={mirrored:<5} facings={facings:<3} -> directions={:<3} n={count}",
            if *mirrored {
                (2 * facings).saturating_sub(2)
            } else {
                *facings
            }
        );
    }
    println!("-- direction counts");
    for (count, n) in &directions {
        println!("  {count:>3} directions: n={n}");
    }
    println!("-- facing metadata word (the issue's 'raw 16-bit field')");
    for (value, count) in &facing_metadata {
        println!("  {value:#06x}: n={count}");
    }
    println!(
        "-- sequences with no facings: {}  with unequal facing frame counts: {}",
        empty_facings.len(),
        ragged_facings.len()
    );
    for line in ragged_facings.iter().take(10) {
        println!("  {line}");
    }
    println!("-- sequence metadata byte distributions");
    for (offset, hist) in metadata_bytes.iter().enumerate() {
        let mut top: Vec<_> = hist.iter().collect();
        top.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        let shown: Vec<String> = top
            .iter()
            .take(12)
            .map(|(value, count)| format!("{value:#04x}:{count}"))
            .collect();
        println!(
            "  byte {offset:>2} distinct={:<4} {}",
            hist.len(),
            shown.join(" ")
        );
    }
    println!("-- byte 0 bits 3-7, the part the engine masks away");
    for (bits, count) in &control_high_bits {
        println!("  {bits:#04x}: n={count}");
    }
    println!(
        "-- byte 2: members holding one value={members_with_one_byte2} \
         members holding several={members_with_many_byte2}"
    );
    println!("-- byte 2 by action label (is it a per-action cadence?)");
    for (label, hist) in &byte2_by_label {
        let mut top: Vec<_> = hist.iter().collect();
        top.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        if top.iter().map(|(_, count)| **count).sum::<usize>() < 40 {
            continue;
        }
        let shown: Vec<String> = top
            .iter()
            .take(8)
            .map(|(value, count)| format!("{value}:{count}"))
            .collect();
        println!("  {label:<18} {}", shown.join(" "));
    }
    println!("-- cycle mode by action label");
    for (label, hist) in &mode_by_label {
        if hist.values().sum::<usize>() < 40 {
            continue;
        }
        let shown: Vec<String> = hist
            .iter()
            .map(|(mode, count)| format!("mode{mode}:{count}"))
            .collect();
        println!("  {label:<18} {}", shown.join(" "));
    }
}
