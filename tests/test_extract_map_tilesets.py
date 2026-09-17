"""Tests for the gamescript map/tileset binding extractor.

The fixtures here are **written to be unlike the real corpus in the ways that matter**, because a
fixture shaped like the corpus cannot fail on what the corpus hides. So: a different tileset in
every position rather than a symmetric row, uppercase paths, reversed key order, a member with two
maps, a procedure with several branches, and an encounter with a map and no tileset. No shipped
gamescript is committed.
"""

import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "tools"))

import extract_map_tilesets as extractor


class MatchingBraceTest(unittest.TestCase):
    def test_finds_the_matching_brace_through_nesting(self):
        text = "{a{b}c}tail"
        self.assertEqual(extractor.matching_brace(text, 0), 7)

    def test_a_brace_inside_a_string_literal_does_not_nest(self):
        # The real `/tileset{...}` procedures contain quoted paths; a brace inside one of those
        # must not be counted. Without the string skip this returns the wrong index and the
        # procedure body is truncated mid-way.
        text = '{ "til/a{b.til" }after'
        self.assertEqual(extractor.matching_brace(text, 0), 17)
        self.assertEqual(text[:17], '{ "til/a{b.til" }')

    def test_an_unterminated_procedure_returns_the_end_rather_than_raising(self):
        text = "{never closed"
        self.assertEqual(extractor.matching_brace(text, 0), len(text))

    def test_refuses_to_start_off_a_brace(self):
        with self.assertRaises(ValueError):
            extractor.matching_brace("abc", 0)


class BasenameTest(unittest.TestCase):
    def test_lowercases_and_strips_either_separator(self):
        self.assertEqual(extractor.basename("til/aibldg01.til"), "aibldg01.til")
        self.assertEqual(extractor.basename("til\\LIBLDG01.til"), "libldg01.til")
        self.assertEqual(extractor.basename("MAP/AICAVE.SMP"), "aicave.smp")
        self.assertEqual(extractor.basename("bare.smp"), "bare.smp")


class DefinitionsTest(unittest.TestCase):
    def test_reads_a_string_literal_definition(self):
        found = extractor.definitions('/tileset"til/ruins01.til"def', "tileset")
        self.assertEqual([values for _, values in found], [["ruins01.til"]])

    def test_reads_every_branch_of_a_procedure_definition(self):
        # The real `aimult.gs` shape. A reader that only handled string literals would return
        # nothing here, which is how 14 procedure-form bindings would go missing.
        source = (
            "/tileset{dungeon_id getdungeonstrength 3 le"
            '{"til/one.til"}{dungeon_id getdungeonstrength 5 le'
            '{"til/two.til"}{"til/three.til"}ifelse}ifelse}bind def'
        )
        found = extractor.definitions(source, "tileset")
        self.assertEqual(
            [values for _, values in found], [["one.til", "two.til", "three.til"]]
        )

    def test_a_key_that_is_not_defined_yields_nothing(self):
        self.assertEqual(extractor.definitions('/mapfile"map/a.smp"def', "tileset"), [])


class BindingsInMemberTest(unittest.TestCase):
    def test_pairs_a_map_with_its_adjacent_tileset(self):
        source = '/mapfile"map/aicave.smp"def /tileset"til/aibldg01.til"def'
        self.assertEqual(
            extractor.bindings_in_member(source), [("aicave.smp", "aibldg01.til")]
        )

    def test_pairs_when_the_keys_are_written_in_the_other_order(self):
        source = '/tileset"til/aibldg01.til"def /mapfile"map/aicave.smp"def'
        self.assertEqual(
            extractor.bindings_in_member(source), [("aicave.smp", "aibldg01.til")]
        )

    def test_uppercase_paths_are_folded(self):
        # `/tileset"til/LIBLDG01.til"` really occurs, and `map/` is split .smp/.SMP.
        source = '/mapfile"MAP/LICAVE.SMP"def /tileset"til/LIBLDG01.til"def'
        self.assertEqual(
            extractor.bindings_in_member(source), [("licave.smp", "libldg01.til")]
        )

    def test_each_map_takes_the_nearest_tileset_not_the_first(self):
        # A member defining two encounters. A reader that paired every map with the member's first
        # tileset would give `second.smp` the wrong one -- and because both are plausible shipped
        # names, the result would look right. Every value here is distinct so that cannot pass.
        source = (
            '/mapfile"map/first.smp"def /tileset"til/alpha.til"def '
            '/mapfile"map/second.smp"def /tileset"til/beta.til"def'
        )
        self.assertEqual(
            sorted(extractor.bindings_in_member(source)),
            [("first.smp", "alpha.til"), ("second.smp", "beta.til")],
        )

    def test_a_map_with_no_tileset_in_the_member_contributes_nothing(self):
        # The "outside combat encounter" case: `mapfile` and `tileset` both undefined means the
        # engine generates the map, and a member naming only one of them has no binding to record.
        # Falling back to `combattileset` here is precisely the retracted conclusion.
        self.assertEqual(extractor.bindings_in_member('/mapfile"map/a.smp"def'), [])
        self.assertEqual(extractor.bindings_in_member('/tileset"til/a.til"def'), [])
        self.assertEqual(extractor.bindings_in_member("no keys at all"), [])

    def test_a_procedure_tileset_yields_every_candidate_for_the_map(self):
        source = (
            '/mapfile"map/chcave.smp"def '
            '/tileset{"til/chbldg01.til" {"til/cavelava.til" exit}forall}def'
        )
        self.assertEqual(
            sorted(extractor.bindings_in_member(source)),
            [("chcave.smp", "cavelava.til"), ("chcave.smp", "chbldg01.til")],
        )

    def test_a_tileset_that_is_not_a_til_is_ignored(self):
        source = '/mapfile"map/a.smp"def /tileset"lbm/notatileset.lbm"def'
        self.assertEqual(extractor.bindings_in_member(source), [])


class StripCommentsTest(unittest.TestCase):
    """Gamescript members use bare ``CR`` line endings, which is the whole reason this is subtle."""

    def test_a_comment_ends_at_a_bare_cr(self):
        # Split only on LF and the entire rest of the "line" looks commented, which is how
        # bindings got read out of disabled code.
        text = '; /tileset"til/dead.til"def\r/tileset"til/live.til"def'
        stripped = extractor.strip_comments(text)
        self.assertNotIn("dead.til", stripped)
        self.assertIn("live.til", stripped)

    def test_handles_lf_and_crlf_too(self):
        for ending in ("\n", "\r\n", "\r"):
            text = "; gone%slive" % ending
            self.assertNotIn("gone", extractor.strip_comments(text))
            self.assertIn("live", extractor.strip_comments(text))

    def test_a_semicolon_inside_a_string_is_not_a_comment(self):
        text = '/tileset"til/a;b.til"def live'
        self.assertEqual(extractor.strip_comments(text), text)

    def test_line_structure_is_preserved_so_positions_stay_meaningful(self):
        # Positional pairing depends on offsets, so stripping must not shift later text onto an
        # earlier line or the nearest-following-tileset search changes answer.
        text = 'a; comment\rb'
        self.assertEqual(extractor.strip_comments(text), "a\rb")

    def test_a_commented_binding_is_not_extracted(self):
        # The real `wilderness_land.gs` shape: the pair is commented out precisely because that
        # member is the outside-combat encounter.
        text = '; /mapfile"map/chcave.smp"def ; /tileset"til/cavewatr.til"def'
        self.assertEqual(
            extractor.bindings_in_member(extractor.strip_comments(text)), []
        )


class ArrayFormTest(unittest.TestCase):
    """The plural selector form: a ``/tilesets`` procedure over a separately-named ``/tiles[...]``."""

    SOURCE = (
        '/mapfiles{...}/dummy currentdict replace '
        '/maps["map/waming.smp" "map/chming.smp"]replace bind def\r'
        '/tilesets{...}/dummy currentdict replace '
        '/tiles["til/cavewatr.til" "til/cavecrys.til"]replace bind def'
    )

    def test_reads_an_array_definition(self):
        self.assertEqual(
            extractor.array_definitions(self.SOURCE, "maps"),
            [["waming.smp", "chming.smp"]],
        )
        self.assertEqual(
            extractor.array_definitions(self.SOURCE, "tiles"),
            [["cavewatr.til", "cavecrys.til"]],
        )

    def test_the_singular_key_does_not_match_the_plural_array(self):
        # `/tileset` must not match `/tilesets`, or the singular reader would swallow the plural
        # form -- and `/tilesets{` is exactly what an earlier `/tileset\s*\{` search missed.
        self.assertEqual(extractor.array_definitions(self.SOURCE, "tileset"), [])
        self.assertEqual(extractor.definitions(self.SOURCE, "tileset"), [])
        self.assertEqual(extractor.definitions(self.SOURCE, "mapfile"), [])

    def test_the_coarse_reading_is_the_full_cross_product(self):
        # Deliberately 2x2 with four distinct values, so a positional zip and a cross product give
        # different answers and this test can tell them apart.
        self.assertEqual(
            sorted(extractor.array_bindings_in_member(self.SOURCE)),
            [
                ("chming.smp", "cavecrys.til"),
                ("chming.smp", "cavewatr.til"),
                ("waming.smp", "cavecrys.til"),
                ("waming.smp", "cavewatr.til"),
            ],
        )

    def test_arrays_of_different_lengths_still_cross(self):
        # 31 of 40 shipped members have equal-length arrays and 9 do not, so an implementation that
        # zipped would silently drop the odd ones.
        source = '/maps["map/a.smp" "map/b.smp"]def /tiles["til/x.til"]def'
        self.assertEqual(
            sorted(extractor.array_bindings_in_member(source)),
            [("a.smp", "x.til"), ("b.smp", "x.til")],
        )

    def test_a_member_with_only_one_array_yields_nothing(self):
        self.assertEqual(
            extractor.array_bindings_in_member('/maps["map/a.smp"]def'), []
        )
        self.assertEqual(
            extractor.array_bindings_in_member('/tiles["til/x.til"]def'), []
        )

    def test_an_unterminated_array_is_skipped_rather_than_raising(self):
        self.assertEqual(extractor.array_definitions('/maps["map/a.smp"', "maps"), [])


class ExtractTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.mkdtemp(prefix="lom-gs-")

    def tearDown(self):
        for name in os.listdir(self.directory):
            os.remove(os.path.join(self.directory, name))
        os.rmdir(self.directory)

    def write(self, name, text):
        with open(os.path.join(self.directory, name), "w", encoding="latin-1") as handle:
            handle.write(text)

    def test_merges_candidates_for_one_map_across_members(self):
        # The real ambiguity: two encounters, one map, two tilesets. The result must be both,
        # sorted -- not the first seen, and not the last.
        self.write("one.gs", '/mapfile"map/chcave.smp"def /tileset"til/ruins01.til"def')
        self.write("two.gs", '/mapfile"map/chcave.smp"def /tileset"til/cavewatr.til"def')
        self.write("three.gs", '/mapfile"map/aicave.smp"def /tileset"til/aibldg01.til"def')
        declared, coarse = extractor.extract(self.directory)
        self.assertEqual(
            declared,
            {
                "aicave.smp": ["aibldg01.til"],
                "chcave.smp": ["cavewatr.til", "ruins01.til"],
            },
        )
        self.assertEqual(coarse, {})

    def test_array_candidates_are_separate_and_exclude_the_declared_one(self):
        # The two tables must be disjoint, or the "extra" framing is a lie and they double-count.
        self.write(
            "waming.gs",
            '/mapfile"map/waming.smp"def /tileset"til/cavewatr.til"def\r'
            '/maps["map/waming.smp"]def /tiles["til/cavewatr.til" "til/cavelava.til"]def',
        )
        declared, coarse = extractor.extract(self.directory)
        self.assertEqual(declared, {"waming.smp": ["cavewatr.til"]})
        self.assertEqual(coarse, {"waming.smp": ["cavelava.til"]})

    def test_a_map_whose_array_candidates_are_all_declared_is_absent_from_the_coarse_table(self):
        self.write(
            "a.gs",
            '/mapfile"map/a.smp"def /tileset"til/x.til"def\r'
            '/maps["map/a.smp"]def /tiles["til/x.til"]def',
        )
        _, coarse = extractor.extract(self.directory)
        self.assertEqual(coarse, {})

    def test_comments_are_honoured_by_extract(self):
        self.write("live.gs", '/mapfile"map/a.smp"def /tileset"til/x.til"def')
        self.write("dead.gs", '; /mapfile"map/b.smp"def /tileset"til/y.til"def')
        declared, _ = extractor.extract(self.directory)
        self.assertEqual(declared, {"a.smp": ["x.til"]})

    def test_a_directory_with_no_bindings_extracts_nothing(self):
        self.write("empty.gs", "; just a comment\n")
        self.assertEqual(extractor.extract(self.directory), ({}, {}))

    def test_the_cli_fails_rather_than_emitting_an_empty_table(self):
        # An empty extraction is a failed extraction. Exiting 0 here is how a regeneration could
        # silently replace the committed table with nothing -- the same vacuous-pass failure the
        # Rust instrument was fixed for.
        self.write("empty.gs", "; nothing here\n")
        self.assertEqual(extractor.main([self.directory]), 1)
        self.assertEqual(extractor.main([self.directory, "--rust"]), 1)

    def test_the_cli_reports_a_missing_directory(self):
        self.assertEqual(extractor.main([os.path.join(self.directory, "absent")]), 2)


class EmitRustTest(unittest.TestCase):
    def test_emits_both_sorted_tables_the_rust_binary_search_can_use(self):
        table = {
            "zzz.smp": ["beta.til"],
            "aaa.smp": ["alpha.til", "gamma.til"],
        }
        emitted = extractor.emit_rust(table, {"aaa.smp": ["delta.til"]})
        self.assertEqual(
            emitted,
            "pub static COMBAT_TILESET_BINDINGS: &[(&str, &[&str])] = &[\n"
            '    ("aaa.smp", &["alpha.til", "gamma.til"]),\n'
            '    ("zzz.smp", &["beta.til"]),\n'
            "];\n"
            "\n"
            "pub static COMBAT_TILESET_ARRAY_CANDIDATES: &[(&str, &[&str])] = &[\n"
            '    ("aaa.smp", &["delta.til"]),\n'
            "];\n",
        )
        # The second table must still be emitted when empty, or the Rust side stops compiling.
        self.assertIn("COMBAT_TILESET_ARRAY_CANDIDATES", extractor.emit_rust(table))
        # The Rust side binary-searches each table, so sortedness is load-bearing rather than
        # cosmetic -- and each table must be sorted independently, not the concatenation.
        for block in emitted.split("pub static ")[1:]:
            keys = [
                line.split('"')[1]
                for line in block.splitlines()
                if line.startswith('    ("')
            ]
            self.assertEqual(keys, sorted(keys), block)


if __name__ == "__main__":
    unittest.main()
