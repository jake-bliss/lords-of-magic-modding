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

    def _depth_is_balanced(self, opener: str, closer: str, what: str) -> None:
        """Counting alone would accept `][`, so track the depth and never let it go negative."""
        depth = 0
        for token in self.tokens:
            if token == opener:
                depth += 1
            elif token == closer:
                depth -= 1
                self.assertGreaterEqual(depth, 0, f"{what}: {closer} precedes its {opener}")
        self.assertEqual(depth, 0, f"{what} do not balance")

    def test_braces_balance(self) -> None:
        self._depth_is_balanced("{", "}", "procedure braces")

    def test_brackets_balance(self) -> None:
        self._depth_is_balanced("[", "]", "array brackets")

    def test_fires_once_per_launch(self) -> None:
        # The key auto-repeats while held; the 2026-09-16 run fired nine times. The guard is only
        # worth anything if the whole body sits inside it, so check the ordering, not the presence.
        guard = self.body.index("userdict /zdone known not")
        flag = self.body.index("/zdone true def")
        first_effect = min(
            self.body.index("screencapture"),
            self.body.index("addterrainsprite"),
        )
        self.assertLess(guard, flag)
        self.assertLess(flag, first_effect, "the body can act before the fire-once flag is set")

    def test_cleanup_never_matches_on_location_alone(self) -> None:
        # `terrainspriteat` returned a village the probe had not placed, and it was destroyed.
        self.assertNotIn("terrainspriteat", self.body)

    def test_cleanup_never_destroys_a_shipped_type_by_type_alone(self) -> None:
        """The regression that mattered: rung 0 reuses the shipped orchard type.

        A type-only sweep over it would destroy every orchard the map generator placed, which is
        the same defect as the village, only wider. Every destroy must be guarded by a cell check
        unless its type id was minted by this keypress.
        """
        for index, (_label, expression) in enumerate(engine_probe.SPRITE_TYPES):
            if engine_probe.is_freshly_registered(index):
                continue
            self.assertNotIn("addterrainspritetype", expression)
            bare_sweep = (
                f"{{dup getterrainspritetype zt{index} eq"
                "{destroyterrainsprite}{pop}ifelse}enumterrainsprites"
            )
            self.assertNotIn(
                bare_sweep,
                self.body,
                f"rung {index} reuses a shipped type and must be cleaned up by cell as well",
            )
            self.assertIn(f"getterrainspritelocation zy{index} eq", self.body)

    def test_every_rung_is_registered_placed_and_cleaned_up(self) -> None:
        for index, (label, expression) in enumerate(engine_probe.SPRITE_TYPES):
            self.assertIn(label, self.body, f"rung {index} is not logged")
            self.assertIn(f"{expression} /zt{index} exch def", self.body)
            self.assertIn(f"zx{index} zy{index} zt{index} addterrainsprite", self.body)
            self.assertIn(f"getterrainspritetype zt{index} eq", self.body)
        self.assertEqual(len(engine_probe.SPRITE_TYPES), len(engine_probe.SEED_OFFSETS))

    def test_exactly_one_rung_reuses_a_shipped_type(self) -> None:
        # The control that proves the placement and capture path works at all.
        reused = [
            index
            for index in range(len(engine_probe.SPRITE_TYPES))
            if not engine_probe.is_freshly_registered(index)
        ]
        self.assertEqual(reused, [0])

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
