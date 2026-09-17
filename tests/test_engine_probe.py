"""Checks on the generated engine probe that are cheap here and expensive in the game.

An attended engine run costs a human a game start and a keypress, so a syntax error found by
a unit test is worth several of them. These assert the properties whose absence has actually
cost a run: unbalanced braces, a missing fire-once guard, and cleanup by location.
"""

import re
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

    def placement_bodies(self):
        """Probes that put a sprite on the map, which is what the cleanup rules are about.

        Keyed on the body rather than on a list, so a new placing probe cannot opt itself out of
        the rules by forgetting to register anywhere.
        """
        return {n: b for n, b in self.bodies().items() if "addterrainsprite" in b}

    def army_anchored_bodies(self):
        """Probes that take their cells from a player's army.

        Placing and army-anchoring are not the same thing. A probe that builds its own map picks
        absolute cells and has no army to ask; holding it to the army rules would force it to
        invent a dependency it does not have. What must not happen is a probe calling
        `anythinglocation` and mishandling the packed location it returns.
        """
        return {n: b for n, b in self.bodies().items() if "anythinglocation" in b}

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
            effects = [
                body.index(marker)
                for marker in ("screencapture", "addterrainsprite", "savescenariomap", "clearmap")
                if marker in body
            ]
            self.assertTrue(effects, f"{name}: body has no observable effect at all")
            self.assertLess(guard, flag, name)
            self.assertLess(flag, min(effects), f"{name}: acts before the fire-once flag")

    def test_never_cleans_up_by_location_alone(self) -> None:
        for name, body in self.bodies().items():
            self.assertNotIn("terrainspriteat", body, name)

    def test_every_body_captures_something(self) -> None:
        for name, body in self.bodies().items():
            self.assertIn("screencapture", body, f"{name}: a run with no capture cannot be read")

    def test_writes_only_to_names_the_probe_owns(self) -> None:
        """Every file the probe names is a `z`-prefixed one of ours.

        The game directory holds 366 loose map files and the engine's save operators overwrite
        without asking. A probe that wrote `map/URAK.scn` would destroy shipped content that no
        archive backup covers, because the backups cover `gs.mpq` and `imp.mpq`, not `map/`.
        """
        written = (".bmp", ".log", ".scn", ".smp", ".sav")
        owned_maps = set(engine_probe.generated_map_names())
        for name, body in self.bodies().items():
            for quoted in re.findall(r'"([^"]*)"', body):
                if not quoted.lower().endswith(written):
                    continue  # `.imp` and `.gs` names are read, and reads harm nothing
                stem = quoted.rsplit("/", 1)[-1]
                if quoted.lower().startswith("map/"):
                    # Anything under map/ has to be a name the cleanup path actually knows about.
                    # A `map/zURAK.scn` starts with `z` and would satisfy a looser rule, but the
                    # install and restore scripts work from `generated_map_names()`, so it would
                    # be written into an unbacked-up directory and left there for ever.
                    self.assertIn(
                        quoted,
                        owned_maps,
                        f"{name}: writes {quoted!r}, which the cleanup path would not remove",
                    )
                    continue
                self.assertTrue(
                    stem.startswith("z"),
                    f"{name}: writes {quoted!r}, which the probe does not own",
                )

    def test_army_anchored_probes_unpack_the_packed_location(self) -> None:
        for name, body in self.army_anchored_bodies().items():
            self.assertIn("anythinglocation /zaloc exch def", body, name)
            self.assertIn("zaloc xy_to_x_y /zay0 exch def /zax0 exch def", body, name)
            self.assertNotIn("anythinglocation /zay0", body, name)
            # findemptylocation takes (location, unittype); three operands strands the x.
            self.assertNotIn("add UNITTYPELAND findemptylocation", body.replace(
                "x_y_to_xy UNITTYPELAND findemptylocation", ""), name)

    def test_army_anchored_probes_report_when_no_army_was_found(self) -> None:
        for name, body in self.army_anchored_bodies().items():
            self.assertIn("no army found", body, name)

    def test_placing_probes_capture_a_plate_and_a_result(self) -> None:
        for name, body in self.placement_bodies().items():
            for capture in ('"zp0.bmp"screencapture', '"zs1.bmp"screencapture'):
                self.assertIn(capture, body, name)

    def test_placing_probes_remove_what_they_placed(self) -> None:
        """A probe that leaves sprites behind has changed the map it was measuring.

        Every placement is matched by a `destroyterrainsprite` guarded on the type, and the type
        has to be one this keypress minted -- a shipped type id is shared with the map's own
        sprites, which is how a village was destroyed on 2026-09-16.
        """
        for name, body in self.placement_bodies().items():
            self.assertIn("destroyterrainsprite", body, name)
            self.assertIn("getterrainspritetype", body, name)
            self.assertIn("addterrainspritetype", body, f"{name}: no freshly minted type to sweep")


class MapSizeProbeTest(unittest.TestCase):
    """The oversized-map ladder for issue #22."""

    def setUp(self) -> None:
        self.body = engine_probe.map_size_body()

    def test_controls_come_before_the_subject(self) -> None:
        """128 and 256 exist in the shipped corpus; 512 does not.

        If a generated 128 does not match a shipped 128, the generator is not a faithful writer and
        the 512 result means nothing. The control has to run first, and has to be in the ladder at
        all.
        """
        self.assertEqual(engine_probe.MAP_SIZES[:2], [128, 256])
        self.assertEqual(engine_probe.MAP_SIZES[-1], 512)
        positions = [self.body.index(f"{size} {size} make_custom_random_map")
                     for size in engine_probe.MAP_SIZES]
        self.assertEqual(positions, sorted(positions))

    def test_operand_order_is_width_then_height(self) -> None:
        """From the only shipped call site, gs\\edit\\mapgen.gs:306 -- width is pushed first.

        Asserted with UNEQUAL arguments. Every size in the ladder is square, so an assertion made
        against the ladder alone would pass just as happily with the operands transposed, and the
        whole point of this order is that the arity table could not settle it.
        """
        self.assertEqual(engine_probe.generate_call(512, 256), "512 256 make_custom_random_map")
        for size in engine_probe.MAP_SIZES:
            self.assertIn(engine_probe.generate_call(size, size), self.body)

    def test_logs_the_engines_own_view_of_the_size(self) -> None:
        """A silent clamp would otherwise look exactly like a successful oversized generation."""
        for size in engine_probe.MAP_SIZES:
            self.assertIn(f'"gen done "{size}" mapw "mapw" maph "maph', self.body)

    def test_save_result_is_captured_not_popped(self) -> None:
        self.assertIn("zname savescenariomap /zok exch def", self.body)
        self.assertNotIn("savescenariomap pop", self.body)

    def test_every_size_saves_to_its_own_new_file(self) -> None:
        """The names in the body and the names the scripts clean up must be the same list.

        They are read from one function so they cannot drift, and this is what would catch it if
        somebody reintroduced a second copy.
        """
        names = re.findall(r'"(map/[^"]*)"', self.body)
        self.assertEqual(names, engine_probe.generated_map_names())
        self.assertEqual(names, [f"map/zz{size}.scn" for size in engine_probe.MAP_SIZES])
        self.assertEqual(len(set(names)), len(names), "two sizes share a filename")

    def test_places_nothing_and_destroys_nothing(self) -> None:
        for forbidden in ("addterrainsprite", "destroyterrainsprite", "savespecialmap"):
            self.assertNotIn(forbidden, self.body)


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


class FlatGroundProbeTest(unittest.TestCase):
    """The probe that builds its own mesh to close the `map2screen` y convention."""

    def setUp(self) -> None:
        self.body = engine_probe.flat_ground_body()

    def test_three_phases_in_the_order_that_makes_them_comparable(self) -> None:
        positions = [self.body.index(f'"zs{n}.bmp"screencapture') for n in (1, 2, 3)]
        self.assertEqual(positions, sorted(positions))
        for tag in ("flat", "plateau", "spike"):
            self.assertIn(f'"cleanup {tag} done"', self.body)

    def test_each_phase_has_its_own_plate(self) -> None:
        """A phase differenced against another phase's terrain would show the whole changed mesh.

        Phases B and C move the ground, so a single plate taken at the start would put every
        raised cell into the difference and bury the sprites the probe is trying to measure.
        """
        for plate in ('"zp0.bmp"', '"zp1.bmp"', '"zp2.bmp"'):
            self.assertIn(f"{plate}screencapture", self.body)
        for plate, shot in (("zp0", "zs1"), ("zp1", "zs2"), ("zp2", "zs3")):
            self.assertLess(
                self.body.index(f'"{plate}.bmp"'),
                self.body.index(f'"{shot}.bmp"'),
                f"{plate} must be captured before {shot}",
            )

    def test_plateau_and_spike_put_the_same_elevation_on_the_placement_cells(self) -> None:
        """This is the entire experiment: same cell, same `getelevation`, opposite neighbourhood.

        If the two phases used different elevations, any difference in drawn y would be explained
        by the elevation and would say nothing about the mesh.
        """
        elevation = engine_probe.FLAT_ELEVATION

        # The block-wide writes, in the order the body performs them. Asserted as a SEQUENCE: a
        # substring search cannot tell the spike phase's block-lowering from phase A's flattening,
        # and a spike phase that never lowers the block is just a second plateau -- which would
        # make the two phases agree for a reason that has nothing to do with the mesh.
        block_writes = re.findall(r"zsx zsy (\S+) setelevation", self.body)
        self.assertEqual(
            block_writes,
            ["0", elevation, "0"],
            "expected flatten, then raise the block, then lower it again before the spikes",
        )

        # And the six cells are raised after that final lowering.
        cell_writes = re.findall(
            rf"zrow (\d+) get {engine_probe.FLAT_ROW_Y} (\S+) setelevation", self.body
        )
        self.assertEqual(
            cell_writes,
            [(str(i), elevation) for i in range(len(engine_probe.FLAT_ROW_X))],
        )
        last_lowering = [m.start() for m in re.finditer(r"zsx zsy 0 setelevation", self.body)][-1]
        first_spike = self.body.index(f"zrow 0 get {engine_probe.FLAT_ROW_Y} {elevation}")
        self.assertLess(last_lowering, first_spike)

        x0, y0, x1, y1 = engine_probe.FLAT_BLOCK
        self.assertLessEqual(y0, y1)
        self.assertLessEqual(x0, x1)

    def test_the_plateau_block_clears_every_placement_neighbourhood(self) -> None:
        """A mesh sampler must not be able to see the block's edge from any placement cell."""
        x0, y0, x1, y1 = engine_probe.FLAT_BLOCK
        margin = 2
        for x in engine_probe.FLAT_ROW_X:
            self.assertGreaterEqual(x - x0, margin, f"cell x={x} is too close to the block edge")
            self.assertGreaterEqual(x1 - x, margin, f"cell x={x} is too close to the block edge")
        self.assertGreaterEqual(engine_probe.FLAT_ROW_Y - y0, margin)
        self.assertGreaterEqual(y1 - engine_probe.FLAT_ROW_Y, margin)
        self.assertLess(x1, engine_probe.FLAT_MAP)
        self.assertLess(y1, engine_probe.FLAT_MAP)

    def test_every_placement_gets_its_own_screen_x(self) -> None:
        """The earlier run had two pairs sharing a screen x and picked the assignment that fit.

        Screen x is `33.941 * (x - y) + L`. With one row, distinct x means distinct screen x, and
        the separation must exceed the 72-pixel donor frame or the clusters merge.
        """
        columns = engine_probe.FLAT_ROW_X
        self.assertEqual(len(set(columns)), len(columns))
        pixels_per_step = 33.941
        gaps = [
            (b - a) * pixels_per_step for a, b in zip(sorted(columns), sorted(columns)[1:])
        ]
        self.assertTrue(all(gap > 72 for gap in gaps), f"frames would overlap: {gaps}")
        span = (max(columns) - min(columns)) * pixels_per_step
        self.assertLess(span, 640 - 72, "the row does not fit on a 640-pixel screen")

    def test_the_camera_is_pointed_rather_than_inherited(self) -> None:
        """No army here, so nothing positions the view unless the probe does it."""
        x, y = engine_probe.FLAT_CAMERA
        self.assertIn(f"{x} {y} centeron", self.body)
        self.assertEqual(self.body.count("centeron"), 3, "each phase re-points after its rebuild")

    def test_the_mesh_is_rebuilt_after_every_elevation_change(self) -> None:
        """Without a rebuild the render keeps the old heights and all three phases look alike."""
        self.assertEqual(self.body.count("rebuild3dmap"), 3)

    def test_it_builds_its_own_map_and_saves_nothing(self) -> None:
        """Nothing of the game's is touched: the map is created in memory and never written."""
        self.assertIn(f"{engine_probe.FLAT_MAP} {engine_probe.FLAT_MAP} newmap", self.body)
        for forbidden in ("savescenariomap", "savespecialmap", "terrainspriteat"):
            self.assertNotIn(forbidden, self.body)


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
