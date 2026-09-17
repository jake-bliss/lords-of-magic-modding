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


class SharedProbeSafetyTest(unittest.TestCase):
    """Properties that must hold for EVERY probe body, checked against each in turn.

    Each of these corresponds to something that has actually gone wrong in a run: an unbalanced
    procedure, a key that auto-repeated, a cleanup that destroyed somebody else's building, and a
    packed location read as a coordinate pair.
    """

    def bodies(self):
        return {name: builder() for name, builder in engine_probe.PROBES.items()}

    def test_braces_and_brackets_balance(self) -> None:
        for name, body in self.bodies().items():
            tokens = list(gs_syntax.tokens(body))
            for opener, closer, what in (("{", "}", "braces"), ("[", "]", "brackets")):
                depth = 0
                for token in tokens:
                    if token == opener:
                        depth += 1
                    elif token == closer:
                        depth -= 1
                        self.assertGreaterEqual(depth, 0, f"{name}: {closer} precedes {opener}")
                self.assertEqual(depth, 0, f"{name}: {what} do not balance")

    def test_fires_once_per_launch(self) -> None:
        for name, body in self.bodies().items():
            guard = body.index("userdict /zdone known not")
            flag = body.index("/zdone true def")
            first_effect = min(body.index("screencapture"), body.index("addterrainsprite"))
            self.assertLess(guard, flag, name)
            self.assertLess(flag, first_effect, f"{name}: acts before the fire-once flag")

    def test_never_cleans_up_by_location_alone(self) -> None:
        for name, body in self.bodies().items():
            self.assertNotIn("terrainspriteat", body, name)

    def test_packed_locations_are_never_read_as_a_pair(self) -> None:
        for name, body in self.bodies().items():
            self.assertIn("anythinglocation /zaloc exch def", body, name)
            self.assertIn("zaloc xy_to_x_y /zay0 exch def /zax0 exch def", body, name)
            self.assertNotIn("anythinglocation /zay0", body, name)
            # findemptylocation takes (location, unittype); three operands strands the x.
            self.assertNotIn("add UNITTYPELAND findemptylocation", body.replace(
                "x_y_to_xy UNITTYPELAND findemptylocation", ""), name)

    def test_reports_when_no_army_was_found(self) -> None:
        for name, body in self.bodies().items():
            self.assertIn("no army found", body, name)

    def test_every_body_captures_a_plate_and_a_result(self) -> None:
        for name, body in self.bodies().items():
            for capture in ('"zp0.bmp"screencapture', '"zs1.bmp"screencapture'):
                self.assertIn(capture, body, name)


class ElevationProbeTest(unittest.TestCase):
    def setUp(self) -> None:
        self.body = engine_probe.elevation_body()

    def test_calls_map2screen_both_ways_for_every_surveyed_cell(self) -> None:
        """The whole point: z = 0 beside z = the cell's own elevation, same cell, same call."""
        self.assertIn("zcx zcy getelevation /ze exch def", self.body)
        self.assertIn("zcx zcy 0 map2screen", self.body)
        self.assertIn("zcx zcy ze map2screen", self.body)

    def test_survey_places_nothing(self) -> None:
        survey = self.body[self.body.index("/zdy exch def"):self.body.index("addterrainspritetype")]
        self.assertNotIn("addterrainsprite", survey)

    def test_placements_are_distinct_on_screen(self) -> None:
        """Screen x moves by 33.941*(dx-dy) and the frame is 72 wide, so equal x needs unequal y."""
        seen = set()
        for dx, dy in engine_probe.PLACEMENT_OFFSETS:
            key = (dx - dy, dx + dy)
            self.assertNotIn(key, seen, f"({dx},{dy}) lands on an earlier placement")
            seen.add(key)

    def test_uses_one_freshly_registered_type(self) -> None:
        # A shipped type id would make the type-only sweep destroy the game's own sprites.
        self.assertIn("addterrainspritetype", engine_probe.ELEVATION_SPRITE)
        self.assertEqual(self.body.count("addterrainspritetype"), 1)


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
            self.assertIn(f"getterrainspritelocation zl{index} eq", self.body)

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

    def test_packed_locations_are_never_read_as_a_pair(self) -> None:
        """`anythinglocation` and `getterrainspritelocation` each return ONE packed location.

        Reading either as an x/y pair underflows the operand stack, which is what killed the
        2026-09-16 run: its log printed an empty x beside y=9152, a packed cell (64,71) at map
        width 128. The shipped corpus decomposes with `xy_to_x_y` precisely because the value
        arrives packed.
        """
        self.assertIn("anythinglocation /zaloc exch def", self.body)
        self.assertNotIn("anythinglocation /zay0 exch def", self.body)
        self.assertIn("zaloc xy_to_x_y /zay0 exch def /zax0 exch def", self.body)
        for index in range(len(engine_probe.SPRITE_TYPES)):
            # The cleanup guard must compare packed-to-packed, never packed against a coordinate.
            self.assertNotIn(f"getterrainspritelocation zy{index}", self.body)
            self.assertNotIn(f"getterrainspritelocation zx{index}", self.body)

    def test_findemptylocation_receives_two_operands(self) -> None:
        """Shipped form is `<location> UNITTYPELAND findemptylocation`, not `<x> <y> ...`.

        Passing three operands strands the x on the stack and derives every cell from y alone.
        """
        for index, (_label, _expression) in enumerate(engine_probe.SPRITE_TYPES):
            dx, dy = engine_probe.SEED_OFFSETS[index]
            self.assertIn(
                f"zax0 {dx} add zay0 {dy} add x_y_to_xy UNITTYPELAND findemptylocation "
                f"/zl{index} exch def",
                self.body,
            )

    def test_reports_when_no_army_was_found(self) -> None:
        # Otherwise the probe places at (-1,-1) and the log looks like a rendering failure.
        self.assertIn("no army found", self.body)

    def test_seeds_are_far_enough_apart_to_read(self) -> None:
        """Two rungs resolving to one cell makes the ladder unreadable.

        A cell is roughly 34 screen pixels of x and the donor frame is 72 wide, so seeds closer
        than two cells could overlap on screen even when they land on distinct cells.
        """
        for i, first in enumerate(engine_probe.SEED_OFFSETS):
            for second in engine_probe.SEED_OFFSETS[i + 1:]:
                distance = abs(first[0] - second[0]) + abs(first[1] - second[1])
                self.assertGreaterEqual(distance, 2, f"{first} and {second} are too close")

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
