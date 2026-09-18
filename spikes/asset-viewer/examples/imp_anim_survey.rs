//! Reproduce the IMP animation-metadata analysis: read the rules out of `lomse.exe`, then check
//! them against every `.imp` in `imp.mpq`.
//!
//! Usage:
//!   cargo run --release --example imp_anim_survey -- lomse.exe imp.mpq LISTFILE
//!
//! The binary half recovers the cycle-mode dispatch and the mirror rule from the instruction
//! stream, and then bounds the negative that matters: it lists *every* register-relative read at a
//! small displacement inside the IMP module, so "nothing reads sequence byte 2" can be checked
//! rather than taken on trust. The corpus half reports what the fields actually hold.
use std::collections::BTreeMap;

use lom_asset_viewer::imp::{ImpHeaderStats, ImpSprite};
use lom_asset_viewer::imp_anim::{
    cycle_mode, direction_count, field_reads, mirrors_facings, recover_cycle_modes, CycleEnd,
    PING_PONG_MODE,
};
use lom_asset_viewer::mpq::Archive;
use lom_asset_viewer::native_table::PeImage;

/// `Imp::Advance`, the function that steps the frame index and ends the cycle.
///
/// Reached from the native operator table: `setimpplayeraction` (0x0049E860) tail-calls
/// `Imp::SetAction` at 0x0049DA80, which calls this at 0x0049DACB.
const ADVANCE: u32 = 0x0049_D9A0;

/// The address range holding the engine's IMP code, used to bound the field-read scan.
///
/// Chosen to cover every function reached from the `imp*` operators in the native table: the
/// lowest is `imp` at 0x0049B690 and the highest is `blankimpplayer` at 0x0049EF60, and the
/// helpers they call (`Imp::GetFrame` 0x0049ABE0 upwards) sit inside the same block.
const IMP_MODULE_START: u32 = 0x0049_9000;
const IMP_MODULE_LENGTH: usize = 0x7000;

fn main() {
    let mut args = std::env::args().skip(1);
    let exe_path = args.next().expect("lomse.exe path");
    let archive_path = args.next().expect("imp.mpq path");
    let listfile = args.next().expect("listfile path");

    let exe = std::fs::read(&exe_path).expect("read executable");
    let image = PeImage::parse(&exe).expect("parse executable");

    println!("== cycle-mode dispatch recovered from the executable");
    let table = recover_cycle_modes(&image, ADVANCE).expect("recover the cycle-mode switch");
    println!(
        "advance={:#010x} dispatch={:#010x} table={:#010x} modes={}",
        table.advance,
        table.dispatch_site,
        table.table_address,
        table.modes.len()
    );
    for entry in &table.modes {
        let note = if entry.mode == PING_PONG_MODE {
            "  (the mode both length sites special-case)"
        } else {
            ""
        };
        println!(
            "  mode {} -> {:#010x}  {}{note}",
            entry.mode, entry.target, entry.end
        );
    }
    let ends: BTreeMap<u8, CycleEnd> = table
        .modes
        .iter()
        .map(|entry| (entry.mode, entry.end))
        .collect();

    println!();
    println!("== register-relative reads at displacements 0..15 inside the IMP module");
    println!("   (the search whose emptiness is the evidence for an unread field)");
    let reads = field_reads(&image, IMP_MODULE_START, IMP_MODULE_LENGTH, 0..=15)
        .expect("scan the IMP module");
    let mut by_key: BTreeMap<(u64, usize), Vec<&str>> = BTreeMap::new();
    for read in &reads {
        by_key
            .entry((read.displacement, read.operand_size))
            .or_default()
            .push(read.text.as_str());
    }
    for ((displacement, size), hits) in &by_key {
        println!(
            "  disp {displacement:>2}  size {size}  n={:<4} e.g. {}",
            hits.len(),
            hits[0]
        );
    }
    println!("  byte-sized reads at each displacement, in full:");
    for displacement in 0..=15_u64 {
        let hits: Vec<&lom_asset_viewer::imp_anim::FieldRead> = reads
            .iter()
            .filter(|read| read.displacement == displacement && read.operand_size == 1)
            .collect();
        println!("    disp {displacement:>2}: {}", hits.len());
        for hit in hits {
            println!("      {:#010x}  {}", hit.address, hit.text);
        }
    }

    println!();
    println!("== sites that form a sequence-record address (index * 16 + header[0x1C])");
    let sites = lom_asset_viewer::imp_anim::sequence_record_sites(
        &image,
        IMP_MODULE_START,
        IMP_MODULE_LENGTH,
    )
    .expect("scan for record addressing");
    for (address, near_header_load, text) in &sites {
        // The window cannot tell a sequence-table index from a frame-table index inside the same
        // function -- both stride by 16 and both sit near the header load. It narrows the set of
        // sites that have to be read by hand, and that set is small enough to read.
        let kind = if *near_header_load {
            "candidate: near a header sequence-table load"
        } else {
            "unrelated 16-byte stride"
        };
        println!("  {address:#010x}  {text:<16} {kind}");
    }

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
    let mut mirror_by_facings: BTreeMap<(bool, usize), usize> = BTreeMap::new();
    let mut directions: BTreeMap<usize, usize> = BTreeMap::new();
    let mut unread_bytes: Vec<BTreeMap<u8, usize>> = vec![BTreeMap::new(); 11];
    let mut facing_metadata: BTreeMap<u16, usize> = BTreeMap::new();
    let mut ragged_facings = Vec::new();
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
            let mirrored = mirrors_facings(&sequence.metadata);
            *mode_counts.entry(mode).or_default() += 1;
            *mirror_by_facings
                .entry((mirrored, sequence.facing_count))
                .or_default() += 1;
            *directions
                .entry(direction_count(&sequence.metadata, sequence.facing_count))
                .or_default() += 1;
            for (offset, value) in sequence.metadata.iter().enumerate() {
                *unread_bytes[offset].entry(*value).or_default() += 1;
            }
            let facings = &sprite.facings
                [sequence.first_facing..sequence.first_facing + sequence.facing_count];
            for facing in facings {
                *facing_metadata.entry(facing.metadata).or_default() += 1;
            }
            // A mirrored sequence has to be able to fill its advertised direction count out of
            // facings that all exist, so the cycle lengths matter per facing, not per sequence.
            let first = facings[0].frame_count;
            if facings.iter().any(|facing| facing.frame_count != first) {
                ragged_facings.push(format!(
                    "{name} seq{index}: {:?}",
                    facings
                        .iter()
                        .map(|facing| facing.frame_count)
                        .collect::<Vec<_>>()
                ));
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
        // If byte 2 were a per-action cadence it would differ between a creature's MOVE and its
        // DIE. If it is an export-time property of the file it will be one value per member.
        let distinct: std::collections::BTreeSet<u8> = sprite
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
    println!("-- cycle modes observed, against the dispatch recovered above");
    for (mode, count) in &mode_counts {
        let end = ends
            .get(mode)
            .map_or("NO JUMP-TABLE SLOT".to_owned(), |end| end.to_string());
        println!("  mode {mode}: n={count:<5} {end}");
    }
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
        "-- sequences whose facings have unequal frame counts: {}",
        ragged_facings.len()
    );
    for line in ragged_facings.iter().take(10) {
        println!("  {line}");
    }
    println!("-- sequence metadata bytes the engine never reads");
    for (offset, hist) in unread_bytes.iter().enumerate() {
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
