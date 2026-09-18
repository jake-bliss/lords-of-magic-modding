//! Regression anchors for the recovered state model.
//!
//! Three kinds of check, deliberately different.
//!
//! **Relations read off the committed tables.** These run without the proprietary binary and fail
//! if the join starts attributing offsets to the wrong object.
//!
//! **A cross-check against a document written from a different instrument.** `docs/save-format.md`
//! records that the save writer copies a 164-byte block from `[gameobj+0x520]` and a player name
//! from `[player+0x50ac]`. Those offsets were recovered by reading the save *file* and the writer
//! by hand; this analysis recovers them by walking call sites. Agreement is two instruments, and it
//! is pinned here so an analyser change that loses it fails rather than being noticed later.
//!
//! **Checks against the binary**, `#[ignore]`d so a machine without `lomse.exe` reports them as
//! ignored rather than as passed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the crate sits two directories below the repository root")
}

fn rows(name: &str) -> Vec<BTreeMap<String, String>> {
    let path = repository_root().join("reports/natives/state").join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is committed: {error}", path.display()));
    let mut lines = text.lines();
    let columns: Vec<&str> = lines
        .next()
        .expect("the table has a header")
        .split('\t')
        .collect();
    lines
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(
                fields.len(),
                columns.len(),
                "row {:?} has {} fields against {} columns",
                fields.first(),
                fields.len(),
                columns.len()
            );
            columns
                .iter()
                .zip(fields.iter())
                .map(|(column, field)| ((*column).to_owned(), (*field).to_owned()))
                .collect()
        })
        .collect()
}

/// Fields of one structure.
///
/// Keyed on base **and kind**, because the base alone is not a key: six addresses in this image are
/// reached both as an object (`lea`/immediate) and as a pointer to one (`mov` from the address),
/// and they are two different candidate structures. A first version of this helper keyed on the
/// base and merged them, which produced a field past its own structure's extent — the failure that
/// found this.
fn fields_of(base: &str, kind: &str) -> BTreeMap<u32, BTreeMap<String, String>> {
    rows("structure-fields.tsv")
        .into_iter()
        .filter(|row| row["base"] == base && row["base_kind"] == kind)
        .map(|row| {
            let offset = u32::from_str_radix(row["offset"].trim_start_matches("0x"), 16)
                .expect("offsets are hexadecimal");
            (offset, row)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The independent cross-check: the savegame documentation was written from other evidence
// ---------------------------------------------------------------------------------------------

/// `docs/save-format.md`: "164 bytes game-setup struct, copied from `[gameobj+0x520]`".
///
/// That offset was read out of the save writer by hand while decoding the file format. This
/// analysis reaches it from the other end — the operator call graph — and must land on the same
/// number. It is recovered as a **width-0** field, because the instruction is `lea` and the block
/// is never dereferenced at that address; a version of the analyser that ignored `lea` reported
/// this field as absent, which is the regression this test exists for.
#[test]
fn the_game_objects_setup_block_is_where_the_save_format_documentation_puts_it() {
    let fields = fields_of("0x005aa12c", "static");
    let block = fields
        .get(&0x520)
        .expect("docs/save-format.md puts the LS_MULT setup block at [gameobj+0x520]");
    assert!(
        block["widths"].split('|').any(|width| width == "0"),
        "the block is address-taken, not dereferenced, so it must carry width 0: {block:?}"
    );
}

/// `docs/save-format.md`: the engine `strcpy`s a name from `[player_i + 0x50AC]`.
///
/// It comes out here as an offset of the *game* object, because the engine forms the player pointer
/// as `lea ecx,[gameobj+eax*4]` — an interior pointer into an embedded array — and this analysis
/// does not recover the stride. So the agreement is on the offset **within the record**, and the
/// field must be marked `indexed` to say the containing element is not known.
#[test]
fn the_player_name_offset_agrees_and_is_marked_as_an_array_element() {
    let fields = fields_of("0x005aa12c", "static");
    let name = fields
        .get(&0x50ac)
        .expect("docs/save-format.md puts a player name at [player+0x50ac]");
    assert_eq!(
        name["indexed"], "indexed",
        "an offset reached through `lea base+index*n` is an offset within element zero and must \
         not be published as a scalar field: {name:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Controls: bases established by other work in this repository
// ---------------------------------------------------------------------------------------------

/// `0x005d1e84` was established as the DirectPlay session object by the vtable-slot recovery in
/// `docs/native-operator-bodies.md`, from a different mechanism — virtual-dispatch taint, not field
/// access. If the subject-word convergence is measuring anything, `net` must pick that base out.
#[test]
fn the_network_subject_converges_on_the_object_the_vtable_work_already_named() {
    let best = rows("subject-convergence.tsv")
        .into_iter()
        .filter(|row| row["subject"] == "net")
        .max_by_key(|row| row["matching"].parse::<usize>().expect("a count"))
        .expect("the `net` subject appears");
    assert_eq!(best["base"], "0x005d1e84");
}

/// The counterweight to the control above, and the finding this document leads with.
///
/// If a per-unit or per-army record were reachable, the operators carrying those words would
/// converge on one base the way the network operators do. They do not. Pinning the ceiling means a
/// future change that *does* recover such a record breaks this test, which is the intent: it is
/// written to be falsified.
#[test]
fn no_entity_subject_converges_the_way_the_network_one_does() {
    let mut best: BTreeMap<String, usize> = BTreeMap::new();
    for row in rows("subject-convergence.tsv") {
        let matching: usize = row["matching"].parse().expect("a count");
        let total: usize = row["operators_with_word"].parse().expect("a count");
        let share = 100 * matching / total.max(1);
        let entry = best.entry(row["subject"].clone()).or_default();
        *entry = (*entry).max(share);
    }
    for subject in ["army", "unit", "city", "player", "artifact", "building"] {
        let share = best[subject];
        assert!(
            share < 25,
            "{subject} now converges at {share}% on one base. If that is real it is a recovered \
             entity record and this test should be replaced by one that names it."
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Internal consistency of the committed tables
// ---------------------------------------------------------------------------------------------

#[test]
fn every_per_operator_access_appears_in_its_structures_field_map() {
    let fields: BTreeSet<(String, String, String)> = rows("structure-fields.tsv")
        .into_iter()
        .map(|row| {
            (
                row["base"].clone(),
                row["base_kind"].clone(),
                row["offset"].clone(),
            )
        })
        .collect();
    for row in rows("operator-field-access.tsv") {
        assert!(
            fields.contains(&(
                row["base"].clone(),
                row["base_kind"].clone(),
                row["offset"].clone()
            )),
            "{} touches {}+{} and the field map does not carry it",
            row["operator"],
            row["base"],
            row["offset"]
        );
    }
}

#[test]
fn a_structures_extent_covers_its_widest_field() {
    for structure in rows("structures.tsv") {
        let extent = u32::from_str_radix(structure["observed_extent"].trim_start_matches("0x"), 16)
            .expect("hexadecimal");
        for (offset, field) in fields_of(&structure["base"], &structure["base_kind"]) {
            let width: u32 = field["widths"]
                .split('|')
                .map(|width| width.parse::<u32>().expect("a width"))
                .max()
                .expect("at least one width");
            assert!(
                offset + width <= extent,
                "{} has a field at +{offset:#x} of {width} bytes past its extent {extent:#x}",
                structure["base"]
            );
        }
    }
}

/// A field written by nobody must name no writers, and one written by somebody must name at least
/// one. The failure this guards is a table whose `access` column and whose name list disagree,
/// which is how a reader looking for "who writes the turn counter" gets a wrong answer.
#[test]
fn the_access_column_agrees_with_the_writer_list() {
    for row in rows("structure-fields.tsv") {
        let writers: usize = row["writers"].parse().expect("a count");
        let named = row["writer_names"] != "-";
        assert_eq!(
            writers > 0,
            named || writers > 24,
            "{}+{} says {writers} writers and lists {:?}",
            row["base"],
            row["offset"],
            row["writer_names"]
        );
        assert_eq!(
            writers > 0,
            row["access"].contains("write"),
            "{}+{} access column disagrees with its writer count",
            row["base"],
            row["offset"]
        );
    }
}

/// The published depth is 2, and the tables must not carry anything from deeper.
#[test]
fn no_committed_row_comes_from_deeper_than_the_published_join() {
    for row in rows("operator-field-access.tsv") {
        let depth: usize = row["depth"].parse().expect("a depth");
        assert!(depth <= 2, "row at depth {depth}: {row:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// Against the binary. `cargo test --release -- --ignored` with LOM_EXE set.
// ---------------------------------------------------------------------------------------------

fn executable() -> Vec<u8> {
    let path = std::env::var_os("LOM_EXE").expect(
        "set LOM_EXE to a copy of lomse.exe. This test is #[ignore]d precisely so that a machine \
         without the binary reports it as ignored instead of as passed.",
    );
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("LOM_EXE is set to {path:?} but cannot be read: {error}"))
}

fn regenerate(bytes: &[u8]) -> (lom_asset_viewer::operator_bodies::Analysis, String) {
    use lom_asset_viewer::native_table::{self, PeImage};
    let image = PeImage::parse(bytes).expect("lomse.exe parses as a PE image");
    let runs = native_table::extract(bytes).expect("the operator table is found");
    let mut seen = BTreeSet::new();
    let mut operators = Vec::new();
    for entry in runs.iter().flat_map(|run| run.entries.iter()) {
        let name = entry.name.to_ascii_lowercase();
        if seen.insert(name.clone()) {
            operators.push((name, entry.entry_point));
        }
    }
    let analysis =
        lom_asset_viewer::operator_bodies::analyse(&image, &operators).expect("the analysis runs");
    let table = lom_asset_viewer::operator_bodies::operator_table(&analysis.reports);
    (analysis, table)
}

/// The control for the whole extension: the state model added columns to the walk, and the table
/// the previous work published must come out of it unchanged. A single differing byte means this
/// change moved a number the documentation quotes.
#[test]
#[ignore = "needs LOM_EXE"]
fn the_previous_operator_table_regenerates_byte_for_byte() {
    let (_, table) = regenerate(&executable());
    let committed =
        std::fs::read_to_string(repository_root().join("reports/natives/operator-bodies.tsv"))
            .expect("the previous table is committed");
    assert_eq!(
        table, committed,
        "the state-model extension changed the published operator table"
    );
}

#[test]
#[ignore = "needs LOM_EXE"]
fn the_committed_state_tables_are_not_stale() {
    use lom_asset_viewer::engine_state;
    let (analysis, _) = regenerate(&executable());
    let model = engine_state::build(&analysis, &analysis.bodies, engine_state::DEFAULT_DEPTH);
    let structures = engine_state::structures(&model);
    for (name, generated) in [
        (
            "operator-field-access.tsv",
            engine_state::access_table(&model),
        ),
        ("structures.tsv", engine_state::structure_table(&structures)),
        (
            "structure-fields.tsv",
            engine_state::field_table(&structures),
        ),
    ] {
        let committed =
            std::fs::read_to_string(repository_root().join("reports/natives/state").join(name))
                .unwrap_or_else(|error| panic!("{name} is committed: {error}"));
        assert_eq!(generated, committed, "{name} is stale");
    }
}

/// The saturation argument, checked rather than asserted: past depth 3 the join stops finding new
/// objects. If a future change makes it keep growing, the chosen depth is no longer defensible.
#[test]
#[ignore = "needs LOM_EXE"]
fn the_join_saturates_before_the_depth_cap() {
    use lom_asset_viewer::engine_state;
    let (analysis, _) = regenerate(&executable());
    let curve = engine_state::reach_curve(&analysis, &analysis.bodies, &[2, 3, 4, 5]);
    assert_eq!(
        curve[0].distinct_bases, curve[3].distinct_bases,
        "the set of objects reached is still growing at depth 5: {curve:?}"
    );
    assert!(
        curve[0].operators_with_fields * 100 >= curve[3].operators_with_fields * 99,
        "the set of operators with fields is still growing at depth 5: {curve:?}"
    );
}

/// The save-format cross-check, recomputed from the binary rather than read off a committed file.
///
/// The offline copies of these two assertions read `reports/natives/state/`, which this repository
/// generated — so they cannot fail on the *rule* being wrong, only on the file being edited. This
/// one recomputes the model and is therefore the version that a change to the join can break.
#[test]
#[ignore = "needs LOM_EXE"]
fn the_documented_save_offsets_come_back_out_of_the_binary() {
    use lom_asset_viewer::engine_state;
    use lom_asset_viewer::operator_bodies::BaseKind;
    let (analysis, _) = regenerate(&executable());
    let model = engine_state::build(&analysis, &analysis.bodies, engine_state::DEFAULT_DEPTH);
    let structures = engine_state::structures(&model);
    let game = structures
        .iter()
        .find(|structure| structure.base == 0x005a_a12c && structure.kind == BaseKind::Static)
        .expect("the object the save loader uses as `this` is recovered");

    let block = game
        .fields
        .get(&0x520)
        .expect("docs/save-format.md: the LS_MULT setup block is copied from [gameobj+0x520]");
    assert!(
        block.widths.contains(&0),
        "the block is reached by `lea` and never dereferenced there: {block:?}"
    );

    let name = game
        .fields
        .get(&0x50ac)
        .expect("docs/save-format.md: a player name is strcpy'd from [player+0x50ac]");
    assert!(
        name.indexed,
        "the player pointer is formed with an index register, so the offset is within element \
         zero and must not be published as a scalar: {name:?}"
    );
}

/// The `net` convergence, recomputed. `0x005d1e84` was named the DirectPlay session object by the
/// vtable work, which shares no mechanism with field access.
#[test]
#[ignore = "needs LOM_EXE"]
fn the_network_convergence_holds_when_recomputed() {
    use lom_asset_viewer::engine_state;
    let (analysis, _) = regenerate(&executable());
    let model = engine_state::build(&analysis, &analysis.bodies, engine_state::DEFAULT_DEPTH);
    let structures = engine_state::structures(&model);
    let best = structures
        .iter()
        .max_by_key(|structure| {
            structure
                .operators
                .iter()
                .filter(|name| name.contains("net"))
                .count()
        })
        .expect("at least one structure");
    assert_eq!(
        best.base, 0x005d_1e84,
        "the `net` operators no longer converge on the session object"
    );
}

/// A structure's field count must not be a function of how many *other* objects its methods touch.
///
/// The failure mode is reachability masquerading as attribution: a method on the world object that
/// also poke the audio mixer must not put the mixer's offsets into the world's field map. Measured
/// as a ceiling on the largest structure, because a join that merged objects grows it without bound.
#[test]
#[ignore = "needs LOM_EXE"]
fn the_largest_structure_does_not_swallow_the_others() {
    use lom_asset_viewer::engine_state;
    let (analysis, _) = regenerate(&executable());
    let model = engine_state::build(&analysis, &analysis.bodies, engine_state::DEFAULT_DEPTH);
    let structures = engine_state::structures(&model);
    let total: usize = structures.iter().map(|structure| structure.fields.len()).sum();
    let largest = structures
        .iter()
        .map(|structure| structure.fields.len())
        .max()
        .expect("at least one structure");
    assert!(
        largest * 2 < total,
        "one structure holds {largest} of {total} recovered fields, which is what merging two \
         objects looks like"
    );
}

/// Objects must not collect each other's fields.
///
/// The DirectPlay session at `0x005d1e84` and the object the save loader uses as `this` at
/// `0x005aa12c` share callees — both go through the engine's string and allocation helpers. If a
/// callee's accesses to *other* objects were folded into the caller's base, or if a pointer loaded
/// out of one object kept the container's identity, the session would acquire the game object's
/// documented `+0x520` setup block. It must not.
#[test]
#[ignore = "needs LOM_EXE"]
fn two_unrelated_objects_do_not_acquire_each_others_fields() {
    use lom_asset_viewer::engine_state;
    let (analysis, _) = regenerate(&executable());
    let model = engine_state::build(&analysis, &analysis.bodies, engine_state::DEFAULT_DEPTH);
    let structures = engine_state::structures(&model);
    let session = structures
        .iter()
        .find(|structure| structure.base == 0x005d_1e84)
        .expect("the session object is recovered");
    assert!(
        !session.fields.contains_key(&0x520),
        "the session object has acquired the game object's setup-block offset, which is what \
         attributing a callee's other objects to its caller looks like"
    );
    let game = structures
        .iter()
        .find(|structure| structure.base == 0x005a_a12c)
        .expect("the game object is recovered");
    let shared = session
        .fields
        .keys()
        .filter(|offset| game.fields.contains_key(offset))
        .count();
    assert!(
        shared * 2 < session.fields.len(),
        "{shared} of the session's {} offsets are also the game object's",
        session.fields.len()
    );
}

/// The two-instrument comparison must look at the whole object it is comparing.
///
/// Stated as a property rather than a threshold: no offset the join found may lie past the range
/// the comparison searches, or the `absolute_only` and `both` counts are describing a prefix of the
/// structure and silently calling it the structure. The rule this replaced — "the range ends at the
/// next materialised base" — fails this, because C++ code takes the address of members and
/// `0x005aa1dc` is a base 0xb0 bytes inside an object with fields out to `+0x6ccc`.
#[test]
#[ignore = "needs LOM_EXE"]
fn the_agreement_range_covers_every_field_the_join_found() {
    use lom_asset_viewer::engine_state;
    use lom_asset_viewer::operator_bodies::BaseKind;
    let (analysis, _) = regenerate(&executable());
    let model = engine_state::build(&analysis, &analysis.bodies, engine_state::DEFAULT_DEPTH);
    let structures = engine_state::structures(&model);
    let rows = engine_state::agreement(&model, &structures);
    for row in &rows {
        let structure = structures
            .iter()
            .find(|structure| structure.base == row.base && structure.kind == BaseKind::Static)
            .expect("every agreement row has a structure");
        let span = row.territory_end - row.base;
        for offset in structure.fields.keys() {
            assert!(
                *offset < span || span == 0,
                "{:#010x} is compared over {span:#x} bytes and the join found a field at \
                 +{offset:#x}",
                row.base
            );
        }
    }
}

/// Three instruments on the same two fields: the map's width and height.
///
/// * The **absolute** instrument, in the previously committed table: `mapw` names `0x005ae9b4` and
///   no other engine address; `maph` names `0x005ae9b8` and no other.
/// * The **join**: the object at `0x005ae958` has fields at `+0x5c` and `+0x60`, and
///   `0x005ae958 + 0x5c == 0x005ae9b4`.
/// * **`docs/save-format.md`**, written from the save file and its writer: `LS_MAP_` begins
///   `u32 width; u32 height` — two adjacent dwords in that order.
///
/// The three share no mechanism. This is the single strongest agreement in this analysis and it is
/// pinned so that losing it fails rather than being noticed later. It runs offline because both
/// tables are committed.
#[test]
fn the_map_width_and_height_agree_across_three_instruments() {
    const MAP_OBJECT: u32 = 0x005a_e958;
    let bodies = std::fs::read_to_string(repository_root().join("reports/natives/operator-bodies.tsv"))
        .expect("the operator table is committed");
    let mut header = bodies.lines();
    let columns: Vec<&str> = header.next().expect("a header").split('\t').collect();
    let address_column = columns
        .iter()
        .position(|column| *column == "global_addresses")
        .expect("the table names the addresses each body touches");
    let addresses = |operator: &str| -> Vec<String> {
        bodies
            .lines()
            .find(|line| line.starts_with(&format!("{operator}\t")))
            .unwrap_or_else(|| panic!("{operator} is in the table"))
            .split('\t')
            .nth(address_column)
            .expect("the column exists")
            .split(',')
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(addresses("mapw"), vec!["0x5ae9b4".to_owned()]);
    assert_eq!(addresses("maph"), vec!["0x5ae9b8".to_owned()]);

    let fields = fields_of("0x005ae958", "static");
    for (offset, address) in [(0x5c_u32, 0x005a_e9b4_u32), (0x60, 0x005a_e9b8)] {
        assert_eq!(MAP_OBJECT + offset, address);
        let field = fields
            .get(&offset)
            .unwrap_or_else(|| panic!("the join found +{offset:#x} of the map object"));
        assert!(
            field["widths"].split('|').any(|width| width == "4"),
            "a map dimension is a dword in both instruments: {field:?}"
        );
    }
}
