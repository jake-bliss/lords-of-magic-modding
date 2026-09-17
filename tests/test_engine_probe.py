"""Checks on the generated engine probe that are cheap here and expensive in the game.

An attended engine run costs a human a game start and a keypress, so a syntax error found by
a unit test is worth several of them. These assert the properties whose absence has actually
cost a run: unbalanced braces, a missing fire-once guard, and cleanup by location.
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

import engine_probe  # noqa: E402
import gs_syntax  # noqa: E402


class EngineProbeTest(unittest.TestCase):
    def setUp(self) -> None:
        self.body = engine_probe.probe_body()
        self.tokens = list(gs_syntax.tokens(self.body))

    def test_braces_and_brackets_balance(self) -> None:
        depth = 0
        lowest = 0
        for token in self.tokens:
            if token == "{":
                depth += 1
            elif token == "}":
                depth -= 1
                lowest = min(lowest, depth)
        self.assertEqual(depth, 0, "procedure braces do not balance")
        self.assertEqual(lowest, 0, "a closing brace precedes its opening brace")
        self.assertEqual(
            self.tokens.count("["), self.tokens.count("]"), "array brackets do not balance"
        )

    def test_fires_once_per_launch(self) -> None:
        # The key auto-repeats while held; the 2026-09-16 run fired nine times.
        self.assertIn("userdict /zdone known not", self.body)
        self.assertIn("/zdone true def", self.body)

    def test_cleanup_never_matches_on_location(self) -> None:
        # `terrainspriteat` returned a village the probe had not placed, and it was destroyed.
        self.assertNotIn("terrainspriteat", self.body)
        self.assertIn("getterrainspritetype", self.body)

    def test_ladder_covers_every_rung(self) -> None:
        for label, _expression in engine_probe.SPRITE_TYPES:
            self.assertIn(label, self.body)
        self.assertEqual(len(engine_probe.SPRITE_TYPES), len(engine_probe.SEED_OFFSETS))

    def test_seeds_are_distinct(self) -> None:
        self.assertEqual(
            len(set(engine_probe.SEED_OFFSETS)), len(engine_probe.SEED_OFFSETS)
        )

    def test_install_requires_the_end_marker(self) -> None:
        source = "; body\n\nend ; DO NOT ADD AFTER 'END'!"
        installed = engine_probe.install(source)
        self.assertIn("addhotkey", installed)
        self.assertTrue(installed.rstrip().endswith("end ; DO NOT ADD AFTER 'END'!"))
        with self.assertRaises(ValueError):
            engine_probe.install("; no marker here")

    def test_disable_intro_flips_exactly_one_guard(self) -> None:
        source = '\ntrue\n\t{\n\t75 75"smk/imptitle.smk"MODAL playvideo \n\t}if\n'
        self.assertIn("\nfalse\n", engine_probe.disable_intro(source))
        with self.assertRaises(ValueError):
            engine_probe.disable_intro("nothing to patch")


if __name__ == "__main__":
    unittest.main()
