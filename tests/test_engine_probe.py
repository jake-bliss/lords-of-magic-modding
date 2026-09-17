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
SCRIPTS_DIR = Path(__file__).resolve().parents[1] / "scripts"

import engine_probe  # noqa: E402
import gs_syntax  # noqa: E402
import map_projection  # noqa: E402
import terrain_rings  # noqa: E402


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
        """A capture before the first placement and another after it.

        Named by position rather than by filename: each probe owns its own capture names, because
        sharing them across probes is what overwrote the elevation run's collected output.
        """
        for name, body in self.placement_bodies().items():
            captures = [m.start() for m in re.finditer(r'"z[^"]*\.bmp"screencapture', body)]
            # `addterrainsprite` is a prefix of `addterrainspritetype`, which registers a type
            # long before anything is placed. Matching the prefix put the "first placement" at the
            # registration and made the plate look late.
            placement = re.search(r"addterrainsprite(?!type)", body)
            self.assertIsNotNone(placement, name)
            first_placement = placement.start()
            self.assertTrue(
                any(at < first_placement for at in captures),
                f"{name}: no plate captured before the first placement",
            )
            self.assertTrue(
                any(at > first_placement for at in captures),
                f"{name}: no capture after placing",
            )

    def test_no_two_probes_share_a_capture_name(self) -> None:
        """A shared name is only destructive once the restore script collects it.

        That is not hypothetical: the mapsize probe reused `zprobe.log` and the elevation run's
        survey log -- the raw data behind the map2screen decode -- was overwritten on 2026-09-17.
        The restore script now collects into a per-run directory, and this keeps the names distinct
        as well.
        """
        seen: dict[str, str] = {}
        for name, body in self.bodies().items():
            for capture in set(re.findall(r'"(z[^"]*\.bmp)"screencapture', body)):
                if capture in seen:
                    self.fail(f"{name} and {seen[capture]} both capture {capture}")
                seen[capture] = name

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
        self.assertEqual(names, engine_probe.map_size_map_names())
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
    """The probe that builds its own mesh to close the `map2screen` y convention.

    These assert on the ORDER of emitted operations, not on the presence of strings. A probe body
    is a program: `rebuild3dmap` three times says nothing if all three run before the elevations
    change, and three cleanup sweeps say nothing if they all run at the end.
    """

    PIXELS_PER_X_STEP = 33.941
    SCREEN_WIDTH = 640
    FRAME_WIDTH = 72

    def setUp(self) -> None:
        self.body = engine_probe.flat_ground_body()

    def _phase_spans(self):
        """(tag, start, end) for each phase, bounded by its own plate and the next phase's."""
        marks = [(tag, self.body.index(f'"{plate}"screencapture'))
                 for tag, plate, _ in engine_probe.FLAT_PHASES]
        spans = []
        for index, (tag, begin) in enumerate(marks):
            finish = marks[index + 1][1] if index + 1 < len(marks) else len(self.body)
            spans.append((tag, begin, finish))
        return spans

    def test_three_phases_in_the_order_that_makes_them_comparable(self) -> None:
        shots = [self.body.index(f'"{shot}"screencapture')
                 for _, _, shot in engine_probe.FLAT_PHASES]
        self.assertEqual(shots, sorted(shots))
        self.assertEqual([tag for tag, _, _ in engine_probe.FLAT_PHASES],
                         ["flat", "plateau", "spike"])

    def test_every_capture_name_is_unique_and_this_probes_own(self) -> None:
        """Each probe owns its capture names, and none may collide inside one run.

        The engine also refuses to overwrite an existing capture, so a duplicate name inside one
        run silently loses the second shot.
        """
        names = re.findall(r'"(z[^"]*\.bmp)"screencapture', self.body)
        self.assertEqual(len(names), len(set(names)), f"duplicate capture name: {names}")
        expected = [n for phase in engine_probe.FLAT_PHASES for n in phase[1:]]
        self.assertEqual(names, expected)
        for other in ("zl0.bmp", "ze0.bmp", "zm128.bmp"):
            self.assertNotIn(f'"{other}"', self.body, "collides with an earlier probe's captures")

    def test_each_phase_plates_before_it_places_and_shoots_after(self) -> None:
        """Asserting only plate-before-shot passes for `place, plate, shot`.

        That ordering puts the sprites in the plate, the difference comes out empty, and it reads
        exactly like "nothing rendered".
        """
        for (tag, plate, shot), (_, begin, finish) in zip(
            engine_probe.FLAT_PHASES, self._phase_spans()
        ):
            window = self.body[begin:finish]
            plate_at = window.index(f'"{plate}"screencapture')
            first_placement = window.index(f'"place {tag} 0 cell ')
            shot_at = window.index(f'"{shot}"screencapture')
            self.assertLess(plate_at, first_placement, f"{plate} is taken after {tag} places")
            self.assertLess(first_placement, shot_at, f"{shot} is taken before {tag} places")

    def test_each_phase_rebuilds_and_re_points_before_its_own_plate(self) -> None:
        """A rebuild that happens before the elevation change renders the previous phase's mesh."""
        camera = f"{engine_probe.FLAT_CAMERA[0]} {engine_probe.FLAT_CAMERA[1]} centeron"
        for (tag, plate, _), (_, begin, finish) in zip(
            engine_probe.FLAT_PHASES, self._phase_spans()
        ):
            window = self.body[begin:finish]
            # The rebuild and centeron for a phase sit just before its plate, which is where this
            # window starts, so look in the run-up to it instead.
            run_up = self.body[:begin]
            self.assertIn("rebuild3dmap", run_up.rsplit(f'"{plate}"', 1)[0][-400:],
                          f"{tag} does not rebuild immediately before its plate")
            self.assertIn(camera, run_up[-400:], f"{tag} does not re-point the camera")
            self.assertNotIn("setelevation", window.split(f'"{plate}"')[0],
                             f"{tag} changes elevations after its own rebuild")

    def test_each_phase_sweeps_twice_and_counts_survivors(self) -> None:
        """One sweep is not enough: destroying during an enumeration may advance past an entry.

        A survivor is the same art on the same cell in the next phase's plate AND shot, so it
        contributes zero changed pixels and reads as "the sprite did not render". The count turns
        that silent corruption into a line in the log.
        """
        sweep = ("{dup getterrainspritetype zt eq"
                 "{destroyterrainsprite}{pop}ifelse}enumterrainsprites")
        for tag, begin, finish in self._phase_spans():
            window = self.body[begin:finish]
            self.assertEqual(window.count(sweep), 2, f"{tag} sweeps once; it must sweep twice")
            self.assertIn("/zleft 0 def", window, f"{tag} does not count survivors")
            self.assertIn(f'"cleanup {tag} left "zleft', window)
            self.assertLess(
                window.index(f'"{engine_probe.FLAT_PHASES[0][2]}"' if False else sweep),
                window.index(f'"cleanup {tag} left "zleft'),
            )

    def test_every_phase_cleans_up_before_the_next_one_plates(self) -> None:
        for index, (tag, begin, finish) in enumerate(self._phase_spans()[:-1]):
            cleanup_at = begin + self.body[begin:finish].index(f'"cleanup {tag} left "zleft')
            next_plate = self.body.index(f'"{engine_probe.FLAT_PHASES[index + 1][1]}"screencapture')
            self.assertLess(cleanup_at, next_plate, f"{tag}'s sprites survive into the next plate")

    def test_plateau_and_spike_put_the_same_elevation_on_the_placement_cells(self) -> None:
        """The experiment: same cell, same `getelevation`, opposite neighbourhood.

        The block writes are asserted as a SEQUENCE. A substring search cannot tell the spike
        phase's block-lowering from phase A's flattening, and a spike phase that never lowers the
        block is a second plateau -- the two phases would then agree for a reason that has nothing
        to do with the mesh.
        """
        elevation = engine_probe.FLAT_ELEVATION
        block_writes = re.findall(r"zsx zsy (\S+) setelevation", self.body)
        self.assertEqual(block_writes, ["0", elevation, "0"])

        cell_writes = re.findall(
            rf"zrow (\d+) get {engine_probe.FLAT_ROW_Y} (\S+) setelevation", self.body
        )
        self.assertEqual(cell_writes,
                         [(str(i), elevation) for i in range(len(engine_probe.FLAT_ROW_X))])
        last_lowering = [m.start() for m in re.finditer(r"zsx zsy 0 setelevation", self.body)][-1]
        self.assertLess(last_lowering,
                        self.body.index(f"zrow 0 get {engine_probe.FLAT_ROW_Y} {elevation}"))

    def test_the_block_loops_use_the_declared_bounds(self) -> None:
        """Checking the constant alone would pass on a body that looped over something else."""
        x0, y0, x1, y1 = engine_probe.FLAT_BLOCK
        self.assertEqual(self.body.count(f"{x0} 1 {x1}{{/zsx exch def"), 2)
        self.assertEqual(self.body.count(f"{y0} 1 {y1}{{/zsy exch def"), 2)
        self.assertIn(f"0 1 {engine_probe.FLAT_MAP - 1}{{/zsx exch def", self.body)

    def test_the_plateau_block_clears_every_placement_neighbourhood(self) -> None:
        x0, y0, x1, y1 = engine_probe.FLAT_BLOCK
        margin = 2
        for x in engine_probe.FLAT_ROW_X:
            self.assertGreaterEqual(x - x0, margin)
            self.assertGreaterEqual(x1 - x, margin)
        self.assertGreaterEqual(engine_probe.FLAT_ROW_Y - y0, margin)
        self.assertGreaterEqual(y1 - engine_probe.FLAT_ROW_Y, margin)
        self.assertLess(x1, engine_probe.FLAT_MAP)
        self.assertLess(y1, engine_probe.FLAT_MAP)

    def test_the_control_sprite_stands_on_ground_that_never_changes(self) -> None:
        """It is the only thing that can tell a camera shift from a mesh effect.

        The camera cannot be moved out of the plateau -- framing the row pins it to within a cell
        or two of the row itself -- so the camera cell's elevation differs between the plateau and
        spike phases. If `centeron` reads terrain height, the whole viewport moves and every sprite
        moves with it. This one does not stand on ground that ever changes, so its movement is
        purely camera.
        """
        cx, cy = engine_probe.FLAT_CONTROL
        x0, y0, x1, y1 = engine_probe.FLAT_BLOCK
        margin = 2
        outside = cx < x0 - margin or cx > x1 + margin or cy < y0 - margin or cy > y1 + margin
        self.assertTrue(outside, "the control sits in or beside the plateau, so it moves with it")
        self.assertNotIn((cx, cy), [(x, engine_probe.FLAT_ROW_Y)
                                    for x in engine_probe.FLAT_ROW_X])
        for tag, _, _ in engine_probe.FLAT_PHASES:
            self.assertIn(f'"control {tag} cell "zx" "zy" elev "ze', self.body)
        self.assertEqual(self.body.count(f"/zx {cx} def /zy {cy} def"),
                         len(engine_probe.FLAT_PHASES))

    def test_the_control_does_not_collide_with_a_placement_on_screen(self) -> None:
        """Same screen x is allowed; the 259 pixels of vertical separation is what keeps it apart."""
        cx, cy = engine_probe.FLAT_CONTROL
        for x in engine_probe.FLAT_ROW_X:
            dx = abs((cx - cy) - (x - engine_probe.FLAT_ROW_Y)) * self.PIXELS_PER_X_STEP
            dy = abs((cx + cy) - (x + engine_probe.FLAT_ROW_Y)) * 14.4
            self.assertTrue(dx > self.FRAME_WIDTH or dy > 67,
                            f"control overlaps the sprite at x={x}: dx={dx:.0f} dy={dy:.0f}")

    def test_every_placement_is_on_screen_once_the_camera_is_accounted_for(self) -> None:
        """The anchor is the drawn LEFT edge, so the frame extends further right.

        Checking only the row's span passes on a row that is centred badly and runs off the screen;
        with the camera at `x - y = 1` the rightmost frame reached x 640 exactly.
        """
        camera_x, camera_y = engine_probe.FLAT_CAMERA
        camera_step = camera_x - camera_y
        for x, y in _flat_cells_for_test():
            left = self.SCREEN_WIDTH / 2 + ((x - y) - camera_step) * self.PIXELS_PER_X_STEP
            right = left + self.FRAME_WIDTH
            self.assertGreaterEqual(left, 0, f"cell ({x},{y}) is off the left edge")
            self.assertLessEqual(right, self.SCREEN_WIDTH, f"cell ({x},{y}) is clipped on the right")

    def test_every_placement_gets_its_own_screen_x(self) -> None:
        columns = engine_probe.FLAT_ROW_X
        self.assertEqual(len(set(columns)), len(columns))
        gaps = [(b - a) * self.PIXELS_PER_X_STEP
                for a, b in zip(sorted(columns), sorted(columns)[1:])]
        self.assertTrue(all(gap > self.FRAME_WIDTH for gap in gaps), f"frames overlap: {gaps}")

    def test_every_phase_places_on_all_six_cells_and_the_control(self) -> None:
        """Checking the constant would pass on a body that placed the same cell six times."""
        for tag, begin, finish in self._phase_spans():
            window = self.body[begin:finish]
            placed = re.findall(r"/zx (\d+) def /zy (\d+) def", window)
            self.assertEqual(
                [(int(a), int(b)) for a, b in placed],
                _flat_cells_for_test(),
                f"{tag} does not place on every cell exactly once",
            )
            self.assertEqual(window.count("zx zy zt addterrainsprite"), len(placed))

    def test_it_builds_its_own_map_and_saves_nothing(self) -> None:
        self.assertIn(f"{engine_probe.FLAT_MAP} {engine_probe.FLAT_MAP} newmap", self.body)
        for forbidden in ("savescenariomap", "savespecialmap", "terrainspriteat"):
            self.assertNotIn(forbidden, self.body)


def _flat_cells_for_test():
    return [(x, engine_probe.FLAT_ROW_Y) for x in engine_probe.FLAT_ROW_X] + [
        engine_probe.FLAT_CONTROL
    ]


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


class MapTagProbeTest(unittest.TestCase):
    """Issue #4's probe. Asserts ORDER, because presence is nearly free to satisfy by accident.

    Every claim this probe can make rests on a sequence: the background has to be painted before
    the rows, the rows before the first save, and the first save before a single sprite exists. A
    test that only checks that each operator appears somewhere would pass on a body that did them
    in any order at all, and the saved bytes would be uninterpretable.
    """

    def setUp(self) -> None:
        self.body = engine_probe.map_tag_body()

    def _at(self, needle: str) -> int:
        index = self.body.find(needle)
        self.assertNotEqual(index, -1, f"{needle!r} missing from the body")
        return index

    def _place_positions(self) -> list[int]:
        return [
            match.start()
            for match in re.finditer(r"addterrainsprite(?!type)", self.body)
        ]

    def test_map_names_are_the_probes_own_list_in_order(self) -> None:
        names = re.findall(r'"(map/[^"]*)"', self.body)
        self.assertEqual(names, engine_probe.map_tag_map_names())
        self.assertEqual(len(set(names)), len(names), "two saves share a filename")

    def test_generated_map_names_is_exactly_the_union(self) -> None:
        """Cleanup must cover every probe, and must not invent a name no probe writes.

        The install and restore scripts delete by this list. A name in it that no probe writes is
        an offer to delete a file the probe did not create, in a directory with no backup.
        """
        union = (
            engine_probe.map_size_map_names()
            + engine_probe.map_tag_map_names()
            + [f"map/{name}" for name in engine_probe.mapload_output_names()]
            + engine_probe.rings_generated_names()
            + [f"map/{name}" for name in engine_probe.mapload_prebuilt_names()]
        )
        self.assertEqual(engine_probe.generated_map_names(), union)
        self.assertEqual(len(set(union)), len(union), "two probes share a map filename")

    def test_background_is_painted_before_either_row(self) -> None:
        """`clearmap` re-runs `newmap`, so anything painted before it is erased."""
        clear = self._at(f"{engine_probe.MAPTAG_BASE_TEXTURE} clearmap")
        self.assertLess(clear, self._at("forcetexture"))
        self.assertLess(clear, self._at("setterrain"))

    def test_the_two_rows_never_touch_the_same_cell(self) -> None:
        """A cell painted by both operators cannot say which one set its tag."""
        forced = {
            (x, y)
            for x, y, _ in engine_probe._map_tag_cells(
                engine_probe.MAPTAG_TEXTURES, engine_probe.MAPTAG_TEXTURE_ROW
            )
        }
        terrained = {
            (x, y)
            for x, y, _ in engine_probe._map_tag_cells(
                engine_probe.MAPTAG_TERRAINS, engine_probe.MAPTAG_TERRAIN_ROW
            )
        }
        self.assertEqual(forced & terrained, set())
        self.assertEqual(len(forced), len(engine_probe.MAPTAG_TEXTURES))
        self.assertEqual(len(terrained), len(engine_probe.MAPTAG_TERRAINS))

    def test_every_terrain_type_is_covered(self) -> None:
        """`gs\\maplib.gs` defines 0..10 and the probe has to ask for all of them.

        A partial sweep would leave the tile the engine picks for the missing type unmeasured,
        and that mapping is half of what the probe exists to recover.
        """
        self.assertEqual(engine_probe.MAPTAG_TERRAINS, list(range(11)))

    def test_forced_row_spans_the_atlas(self) -> None:
        """Slot 0 and the last declared slot both have to be asked for.

        `tilesb01.til` declares 624 slots and the corpus never exceeds 623. If the tag were a
        narrower field than the index, only a high slot would show it, so the high end is the
        informative one and must not be quietly dropped.
        """
        self.assertIn(0, engine_probe.MAPTAG_TEXTURES)
        self.assertEqual(max(engine_probe.MAPTAG_TEXTURES), 623)

    def test_each_painted_cell_is_logged_after_it_is_painted(self) -> None:
        """A log line before the write would survive a write that faulted."""
        for x, y, texture in engine_probe._map_tag_cells(
            engine_probe.MAPTAG_TEXTURES, engine_probe.MAPTAG_TEXTURE_ROW
        ):
            write = self._at(f"zx zy {texture} forcetexture")
            self.assertLess(write, self._at(f'"forced cell "zx" "zy" texture {texture} '))
        for x, y, terrain in engine_probe._map_tag_cells(
            engine_probe.MAPTAG_TERRAINS, engine_probe.MAPTAG_TERRAIN_ROW
        ):
            write = self._at(f"zx zy {terrain} setterrain")
            self.assertLess(write, self._at(f'"terrain cell "zx" "zy" set {terrain} '))

    def test_the_scn_and_smp_pair_save_the_same_state(self) -> None:
        """The 49-versus-52-byte question is only answerable if nothing changed between them."""
        scn, smp, _, _ = engine_probe.map_tag_map_names()
        between = self.body[self._at(f'"{scn}"') : self._at(f'"{smp}"')]
        for mutation in ("forcetexture", "setterrain", "addterrainsprite", "clearmap", "newmap"):
            self.assertNotIn(mutation, between, f"{mutation} runs between the two saves")

    def test_terrain_only_saves_happen_before_any_sprite_exists(self) -> None:
        _, smp, _, _ = engine_probe.map_tag_map_names()
        self.assertLess(self._at(f'"{smp}"'), self._place_positions()[0])

    def test_sprite_save_happens_after_every_placement(self) -> None:
        _, _, with_sprites, _ = engine_probe.map_tag_map_names()
        self.assertLess(max(self._place_positions()), self._at(f'"{with_sprites}"'))
        self.assertEqual(len(self._place_positions()), len(engine_probe.MAPTAG_SPRITE_CELLS))

    def test_removed_save_happens_after_both_sweeps(self) -> None:
        """A survivor left standing writes a record into the file that is supposed to lack one."""
        _, _, _, removed = engine_probe.map_tag_map_names()
        sweeps = [m.start() for m in re.finditer("destroyterrainsprite", self.body)]
        self.assertEqual(len(sweeps), 2)
        self.assertLess(max(sweeps), self._at(f'"{removed}"'))
        self.assertLess(self._at('"count after cleanup "'), self._at(f'"{removed}"'))

    def test_sprite_cells_separate_packed_from_x_major(self) -> None:
        """Three cells whose two candidate encodings cannot be confused.

        Before the run, the corpus was read as X-major (`x * height + y`) while sprite *locations*
        looked packed (`y * width + x`). The cells are chosen to give six distinct numbers rather
        than to look tidy, which is the only reason the run could tell them apart. It did: the
        saved records came back packed, and X-major is refuted. Keep the property -- the next probe
        that places sprites needs it too.
        """
        cells = engine_probe.MAPTAG_SPRITE_CELLS
        size = engine_probe.MAPTAG_MAP
        packed = [y * size + x for x, y in cells]
        x_major = [x * size + y for x, y in cells]
        self.assertEqual(len(set(packed)), len(cells))
        self.assertEqual(len(set(x_major)), len(cells))
        self.assertEqual(set(packed) & set(x_major), set())

    def test_every_placement_is_inside_the_frame_the_camera_gives(self) -> None:
        """A capture pair around a placement nobody can see is two screenshots and no evidence.

        The 2026-09-17 run proved this the expensive way: the plate and the shot differed by zero
        bytes because the sprites landed off the left edge and under the editor panel. The shared
        rule that a placing probe brackets its placements with captures is necessary and not
        sufficient -- it cannot see whether the subject is in the picture. The projection is
        decoded, so this is a check rather than a hope.
        """
        for cell in engine_probe.MAPTAG_SPRITE_CELLS:
            offset = map_projection.screen_offset_from_camera(cell, engine_probe.MAPTAG_CAMERA)
            self.assertTrue(
                map_projection.is_in_frame(cell, engine_probe.MAPTAG_CAMERA),
                f"{cell} projects to {offset} from the camera and would not be drawn in full",
            )

    def test_the_cells_that_photographed_nothing_would_now_fail(self) -> None:
        """The guard has to reject the arrangement that actually failed, or it guards nothing."""
        for cell in ((20, 30), (21, 30), (20, 31)):
            self.assertFalse(map_projection.is_in_frame(cell, engine_probe.MAPTAG_CAMERA))

    def test_the_capture_pair_brackets_the_placements(self) -> None:
        first = self._place_positions()[0]
        self.assertLess(self._at(f'"{engine_probe.MAPTAG_PLATE}"screencapture'), first)
        self.assertGreater(
            self._at(f'"{engine_probe.MAPTAG_SHOT}"screencapture'), max(self._place_positions())
        )

    def test_the_log_is_cycled_after_every_save(self) -> None:
        """A fault on a later save must not take the earlier results down with it."""
        saves = len(re.findall(r"save(?:scenario|special)map", self.body))
        self.assertEqual(saves, len(engine_probe.map_tag_map_names()))
        self.assertEqual(self.body.count('"zprobe.log""abw"file'), saves + 1)



class MapLoadProbeTest(unittest.TestCase):
    """The probe that asks whether the engine will load a map this project wrote.

    Everything here is about ORDER and about the controls. The run is only readable if a failure
    names the step that failed: rung 0 has to prove `loadscenariomap` works at all before any of
    our own bytes are handed over, and rung 1 has to prove delivery with bytes that are *equal* to
    a file the engine certainly loads. If those two are not first, a rejection at rung 2 cannot be
    told apart from a broken instrument.
    """

    def setUp(self) -> None:
        self.body = engine_probe.mapload_body()

    def _at(self, needle: str) -> int:
        index = self.body.find(needle)
        self.assertNotEqual(index, -1, f"{needle!r} missing from the body")
        return index

    def test_the_controls_come_before_anything_of_ours_is_loaded(self) -> None:
        control = self._at('"map/zm0.scn"strcpy\n\tzname loadscenariomap')
        identical = self._at('"map/zm1.scn"strcpy')
        first_edit = self._at('"map/zm2.scn"strcpy')
        created = self._at('"map/zm5.scn"strcpy')
        non_square = self._at('"map/zm6.scn"strcpy')
        self.assertLess(control, identical, "the instrument must be proven before delivery")
        self.assertLess(identical, first_edit, "delivery must be proven before an edited map")
        self.assertLess(first_edit, created, "an edit is a smaller claim than a created map")
        self.assertLess(created, non_square, "square must be proven before non-square")

    def test_the_control_file_is_written_by_the_engine_in_this_keypress(self) -> None:
        """Rung 0 only means something if the engine wrote the file it loads.

        If the control were prepared offline it would be testing our writer, which is the very
        thing it exists to hold constant.
        """
        saved = self._at('"map/zm0.scn"strcpy\n\tzname savescenariomap')
        loaded = self._at('"map/zm0.scn"strcpy\n\tzname loadscenariomap')
        self.assertLess(saved, loaded)
        self.assertLess(self._at("newmap"), saved, "the control map is built before it is saved")

    def test_every_rung_loads_exactly_one_input_in_order(self) -> None:
        loads = re.findall(r'zname"map/(z[^"]*)"strcpy\n\tzname loadscenariomap', self.body)
        self.assertEqual(loads, engine_probe.mapload_input_names())

    def test_every_rung_keeps_the_engines_verdict(self) -> None:
        """A load whose boolean is discarded is a load that cannot be reported.

        All the shipped call sites read `... loadscenariomap not{...}if`, so the operator pushes one
        value. Dropping it would make the whole probe unreadable -- and `pop` is the easy mistake.
        """
        loads = self.body.count("loadscenariomap")
        self.assertEqual(self.body.count("loadscenariomap /zok exch def"), loads)
        self.assertNotIn("loadscenariomap pop", self.body)
        self.assertEqual(self.body.count('loaded "zok'), loads)

    def test_the_renderer_is_only_touched_after_a_successful_load(self) -> None:
        """Rebuilding the 3D map from a rejected state is the likeliest way to lose the whole run.

        A crash at rung 2 would take rungs 3 to 6 with it, so every rebuild sits inside the `zok`
        guard rather than after it.
        """
        rebuilds = self.body.count("rebuild3dmap")
        guards = self.body.count("\tzok\n\t\t{\n")
        self.assertEqual(guards, len(engine_probe.mapload_input_names()))
        # One rebuild per guarded rung, plus the blend map's own rebuild, which follows no load.
        self.assertEqual(rebuilds, guards + 1)

        # Assert PLACEMENT, not existence. The previous version of this checked that *something*
        # appeared earlier in the file -- `max(preceding, blend) > -1` -- which the control's
        # `clearmap` made unconditionally true, so moving the rebuild outside the guard left the
        # suite green. Indentation is the guard: emitted lines inside `\tzok\n\t\t{` carry two
        # tabs, everything at rung level carries one.
        guarded = 0
        for line in self.body.split("\n"):
            if "rebuild3dmap" not in line:
                continue
            if line.startswith("\t\t"):
                guarded += 1
            else:
                # The only ungrated rebuild is the blend map's, which follows no load at all.
                self.assertIn(
                    "rebuild3dmap resetvisibility rendermap refreshdirty",
                    line,
                    f"unexpected ungrated rebuild: {line!r}",
                )
                self.assertTrue(
                    line.startswith("\t") and not line.startswith("\t\t"),
                    f"rebuild at unexpected depth: {line!r}",
                )
        self.assertEqual(
            guarded,
            guards,
            "every per-rung rebuild must sit inside its `zok` guard",
        )

    def test_each_load_is_echoed_back_to_its_own_file(self) -> None:
        """The offline diff of input against echo is the strongest readback available.

        It reports not only that the file was accepted but whether the engine *normalised*
        anything -- the header word, the border bit, the `+24` attribute all show up as byte
        differences. Two rungs sharing an echo name would destroy that.
        """
        echoes = re.findall(r'zname"map/(zn[^"]*)"strcpy', self.body)
        self.assertEqual(echoes, engine_probe.MAPLOAD_ECHOES)
        self.assertEqual(len(set(echoes)), len(echoes))

    def test_the_log_is_cycled_after_every_rung(self) -> None:
        """A fault in a later rung must not take the earlier rungs' results down with it."""
        rungs = len(engine_probe.mapload_input_names())
        # One open at the start, one after the control save, one after each rung.
        self.assertEqual(self.body.count('"zprobe.log""abw"file'), rungs + 2)

    def test_every_rung_captures_a_frame(self) -> None:
        """`loadscenariomap` returning true says the file parsed, not that the map drew."""
        shots = re.findall(r'"(z[^"]*\.bmp)"screencapture', self.body)
        self.assertEqual(
            shots,
            engine_probe.MAPLOAD_SHOTS + [engine_probe.MAPLOAD_BLEND_SHOT],
        )
        self.assertEqual(len(set(shots)), len(shots), "two rungs share a capture filename")

    def test_the_blend_background_is_forced_not_painted(self) -> None:
        """`setterrain` is the operator under test and it blends.

        Sweeping it across the map would lay transitions against the default terrain and then
        partly overwrite them, so the tiles around each blob could not be attributed to the blob.
        """
        background = self._at(f"{engine_probe.MAPLOAD_BLEND_BASE_TILE} clearmap")
        first_blob = self._at('"blob 0 terrain 0')
        self.assertLess(background, first_blob)
        # No setterrain may run before the background is down.
        self.assertGreater(self.body.find("setterrain"), background)

    def test_the_background_terrain_is_read_back_before_any_blob(self) -> None:
        readback = self._at('"blend background terrain reads "')
        self.assertLess(readback, self._at('"blob 0 terrain 0'))

    def test_the_blobs_cannot_blend_into_each_other(self) -> None:
        """The measured blend footprint is one cell beyond the painted run on every side.

        A 3x3 blob therefore influences a 5x5 area. Adjacent blobs must be more than 5 apart or a
        transition tile could belong to either of them.
        """
        origins = [
            engine_probe._mapload_blob_origin(index)
            for index in range(len(engine_probe.MAPLOAD_BLEND_TYPES))
        ]
        self.assertEqual(len(set(origins)), len(origins))
        for left in range(len(origins)):
            for right in range(left + 1, len(origins)):
                (ax, ay), (bx, by) = origins[left], origins[right]
                self.assertGreater(
                    max(abs(ax - bx), abs(ay - by)),
                    5,
                    f"blobs {left} and {right} are close enough to blend together",
                )
        # And every blob, plus its one-cell halo, is inside the map.
        for x, y in origins:
            self.assertGreater(x, 0)
            self.assertGreater(y, 0)
            self.assertLess(x + 3, engine_probe.MAPLOAD_BLEND_MAP - 1)
            self.assertLess(y + 3, engine_probe.MAPLOAD_BLEND_MAP - 1)

    def test_every_terrain_type_gets_a_blob(self) -> None:
        self.assertEqual(engine_probe.MAPLOAD_BLEND_TYPES, list(range(11)))
        painted = [int(value) for value in re.findall(r'"blob \d+ terrain (\d+) at ', self.body)]
        self.assertEqual(painted, engine_probe.MAPLOAD_BLEND_TYPES)

    def test_the_interior_flag_rectangle_is_nowhere_near_an_edge(self) -> None:
        """The corpus only ever flags the perimeter, in 146 of 146 files.

        The point of the rung is to hand the engine an interior flag, so the rectangle has to be
        unmistakably interior on the 128x128 donor -- otherwise a result could be read as the
        engine merely rebuilding a border.

        This reads the rectangle out of **the shell script that actually decides it**. The
        constants in `engine_probe` are referenced by nothing else, so asserting on them alone was
        false assurance: the script could be changed to `0 0 3 3` and this test would still pass
        while rung 4 measured exactly the border it was designed to avoid.
        """
        script = SCRIPTS_DIR.joinpath("build-mapload-inputs.sh").read_text()
        match = re.search(r"--map-flag-rect \S+ (\d+) (\d+) (\d+) (\d+)", script)
        self.assertIsNotNone(match, "the script no longer flags a rectangle")
        x0, y0, x1, y1 = (int(value) for value in match.groups())
        self.assertEqual(
            (x0, y0, x1, y1),
            engine_probe.MAPLOAD_INTERIOR_FLAG,
            "the script and the documented constant disagree",
        )
        self.assertLessEqual(x0, x1)
        self.assertLessEqual(y0, y1)
        for value in (x0, y0):
            self.assertGreater(value, 8, "too close to a border to be read as interior")
        for value in (x1, y1):
            self.assertLess(value, 119, "too close to a border to be read as interior")

    def test_the_probe_cells_distinguish_the_two_packings(self) -> None:
        """Read-back cells must not be symmetric under transposition."""
        for x, y in engine_probe.MAPLOAD_PROBE_CELLS:
            self.assertNotEqual(x, y, f"({x}, {y}) reads the same under either packing")

    def test_created_maps_cover_square_and_non_square(self) -> None:
        """Read from the script, for the same reason as the rectangle above."""
        script = SCRIPTS_DIR.joinpath("build-mapload-inputs.sh").read_text()
        created = [
            (int(w), int(h))
            for w, h in re.findall(r"--map-create (\d+) (\d+) ", script)
        ]
        self.assertEqual(
            created,
            [engine_probe.MAPLOAD_CREATED_SIZE, engine_probe.MAPLOAD_CREATED_NON_SQUARE],
            "the script and the documented constants disagree",
        )
        square, other = created
        self.assertEqual(square[0], square[1])
        self.assertNotEqual(other[0], other[1], "the non-square rung must be non-square")

    def test_the_shell_scripts_take_their_name_lists_from_this_module(self) -> None:
        """The scripts are what run; a Python-only assertion cannot see them.

        `restore-game-archives.sh` already derives its list from `generated_map_names()`. The two
        mapload scripts hardcoded `zm1..zm6`, so adding a rung would have left the build script not
        creating it and the install check not looking for it -- and the probe would then have logged
        a missing file as an engine rejection, which is the worst shape a probe bug can take.
        """
        build = SCRIPTS_DIR.joinpath("build-mapload-inputs.sh").read_text()
        install = SCRIPTS_DIR.joinpath("install-engine-probe.sh").read_text()
        restore = SCRIPTS_DIR.joinpath("restore-game-archives.sh").read_text()
        self.assertIn("mapload_prebuilt_names", build)
        self.assertIn("mapload_prebuilt_names", install)
        self.assertIn("generated_map_outputs", install)
        self.assertIn("generated_map_names", restore)
        # And no script may carry the list inline any more.
        for name, text in (("build", build), ("install", install)):
            self.assertNotRegex(
                text,
                r"zm1 zm2 zm3",
                f"{name} still hardcodes the input list",
            )

    def test_install_time_clearing_never_touches_a_probe_input(self) -> None:
        """Regression: the install script deleted the six input maps it had just verified.

        `install-engine-probe.sh` clears stale probe *output* before a run, because a leftover from
        a previous run would be measured as this run's result. It took that list from
        `generated_map_names()`, which now also contains `mapload`'s inputs -- so the clearing step
        removed the very files rungs 1 to 6 were about to load, after the prerequisite check had
        confirmed they were present. It would have spent an attended session loading nothing.
        """
        inputs = set(engine_probe.generated_map_inputs())
        outputs = set(engine_probe.generated_map_outputs())
        self.assertEqual(inputs & outputs, set(), "a name cannot be both cleared and required")
        # Rung 0's control is engine-written, so it must be cleared, never supplied.
        self.assertIn("map/zm0.scn", outputs)
        self.assertNotIn("map/zm0.scn", inputs)
        # Restore must still remove both, or the probe's files outlive the probe.
        self.assertEqual(
            set(engine_probe.generated_map_names()),
            inputs | outputs,
        )

    def test_input_and_output_names_never_collide(self) -> None:
        """An echo landing on an input would overwrite the thing the next rung loads."""
        inputs = set(engine_probe.mapload_prebuilt_names())
        outputs = set(engine_probe.mapload_output_names())
        self.assertEqual(inputs & outputs, set())
        for name in inputs | outputs:
            self.assertTrue(name.startswith("z"), f"{name} is outside the probe's namespace")


class TerrainRingProbeTest(unittest.TestCase):
    """The 11x11 transition matrix, plus two riders.

    The mapload run measured one background and found nine of eleven terrains sharing a ring. That
    is a regularity, not a law -- road already breaks it -- so a painter needs the whole matrix. The
    assertions here are mostly about ORDER and about the background being laid the one way that does
    not itself blend.
    """

    def setUp(self) -> None:
        self.body = engine_probe.terrain_rings_body()

    def _at(self, needle: str) -> int:
        index = self.body.find(needle)
        self.assertNotEqual(index, -1, f"{needle!r} missing from the body")
        return index

    def test_the_terrain_table_has_not_drifted_from_the_rust_source(self) -> None:
        """`map.rs` owns the terrain-to-tile table; this module mirrors it for `clearmap`.

        Two copies of a measured table is how one of them ends up stale, so this parses the values
        back out of the Rust rather than trusting a comment that says they match.
        """
        source = (
            Path(__file__).resolve().parents[1]
            / "spikes"
            / "asset-viewer"
            / "src"
            / "map.rs"
        ).read_text()
        pairs = re.findall(
            r"terrain_type:\s*(\d+),\s*base_tile:\s*(\d+),", source
        )
        self.assertEqual(len(pairs), 11, "did not find all eleven rows in map.rs")
        rust = {int(terrain): int(tile) for terrain, tile in pairs}
        self.assertEqual(engine_probe.TERRAIN_BASE_TILES, rust)

    def test_every_background_is_forced_with_clearmap_never_painted(self) -> None:
        """`setterrain` is the operator under test and it blends.

        Laying a background with it would put transitions against the default terrain and then
        partly overwrite them, so the ring around each blob could not be attributed to the blob.
        `clearmap` forces one tile everywhere and blends nothing.
        """
        for background, tile in sorted(engine_probe.TERRAIN_BASE_TILES.items()):
            marker = f"{engine_probe.RINGS_MAP} {engine_probe.RINGS_MAP} newmap {tile} clearmap"
            self.assertIn(marker, self.body, f"background {background} is not forced")
        # And no setterrain may appear before the first background is down.
        self.assertGreater(self.body.find("setterrain"), self._at("clearmap"))

    def test_each_background_is_read_back_before_any_blob_is_painted(self) -> None:
        """A forced tile that does not answer the intended type invalidates that whole row."""
        for background in engine_probe.RINGS_TERRAINS:
            readback = self._at(f'"background {background} tile ')
            first_blob = self._at(f'"bg {background} blob 0 ')
            self.assertLess(
                readback, first_blob, f"background {background} is painted before it is checked"
            )

    def test_all_eleven_backgrounds_carry_all_eleven_terrains(self) -> None:
        painted = re.findall(r'"bg (\d+) blob (\d+) terrain (\d+) at ', self.body)
        self.assertEqual(len(painted), 121, "the matrix must be complete")
        seen = {(int(bg), int(terrain)) for bg, _, terrain in painted}
        self.assertEqual(
            seen,
            {
                (bg, terrain)
                for bg in engine_probe.RINGS_TERRAINS
                for terrain in engine_probe.RINGS_TERRAINS
            },
        )
        # Each row paints its blobs in index order, so a blob index always means the same terrain.
        for bg, index, terrain in painted:
            self.assertEqual(index, terrain, "blob index and terrain must line up")

    def test_the_diagonal_is_the_control(self) -> None:
        """Painting a terrain onto its own background is the row's control.

        On the one background already measured it produced a ring of pure background -- no
        transition -- which is what shows the other ten rings are measuring a boundary rather than
        reporting noise. Every row needs one.
        """
        for background in engine_probe.RINGS_TERRAINS:
            self.assertIn(
                f'"bg {background} blob {background} terrain {background} at ',
                self.body,
            )

    def test_blobs_cannot_blend_into_each_other(self) -> None:
        """The measured footprint is one cell beyond the painted run on every side.

        A 3x3 blob therefore influences 5x5, so origins must be more than 5 apart or a transition
        tile could belong to either of two blobs.
        """
        origins = [
            engine_probe.rings_blob_origin(index)
            for index in range(len(engine_probe.RINGS_TERRAINS))
        ]
        self.assertEqual(len(set(origins)), len(origins))
        for left in range(len(origins)):
            for right in range(left + 1, len(origins)):
                (ax, ay), (bx, by) = origins[left], origins[right]
                self.assertGreater(
                    max(abs(ax - bx), abs(ay - by)),
                    5,
                    f"blobs {left} and {right} can blend together",
                )
        for x, y in origins:
            self.assertGreater(x, 0)
            self.assertGreater(y, 0)
            self.assertLess(x + engine_probe.RINGS_BLOB, engine_probe.RINGS_MAP - 1)
            self.assertLess(y + engine_probe.RINGS_BLOB, engine_probe.RINGS_MAP - 1)

    def test_one_capture_per_background(self) -> None:
        """A row whose blobs never painted and a row that rendered flat look identical in the log."""
        shots = re.findall(r'"(zr\d+\.bmp)"screencapture', self.body)
        self.assertEqual(shots, engine_probe.rings_shot_names())
        self.assertEqual(len(set(shots)), len(shots))
        # Each capture comes after that row's blobs and before its save.
        for background in engine_probe.RINGS_TERRAINS:
            last_blob = self.body.rindex(f'"bg {background} blob 10 ')
            shot = self.body.index(f'"{engine_probe.rings_shot_names()[background]}"', last_blob)
            save = self.body.index(f'"{engine_probe.rings_map_names()[background]}"', last_blob)
            self.assertLess(last_blob, shot)
            self.assertLess(shot, save)

    def test_one_save_per_background_in_order(self) -> None:
        saves = re.findall(r'zname"(map/zr[^"]*)"strcpy', self.body)
        self.assertEqual(saves, engine_probe.rings_map_names())
        self.assertEqual(len(set(saves)), len(saves), "two rows share a filename")

    def test_the_log_is_cycled_after_every_save(self) -> None:
        """A fault in a later row must not take the earlier rows' results with it."""
        saves = self.body.count("savescenariomap")
        self.assertEqual(saves, 11 + len(engine_probe.FLAG_ISOLATION_CALLS))
        self.assertEqual(self.body.count('"zprobe.log""abw"file'), saves + 1)

    # --- rider B: which renderer call clears the bit ------------------------------------------

    def test_each_flag_isolation_rung_gets_a_fresh_map(self) -> None:
        """Once something clears the bit it stays cleared.

        Reusing one map would make every call after the first measure the previous call's result,
        which is the same confounding that made the original reading wrong.
        """
        section = self.body[self._at('"flag isolation start"') :]
        self.assertEqual(
            section.count("newmap"),
            len(engine_probe.FLAG_ISOLATION_CALLS),
            "one fresh map per call",
        )
        self.assertEqual(
            section.count("savescenariomap"), len(engine_probe.FLAG_ISOLATION_CALLS)
        )

    def test_the_flag_isolation_control_runs_no_renderer_call(self) -> None:
        """The control reproduces the save that showed the bit SET, so it must stay bare."""
        self.assertEqual(engine_probe.FLAG_ISOLATION_CALLS[0], ("control", ""))
        start = self._at('"flag isolation start"')
        control_save = self.body.index('zname"map/zf0.scn"strcpy', start)
        between = self.body[start:control_save]
        for call in ("rebuild3dmap", "resetvisibility", "rendermap", "refreshdirty"):
            self.assertNotIn(call, between, f"the control must not call {call}")

    def test_each_renderer_call_is_isolated_to_one_rung(self) -> None:
        calls = [call for _, call in engine_probe.FLAG_ISOLATION_CALLS if call]
        self.assertEqual(
            sorted(calls),
            ["rebuild3dmap", "refreshdirty", "rendermap", "resetvisibility"],
            "all four candidates must be tested",
        )
        section = self.body[self._at('"flag isolation start"') :]
        for call in calls:
            # Exactly once each in this section: a rung that ran two calls would isolate neither.
            self.assertEqual(
                len(re.findall(rf"^\t{call}$", section, re.MULTILINE)),
                1,
                f"{call} must appear in exactly one rung",
            )

    # --- rider C: the sprite-type table -------------------------------------------------------

    def test_the_sprite_table_dump_is_last(self) -> None:
        """It is the most speculative part of the run, so it must risk only itself.

        `forall` over a dict is read out of the shipped scripts rather than documented, and `cvs`
        on a name key is the part most likely to misbehave. Everything above it is saved and its
        log flushed before this runs.
        """
        table = self._at('"sprite type table start"')
        self.assertGreater(table, self._at('"flag isolation start"'))
        self.assertGreater(table, self.body.rindex("savescenariomap"))
        self.assertLess(table, self._at('"terrain ring probe done"'))

    def test_the_forall_body_binds_the_value_before_the_key(self) -> None:
        """`forall` pushes key then value, so the value is on top.

        `/zv exch def` binds the value and leaves the key for `/zk exch def`. Getting this backwards
        would log ids as names and names as ids, and the run would look like it worked.
        """
        self.assertIn("terrainsprites{/zv exch def /zk exch def", self.body)
        # The name needs a string buffer to print through; `zkey` is that buffer, not `zname`,
        # which is in use for the save filenames.
        self.assertIn("zk zkey cvs", self.body)
        self.assertIn("/zkey", self.body)

    def test_the_probe_counts_what_it_enumerated(self) -> None:
        """An empty dict and a failed enumeration look identical without a count."""
        self.assertIn("/zcount 0 def", self.body)
        self.assertIn("/zcount zcount 1 add def", self.body)
        self.assertIn('"sprite type table done count "zcount', self.body)

    def test_generated_names_include_this_probes_files_exactly_once(self) -> None:
        names = engine_probe.rings_generated_names()
        self.assertEqual(len(set(names)), len(names))
        union = engine_probe.generated_map_names()
        self.assertEqual(len(set(union)), len(union), "two probes share a map filename")
        for name in names:
            self.assertIn(name, union, "cleanup must cover every file this probe writes")
        # These are outputs, not prerequisites: nothing has to exist before the run.
        for name in names:
            self.assertNotIn(name, engine_probe.generated_map_inputs())


class DirectionConventionTest(unittest.TestCase):
    """The analyser's direction labels and the Rust writer's must mean the same thing.

    `tools/terrain_rings.py` labelled the measured ring `N, S, W, E, NW, NE, SW, SE`, and those
    labels are what the `.til` column convention was derived *against*. If the Rust flipped its
    reading of a column and the analyser kept its labels, every number in the run sheet would
    silently refer to a different cell and the derivation recorded in `docs/map-format.md` would be
    describing an experiment nobody ran.

    So this parses the offsets back out of the Rust rather than trusting that two files agree.
    """

    ROOT = Path(__file__).resolve().parents[1] / "spikes" / "asset-viewer" / "src"

    def test_the_analysers_ring_labels_match_the_rust_offset_table(self) -> None:
        source = (self.ROOT / "map.rs").read_text()
        table = source[source.index("pub const TRANSITION_RING_OFFSETS") :]
        table = table[: table.index("];")]
        rust = [
            (int(dx), int(dy))
            for dx, dy in re.findall(r"direction:\s*\((-?\d+),\s*(-?\d+)\)", table)
        ]
        self.assertEqual(len(rust), 8, "did not find all eight rows in map.rs")
        analyser = [offset for offset, _name in terrain_rings.DIRECTIONS]
        self.assertEqual(
            analyser,
            rust,
            "terrain_rings.py samples the ring in a different order from TRANSITION_RING_OFFSETS",
        )

    def test_the_til_column_convention_matches_the_analysers_labels(self) -> None:
        """`Direction::offset` is the derived `.til` reading; the labels must agree with it.

        The derivation is recorded in `Direction::offset`'s own documentation and was checked
        against `artifacts/engine-probe-captures/terrainrings-20260917`. A mirrored convention
        would negate all eight of these, which is exactly the failure this catches.
        """
        source = (self.ROOT / "tile.rs").read_text()
        body = source[source.index("pub const fn offset(self)") :]
        body = body[: body.index("\n    }")]
        pairs = re.findall(
            r"Direction::(\w+) => \((-?\d+), (-?\d+)\)", body
        )
        rust = {name: (int(dx), int(dy)) for name, dx, dy in pairs}
        self.assertEqual(len(rust), 8, "did not find all eight columns in tile.rs")
        expected = {
            "North": (0, -1),
            "NorthEast": (1, -1),
            "East": (1, 0),
            "SouthEast": (1, 1),
            "South": (0, 1),
            "SouthWest": (-1, 1),
            "West": (-1, 0),
            "NorthWest": (-1, -1),
        }
        self.assertEqual(rust, expected)
        # And the analyser's own labels name the same cells, spelled out rather than derived, so
        # both sides of the derivation are pinned instead of one being defined by the other.
        labels = {name: offset for offset, name in terrain_rings.DIRECTIONS}
        self.assertEqual(labels["N"], rust["North"])
        self.assertEqual(labels["S"], rust["South"])
        self.assertEqual(labels["W"], rust["West"])
        self.assertEqual(labels["E"], rust["East"])
        self.assertEqual(labels["NW"], rust["NorthWest"])
        self.assertEqual(labels["NE"], rust["NorthEast"])
        self.assertEqual(labels["SW"], rust["SouthWest"])
        self.assertEqual(labels["SE"], rust["SouthEast"])


class PaintRefusalReachabilityTest(unittest.TestCase):
    """Every declared `PaintRefusal` must be reachable, and its message must be true.

    Five variants shipped that nothing ever constructed -- `NotATerrainType`, `RegionCoversMap`,
    `PaintedRoadIsRagged`, `RingDependsOnPaintedTerrain`, `DirectionMissingFromTable`. Dead code is
    the lesser problem. `RegionCoversMap`'s `Display` was still telling users the operation was
    refused after it had started succeeding and writing 81 cells, so the enum had become a set of
    promises about behaviour that no longer existed, and the compiler cannot see that because the
    variants are `pub`.

    This is checked from Python because the whole point is that Rust will not complain.
    """

    SOURCES = ("map.rs", "main.rs")

    def setUp(self) -> None:
        root = Path(__file__).resolve().parents[1] / "spikes" / "asset-viewer" / "src"
        self.text = {name: (root / name).read_text() for name in self.SOURCES}
        body = self.text["map.rs"]
        start = body.index("pub enum PaintRefusal {")
        self.enum = body[start : body.index("\n}\n", start)]

    @staticmethod
    def _without_test_module(source: str) -> str:
        """`source` up to its `#[cfg(test)]`.

        A variant only ever constructed by a test is unreachable in production, and a test that
        names it proves only that it is spellable. Verified by mutation: redirecting a live variant's
        real construction site elsewhere leaves the test module still naming it, and without this
        the check passed.
        """
        marker = "#[cfg(test)]"
        return source[: source.index(marker)] if marker in source else source

    def _without_display_arms(self, source: str) -> str:
        """`source` with the `Display for PaintRefusal` block removed.

        **This exclusion is the test.** Every dead variant had a `Self::X =>` arm in `Display` --
        that arm *was* the lie -- so counting those as construction sites makes the check vacuous
        against exactly the five variants that prompted it. Verified by mutation: redirecting a live
        variant's only real construction site elsewhere has to fail, and without this exclusion it
        did not.
        """
        marker = "impl fmt::Display for PaintRefusal {"
        if marker not in source:
            return source
        start = source.index(marker)
        end = source.index("\n}\n", start)
        return source[:start] + source[end:]

    def test_every_declared_refusal_is_constructed_somewhere(self) -> None:
        declared = set(re.findall(r"^    ([A-Z][A-Za-z]*)", self.enum, re.M))
        self.assertTrue(declared, "found no variants; the enum was not located")
        # And the enum declaration itself must not count as a use of its own names.
        constructed = set()
        for source in self.text.values():
            body = self._without_test_module(source)
            body = self._without_display_arms(body).replace(self.enum, "")
            constructed |= set(re.findall(r"PaintRefusal::([A-Z][A-Za-z]*)", body))
            constructed |= set(re.findall(r"Self::([A-Z][A-Za-z]*)", body))
        unreachable = sorted(declared - constructed)
        self.assertEqual(
            unreachable,
            [],
            "these refusals are declared and never constructed, so their Display text is a "
            "promise about behaviour nothing can produce",
        )

    def test_the_deleted_refusals_have_not_come_back(self) -> None:
        """Named individually, because each one described a rule the paint no longer follows.

        A whole-map paint is now a legitimate operation -- it is "fill with correct interior tiles"
        -- and road is refused by the tileset having no boundary tile for it, not by a special case.
        """
        for gone in (
            "RegionCoversMap",
            "PaintedRoadIsRagged",
            "RingDependsOnPaintedTerrain",
            "DirectionMissingFromTable",
            "NotATerrainType",
        ):
            for name, source in self.text.items():
                self.assertNotIn(gone, source, f"{gone} reappeared in {name}")


if __name__ == "__main__":
    unittest.main()
