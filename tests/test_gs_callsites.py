"""Checks on the call-site scanner, against synthetic scripts.

The real corpus is proprietary and is not in this repository, so every fixture here is written by
hand. Each one encodes a mistake that has actually been made while reading operand orders out of
the shipped scripts.
"""

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

import gs_callsites  # noqa: E402


class CorpusFixture:
    """A throwaway directory of `.gs` files."""

    def __init__(self, files: dict[str, str]) -> None:
        self._directory = tempfile.TemporaryDirectory()
        self.path = Path(self._directory.name)
        for name, source in files.items():
            target = self.path / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(source)

    def close(self) -> None:
        self._directory.cleanup()


class DefinitionsTest(unittest.TestCase):
    def test_a_name_bound_to_a_procedure_is_not_a_value(self) -> None:
        """This distinction is the whole point: `great_temple` looks like a constant and is not."""
        definitions = gs_callsites.Definitions()
        source = "/great_temple{exch get}def /orchard 4 def"
        definitions.add_file([token for token, _ in gs_callsites.tokens_with_offsets(source)])
        self.assertEqual(definitions.classify("great_temple"), "procedure")
        self.assertEqual(definitions.classify("orchard"), "value")

    def test_literals_are_classified_without_the_corpus(self) -> None:
        definitions = gs_callsites.Definitions()
        self.assertEqual(definitions.classify("128"), "number")
        self.assertEqual(definitions.classify("-1.5"), "number")
        self.assertEqual(definitions.classify('"map/zz512.scn"'), "string")
        self.assertEqual(definitions.classify("/tower3"), "literal-name")
        self.assertEqual(definitions.classify("nobody_defines_this"), "unknown")


class ScanTest(unittest.TestCase):
    def setUp(self) -> None:
        self.fixture = CorpusFixture(
            {
                "a.gs": (
                    "/keep_ttype{exch get}def\n"
                    "/place{cx cy f terrainsprites begin keep_ttype end addterrainsprite}def\n"
                ),
                "b.gs": "s_x s_y terrainsprites /tower3 get addterrainsprite\n",
                "c.txt": "x y esp03 addterrainsprite\n",
                "d.png": "not script content\n",
            }
        )
        self.addCleanup(self.fixture.close)

    def test_finds_call_sites_across_extensions_that_hold_script(self) -> None:
        sites, _ = gs_callsites.scan(self.fixture.path, "addterrainsprite")
        self.assertEqual(len(sites), 3, "the .txt member holds script too; the .png does not")

    def test_a_definition_of_the_operator_is_not_a_call_of_it(self) -> None:
        fixture = CorpusFixture({"e.gs": "/myop{1 2 add}def\n3 4 myop\n"})
        self.addCleanup(fixture.close)
        sites, _ = gs_callsites.scan(fixture.path, "myop")
        self.assertEqual(len(sites), 1)
        self.assertEqual(sites[0].window, ["3", "4"])

    def test_the_window_stops_at_a_procedure_boundary(self) -> None:
        """Tokens on the far side of a `{` are not operands of this call."""
        fixture = CorpusFixture({"f.gs": "99 98 {1 2 myop}\n"})
        self.addCleanup(fixture.close)
        sites, _ = gs_callsites.scan(fixture.path, "myop")
        self.assertEqual(sites[0].window, ["1", "2"])

    def test_a_procedure_literal_is_one_operand_not_a_boundary(self) -> None:
        """`currentplayer{...}enumplayerarmies` takes two operands.

        Treating `}` as the end of the window reported a blank pattern, which reads as "takes no
        operands" for an operator two existing probes are built on. Walking back to the matching
        `{` keeps both operands visible, and the nested `{pop}` inside must not end the walk early.
        """
        fixture = CorpusFixture(
            {"i.gs": "currentplayer{zseen not{pop}ifelse}enumplayerarmies\n"}
        )
        self.addCleanup(fixture.close)
        sites, _ = gs_callsites.scan(fixture.path, "enumplayerarmies")
        self.assertEqual(sites[0].window, ["currentplayer", "{...}"])
        self.assertEqual(sites[0].pattern, "currentplayer {...}")

    def test_a_truncated_window_says_so(self) -> None:
        """A window cut by the token limit must not read like a complete one.

        The `make_custom_random_map` operand order rests on a window sitting exactly at the limit;
        silently dropping `map_width` would have left the order looking unsupported.
        """
        fixture = CorpusFixture({"j.gs": "a b c d e f g h myop\n"})
        self.addCleanup(fixture.close)
        sites, definitions = gs_callsites.scan(fixture.path, "myop", window=3)
        self.assertTrue(sites[0].truncated)
        self.assertTrue(sites[0].pattern.startswith("..."))
        self.assertIn("--window", gs_callsites.report(sites, definitions, "myop"))

    def test_a_window_ending_at_a_real_boundary_is_not_truncated(self) -> None:
        fixture = CorpusFixture({"k.gs": "/x 1 def 4 5 myop\n"})
        self.addCleanup(fixture.close)
        sites, _ = gs_callsites.scan(fixture.path, "myop", window=6)
        self.assertFalse(sites[0].truncated)
        self.assertEqual(sites[0].window, ["4", "5"])

    def test_names_absent_from_the_corpus_are_surfaced_as_engine_operators(self) -> None:
        """`terrainsprites /tower3 get` is three tokens and one operand, because `get` is an operator.

        Only flagging corpus-defined procedures left that case silent, which is the same
        miscount the tool exists to prevent.
        """
        fixture = CorpusFixture({"l.gs": "s_x s_y terrainsprites /tower3 get myop\n"})
        self.addCleanup(fixture.close)
        sites, definitions = gs_callsites.scan(fixture.path, "myop")
        text = gs_callsites.report(sites, definitions, "myop")
        self.assertIn("engine operators", text)
        self.assertIn("get", text.split("engine operators", 1)[1])

    def test_procedures_in_the_window_are_flagged(self) -> None:
        """The `addterrainsprite` trap: four tokens, three operands."""
        sites, definitions = gs_callsites.scan(self.fixture.path, "addterrainsprite")
        text = gs_callsites.report(sites, definitions, "addterrainsprite")
        self.assertIn("PROCEDURES", text)
        self.assertIn("keep_ttype", text.split("PROCEDURES", 1)[1])

    def test_a_window_of_plain_values_is_not_flagged(self) -> None:
        """Asserted on the classification, not on the absence of a word in the report.

        Checking only that "PROCEDURES" is missing would pass just as well if the names had been
        misclassified as `unknown` and landed in the other warning block instead.
        """
        fixture = CorpusFixture({"g.gs": "/sx 1 def /sy 2 def\nsx sy 7 myop\n"})
        self.addCleanup(fixture.close)
        sites, definitions = gs_callsites.scan(fixture.path, "myop")
        self.assertEqual(definitions.classify("sx"), "value")
        self.assertEqual(definitions.classify("sy"), "value")
        report = gs_callsites.report(sites, definitions, "myop")
        self.assertNotIn("PROCEDURES", report)
        self.assertNotIn("engine operators", report)

    def test_a_name_is_not_a_definition_just_because_a_brace_follows_it(self) -> None:
        """`/sprite_type get exec` looks up a key; a dictionary literal is full of `key{proc}`.

        Recording those as definitions makes the classifier wrong in the direction that matters:
        a name wrongly known as a `value` stops being reported as an unresolved engine operator.
        """
        fixture = CorpusFixture(
            {
                "m.gs": (
                    "/real_proc{1 2 add}bind def\n"
                    "/real_value 4 def\n"
                    "currentdungeon /sprite_type get exec\n"
                    "/table << -1{neutral}LIFE{life}>> replace bind def\n"
                )
            }
        )
        self.addCleanup(fixture.close)
        _, definitions = gs_callsites.scan(fixture.path, "anything")
        self.assertEqual(definitions.classify("real_proc"), "procedure")
        self.assertEqual(definitions.classify("real_value"), "value")
        self.assertEqual(definitions.classify("sprite_type"), "unknown")

    def test_a_literal_name_is_not_a_call_but_does_not_suppress_one(self) -> None:
        """`/myop myop` pushes a literal name and then calls: one call site, not zero and not two.

        An earlier guard skipped any call preceded by `/name`, meaning to skip definitions. A
        definition is `/myop{...}def`, which never produces that adjacency, so the guard only ever
        threw away real calls.
        """
        fixture = CorpusFixture({"n.gs": "/myop myop\n"})
        self.addCleanup(fixture.close)
        sites, _ = gs_callsites.scan(fixture.path, "myop")
        self.assertEqual(len(sites), 1)
        self.assertEqual(sites[0].window, ["/myop"])

    def test_position_points_at_the_right_occurrence_on_a_long_line(self) -> None:
        """Shipped lines run to thousands of characters and repeat names many times.

        Re-deriving a position by searching for the token text lands on the first occurrence, which
        is the wrong one. The offset is carried through instead.
        """
        filler = "pop " * 400
        # Deliberately DIFFERENT columns: equal ones would pass even if the column never reset at
        # a newline, or were computed from the wrong occurrence.
        fixture = CorpusFixture({"h.gs": f"1 2 myop {filler}\n      5 6 myop\n"})
        self.addCleanup(fixture.close)
        sites, _ = gs_callsites.scan(fixture.path, "myop")
        self.assertEqual([site.line for site in sites], [1, 2])
        self.assertEqual(sites[0].column, 5)
        self.assertEqual(sites[1].column, 11)
        self.assertEqual([site.where.split(":", 1)[1] for site in sites], ["1:5", "2:11"])

    def test_missing_operator_reports_rather_than_raising(self) -> None:
        sites, definitions = gs_callsites.scan(self.fixture.path, "not_an_operator")
        self.assertEqual(sites, [])
        self.assertIn("no call sites", gs_callsites.report(sites, definitions, "not_an_operator"))
        with contextlib.redirect_stdout(io.StringIO()) as captured:
            exit_code = gs_callsites.main([str(self.fixture.path), "not_an_operator"])
        self.assertEqual(exit_code, 1, "a search that found nothing must not report success")
        self.assertIn("no call sites", captured.getvalue())


if __name__ == "__main__":
    unittest.main()
