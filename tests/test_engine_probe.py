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


class CaptureCleanupProvenanceTest(unittest.TestCase):
    """`z*.bmp` cleanup and collection must come from an exact list, never a glob.

    Regression: both `install-engine-probe.sh` (clearing stale output before a run) and
    `restore-game-archives.sh` (collecting output after one) matched `z*.bmp` in the game's
    `English/` directory. That directory is not this project's -- it is the live Steam install --
    and a user's own file starting with `z` (an `English/zReference.bmp`, say) sitting there would
    be silently deleted by the exact same code path that clears this project's own stale captures.
    `generated_map_names()` already gets this right for the loose `map/` directory; capture
    filenames need the identical treatment, which is what `capture_names_for`/`all_capture_names`
    provide.
    """

    def test_every_probes_own_capture_names_are_recovered_exactly(self) -> None:
        for probe, builder in engine_probe.PROBES.items():
            body = builder()
            expected = re.findall(r'"(z[^"]*\.bmp)"screencapture', body) + ["zprobe.log"]
            self.assertEqual(engine_probe.capture_names_for(probe), expected, probe)

    def test_the_union_covers_every_probe_and_has_no_duplicates(self) -> None:
        union = engine_probe.all_capture_names()
        self.assertEqual(len(set(union)), len(union), "a capture name repeats in the union")
        for probe in engine_probe.PROBES:
            for name in engine_probe.capture_names_for(probe):
                self.assertIn(name, union, f"{probe}'s {name!r} is missing from the union")

    def test_the_shell_scripts_use_the_exact_list_never_a_bare_glob(self) -> None:
        """A Python-only assertion cannot see what the scripts that actually run would delete."""
        install = SCRIPTS_DIR.joinpath("install-engine-probe.sh").read_text()
        restore = SCRIPTS_DIR.joinpath("restore-game-archives.sh").read_text()
        self.assertIn("all_capture_names", install)
        self.assertIn("all_capture_names", restore)
        # The actual glob expression the pre-fix scripts expanded against the live game
        # directory. Matched against the shell tokens, not the prose describing why it is gone
        # (this docstring and the scripts' own comments both say `z*.bmp` on purpose).
        for name, text in (("install", install), ("restore", restore)):
            self.assertNotRegex(
                text,
                r'/z\*\.bmp|"\$\{game_dir\}"/z\*',
                f"{name} still globs captures instead of naming them",
            )


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


class UnitAnchorProbeTest(unittest.TestCase):
    """Does the unit draw path apply the same anchor as the terrain-sprite path?

    hotspots.md states its own limit: record 0 was confirmed by placing a unit IMP through the
    *terrain sprite* path, which does not prove the *unit* draw path computes its anchor the same
    way. These assertions hold the generated body to the shape the run sheet promises: a control
    rung and a unit rung sharing one cell, three such cells so three independent anchors are
    recovered rather than one, the shipped sanity control first, and a unit placed by a verbatim
    shipped call site rather than a reconstructed one.

    The 2026-09-19 run is why several of these read the way they do. That run's probe was correct
    and its captures were good; its *expectation* was wrong, because it measured a world-map draw
    against a combat-zoom IMP. The assertions about the subject art below are what stop that
    particular error from being reintroduced silently.
    """

    def setUp(self) -> None:
        self.body = engine_probe.unit_anchor_body()
        self.cells = range(len(engine_probe.UNIT_ANCHOR_SEED_OFFSETS))

    def _at(self, needle: str) -> int:
        index = self.body.find(needle)
        self.assertNotEqual(index, -1, f"{needle!r} missing from the body")
        return index

    def test_each_cell_is_found_once_and_every_rung_on_it_uses_that_one_cell(self) -> None:
        """The 2026-09-16 method: one cell per observation, subjects placed and removed in turn.

        Sharing a cell between the control rung and the unit rung is what makes them comparable
        without solving for the camera twice -- the unknown constants common to both cancel.
        """
        for index, (dx, dy) in enumerate(engine_probe.UNIT_ANCHOR_SEED_OFFSETS):
            self.assertIn(
                f"zax0 {dx} add zay0 {dy} add x_y_to_xy UNITTYPELAND findemptylocation "
                f"/zcell{index} exch def",
                self.body,
            )
            self.assertIn(
                f"zcell{index} xy_to_x_y /zcy{index} exch def /zcx{index} exch def", self.body
            )
            # Both placements on this cell -- the control sprite and the unit -- use this cell's
            # own variables, never a second, independently-derived cell.
            self.assertEqual(self.body.count(f"zcx{index} zcy{index} zt1 addterrainsprite"), 1)
            self.assertIn(f"0{{}}0 zcell{index} zowner add_unit_to_location", self.body)
            self.assertIn(f"zcx{index} zcy{index} armyat /zarmy{index} exch def", self.body)

    def test_a_cell_findemptylocation_could_not_supply_is_refused_not_used(self) -> None:
        """`findemptylocation` returns -1 when it finds nothing.

        Running the rungs on -1 would place on, and later delete from, whatever cell -1 decomposes
        to. Every cell's rungs sit behind a validity gate, and a refusal is logged rather than
        passed over in silence.
        """
        for index in self.cells:
            gate = f"zcell{index} -1 ne"
            self.assertIn(gate, self.body, gate)
            self.assertLess(self._at(gate), self._at(f"zcx{index} zcy{index} zt1 addterrainsprite"))
            self.assertIn(f"cell{index} SKIPPED -- findemptylocation returned -1", self.body)

    def test_the_seed_is_never_the_armys_own_occupied_cell(self) -> None:
        """Regression: seeding `findemptylocation` from `zaloc` risks handing back `zaloc` itself.

        If `findemptylocation` can return its own seed when it judges that cell acceptable,
        seeding from the army's own occupied cell risks a target cell equal to the player's own
        starting army's cell, and the unit rung would then find and delete that army instead of
        the one it placed. This cannot be verified without the engine (whether `findemptylocation`
        can hand back its seed is GameScript semantics this project has no way to observe
        offline), so the test asserts the generator-level fact that is actually checkable: every
        seed is an OFFSET cell, matching the two sibling probes, never the bare `zaloc`.
        """
        self.assertNotIn("zaloc UNITTYPELAND findemptylocation", self.body)
        for offset in engine_probe.UNIT_ANCHOR_SEED_OFFSETS:
            self.assertNotEqual(offset, (0, 0))
        self.assertEqual(
            len(set(engine_probe.UNIT_ANCHOR_SEED_OFFSETS)),
            len(engine_probe.UNIT_ANCHOR_SEED_OFFSETS),
            "two seeds are identical, so two observations cannot be independent",
        )

    def test_rung_order_is_control_then_same_cell_control_then_the_unit(self) -> None:
        """A failure has to name its own rung, so the shipped sanity control comes first."""
        rung0 = self._at("zt0 addterrainsprite")
        for index in self.cells:
            control = self._at(f"zcx{index} zcy{index} zt1 addterrainsprite")
            unit = self._at(f"0{{}}0 zcell{index} zowner add_unit_to_location")
            self.assertLess(rung0, control)
            self.assertLess(control, unit)

    def test_rung0_is_the_ladder_runs_own_shipped_control(self) -> None:
        self.assertIn(engine_probe.UNIT_ANCHOR_CONTROL_TYPE, self.body)
        self.assertIn(f"{engine_probe.UNIT_ANCHOR_CONTROL_TYPE} /zt0 exch def", self.body)

    def test_the_control_art_is_the_one_whose_terrain_path_result_is_already_measured(self) -> None:
        """The control rung's job is to recover the anchor, and only that.

        It places `licr2a.imp` through the terrain-sprite path because that exact combination is
        the measurement made on 2026-09-16 and reproduced to the pixel on 2026-09-19 -- 30x122 at
        frame 0, record 0 `(1,-25)`. Its expected result is therefore known in advance, which is
        what makes it a control rather than a second unknown.
        """
        self.assertIn(
            f'["{engine_probe.UNIT_ANCHOR_CONTROL_IMP}"]cvx addterrainspritetype', self.body
        )
        self.assertEqual(engine_probe.UNIT_ANCHOR_CONTROL_FRAME, 0)
        self.assertEqual(engine_probe.UNIT_ANCHOR_CONTROL_FRAME_SIZE, (30, 122))
        self.assertEqual(engine_probe.UNIT_ANCHOR_CONTROL_FRAME_PLACEMENT, (1, -25))

    def test_the_subject_art_is_the_world_map_zoom_variant_of_the_subject_unit(self) -> None:
        """The 2026-09-19 error, encoded so it cannot come back.

        `gs\\imps.gs`'s `unittype_imp_filename` appends `unit_zoom_letter` of the screen mode the
        engine pushes: `A` for COMBAT_SCREEN and LOCATION_SCREEN, `B` for SCROLLINGMAP_SCREEN,
        REGION_SCREEN and WORLD_SCREEN. A unit standing on the world map is therefore drawn from
        `...b.imp`, and the previous version of this probe expected to recognise it against the
        `...a.imp` frame table. The subject art must be the B variant of the subject unit's own
        name, and it must never be registered as a terrain sprite type -- it is measured against,
        not placed.
        """
        symbol = engine_probe.UNIT_ANCHOR_TYPE_SYMBOL
        self.assertEqual(engine_probe.UNIT_ANCHOR_SUBJECT_IMP, f"units/imp/{symbol}b.imp")
        self.assertNotIn(engine_probe.UNIT_ANCHOR_SUBJECT_IMP, self.body)
        # The control art is a different file on purpose; asserting they are the same was the
        # premise that made the last run uninterpretable.
        self.assertNotEqual(
            engine_probe.UNIT_ANCHOR_SUBJECT_IMP, engine_probe.UNIT_ANCHOR_CONTROL_IMP
        )

    def test_unit_call_is_copied_from_the_shipped_call_site_not_reconstructed(self) -> None:
        """gs\\PLAYER5.gs:430 verbatim, `start_loc`/`2` swapped for this probe's own cell/owner.

        Operand order for `add_unit_to_location` is TYPE STR ARTLIST NAME LOC OWNER -- the reverse
        of the pop order in its own definition (`/owner /loc /this_name /this_artlist /this_str
        /this_type`) -- and this asserts the exact shipped token shape, not a paraphrase of it.
        """
        for index in self.cells:
            self.assertIn(
                f"unittypedict begin /{engine_probe.UNIT_ANCHOR_TYPE_SYMBOL} end 0{{}}0 "
                f"zcell{index} zowner add_unit_to_location",
                self.body,
            )

    def test_never_forces_or_assumes_a_facing(self) -> None:
        """Facing is read for information only; identification comes from the frame's own size.

        No shipped call site sets ARMY_FACING, so constructing one would be exactly the
        reconstruction-from-resemblance the project's rules forbid.
        """
        self.assertNotIn("ARMY_FACING", self.body[: self._at("ARMY_FACING getarmydata")])
        self.assertNotIn("setarmydata", self.body)
        for index in self.cells:
            self.assertIn(
                f"zarmy{index} ARMY_FACING getarmydata /zfacing{index} exch def", self.body
            )

    def test_army_is_found_by_location_never_by_a_fabricated_return_value(self) -> None:
        """`add_unit_to_location`'s body ends on a boolean branch; it pushes nothing back."""
        after_call = self.body[self._at("add_unit_to_location") + len("add_unit_to_location") :]
        immediate = after_call.split("armyat", 1)[0]
        self.assertNotIn("exch def", immediate, "treats add_unit_to_location as if it returned")
        for index in self.cells:
            self.assertIn(f"zcx{index} zcy{index} armyat", self.body)

    def test_cleanup_is_the_exact_army_id_never_a_type_or_location_sweep(self) -> None:
        for index in self.cells:
            self.assertIn(f"zarmy{index} deletearmynow", self.body)
            # An army has no sprite type, so the terrain-sprite sweep idiom must never appear
            # between placing a unit and deleting it.
            start = self._at(f"0{{}}0 zcell{index} zowner add_unit_to_location")
            stop = self.body.index(f"zarmy{index} deletearmynow", start)
            self.assertNotIn("getterrainspritetype", self.body[start:stop])

    def test_army_delete_is_gated_on_a_valid_id_and_a_matching_location(self) -> None:
        """Regression: `deletearmynow` once ran unconditionally, on whatever `armyat` returned.

        The reported location was computed and logged but never checked, so a stale or foreign
        army id -- or a cell that silently was not the one this probe placed into -- would still
        be deleted. Whether `armyat` can ever hand back a foreign army, and what `deletearmynow`
        does with an invalid id, are GameScript semantics no offline test can settle; what this
        test can and does check is the generator-level fact: the delete for each observation sits
        behind a boolean gate testing BOTH the id and the location, never bare.
        """
        for index in self.cells:
            gate = f"zarmy{index} -1 ne zaloc{index} zcell{index} eq and"
            self.assertIn(gate, self.body, gate)
            gate_at = self._at(gate)
            delete_at = self.body.index(f"zarmy{index} deletearmynow", gate_at)
            between = self.body[gate_at + len(gate) : delete_at]
            # The delete must be the FIRST thing the true branch does -- not merely present
            # somewhere after the gate, which an unconditional delete placed later would satisfy.
            self.assertNotIn("deletearmynow", between, str(index))
            self.assertIn("{", between, f"cell{index}: delete is not inside a conditional branch")
            self.assertIn(f"cell{index} cleanup REFUSED", self.body)

    def test_rung0_checks_for_an_existing_orchard_before_placing_or_destroying(self) -> None:
        """Regression: rung 0 minted no distinguishing state, so its cleanup swept by type AND
        cell alone -- indistinguishable from a shipped orchard the map generator already put
        there. `findemptylocation UNITTYPELAND` only rules out a land UNIT standing on a cell,
        never a decorative terrain sprite, so an existing orchard was not ruled out either.

        This cannot be exercised without a real map (the fixture here is the generated text, not
        a map, exactly the blind spot called out for the type-and-location assertion below), so
        what this test checks is the generator-level fact: a presence check runs BEFORE the
        placement, and the placement/cleanup sequence is reachable only when it comes back false.
        """
        presence_check = self._at("/zorchard_present false def")
        placement = self._at("zcx0 zcy0 zt0 addterrainsprite")
        self.assertLess(presence_check, placement)
        self.assertIn(
            "{dup getterrainspritetype zt0 eq"
            "{dup getterrainspritelocation zcell0 eq"
            "{pop /zorchard_present true def}{pop}ifelse}"
            "{pop}ifelse}enumterrainsprites",
            self.body,
        )
        branch = self._at("zorchard_present")
        self.assertLess(branch, placement)
        self.assertIn("rung0 SKIPPED", self.body)
        skip_at = self._at("rung0 SKIPPED")
        self.assertLess(branch, skip_at)
        self.assertLess(skip_at, placement, "the skip branch must precede the placement branch")

    def test_rung0_and_the_control_rung_are_swept_by_the_right_scope_each(self) -> None:
        """Rung 0 reuses the shipped orchard type; a type-only sweep on it would delete orchards.

        NOTE on what this cannot catch: the fixture here is the generated script text, not a map,
        so this only proves the destroy call is scoped by BOTH `getterrainspritetype zt0 eq` and
        `getterrainspritelocation zcell0 eq` in the source. It cannot show that scoping actually
        spares a real shipped orchard at runtime -- only an attended run against a real map can.
        """
        rung0_zone = self.body[self._at("zcx0 zcy0 zt0 addterrainsprite") : self._at("zt1 add")]
        self.assertIn("getterrainspritelocation zcell0 eq", rung0_zone)
        self.assertNotIn(
            "{dup getterrainspritetype zt0 eq{destroyterrainsprite}{pop}ifelse}enumterrainsprites",
            rung0_zone,
        )
        # The control type was minted this keypress, so it gets only the type-only sweep -- the
        # type-and-location sweep would be redundant, since nothing else on the map can carry an
        # id this keypress just registered. One sweep per cell.
        self.assertEqual(
            self.body.count(
                "{dup getterrainspritetype zt1 eq"
                "{destroyterrainsprite}{pop}ifelse}enumterrainsprites"
            ),
            len(engine_probe.UNIT_ANCHOR_SEED_OFFSETS),
        )

    def test_every_capture_is_uniquely_named_and_this_probes_own(self) -> None:
        names = re.findall(r'"(z[^"]*\.bmp)"screencapture', self.body)
        # plate, the orchard control, then three captures per cell: control, unit, post-cleanup.
        self.assertEqual(len(names), 2 + 3 * len(engine_probe.UNIT_ANCHOR_SEED_OFFSETS))
        self.assertEqual(names[0], "zu0.bmp")
        self.assertEqual(len(names), len(set(names)))
        self.assertEqual(names, sorted(names, key=lambda n: int(n[2:-4])))

    def test_plate_precedes_every_rung_and_each_cell_ends_on_a_post_cleanup_capture(self) -> None:
        names = re.findall(r'"(z[^"]*\.bmp)"screencapture', self.body)
        plate = self._at('"zu0.bmp"screencapture')
        self.assertLess(plate, self._at("zt0 addterrainsprite"))
        for index in self.cells:
            cleanup = self._at(f"zarmy{index} deletearmynow")
            final_shot = self._at(f'"{names[1 + 3 * (index + 1)]}"screencapture')
            self.assertLess(cleanup, final_shot)

    def test_three_cells_give_three_independently_anchored_observations(self) -> None:
        """A single placement can only tell zero residual from non-zero; it cannot tell a constant
        non-zero residual from one that varies by facing, because facing is never forced (see
        `test_never_forces_or_assumes_a_facing`) and one draw samples exactly one facing.

        The 2026-09-19 run placed the unit twice on ONE cell for exactly this reason and got the
        same facing both times, pixel-identical captures included -- one facing sampled twice.
        Different cells are the only lever this probe has, and they buy something a repeat cannot:
        each observation gets its own control rung and therefore its own recovered anchor, so a
        residual that is really a property of one cell's projection cannot masquerade as a
        property of the unit draw path. It still cannot FORCE the facings to differ; nothing
        offline can assert that, and the run sheet says so.
        """
        self.assertGreaterEqual(len(engine_probe.UNIT_ANCHOR_SEED_OFFSETS), 3)
        self.assertEqual(
            self.body.count("add_unit_to_location"), len(engine_probe.UNIT_ANCHOR_SEED_OFFSETS)
        )
        # Each observation carries its own control rung, so each has its own anchor.
        self.assertEqual(
            self.body.count("zt1 addterrainsprite"), len(engine_probe.UNIT_ANCHOR_SEED_OFFSETS)
        )
        facings = [self._at(f"/zfacing{i} exch def") for i in self.cells]
        self.assertEqual(facings, sorted(facings))

    def test_every_candidate_frame_is_identifiable_from_the_capture_in_both_orientations(
        self,
    ) -> None:
        """Identification must not depend on assuming the engine did not mirror the frame.

        The engine mirrors: on 2026-09-19 the Unicorn was stored facing right and drawn facing
        left, and its detached two-pixel bottom tail landed at sprite-relative columns 1-3 rather
        than 43-45. So a candidate frame is only identifiable if its predicted top-left and its
        silhouette size, taken together, are unique across every frame in the table in BOTH
        orientations. For `licr2b` frame 33 they were not -- three MOVE frames mirrored predicted
        the same top-left, and only the width separated them.

        The predicted top-left is `anchor + placement - (w>>1, h>>1)`, so for a fixed anchor the
        distinguishing quantity is `placement - (w>>1, h>>1)`, with `placement.x` negated in the
        mirrored case.
        """
        signatures = []
        for width, height, x, y in engine_probe.UNIT_ANCHOR_SUBJECT_FRAMES.values():
            for placement_x in (x, -x):
                signatures.append(
                    (placement_x - (width >> 1), y - (height >> 1), width, height)
                )
        self.assertEqual(
            len(signatures),
            len(set(signatures)),
            "two candidate frames are indistinguishable in the capture",
        )

    def test_every_candidate_frames_record0_x_is_far_enough_from_zero_to_show_a_mirror(
        self,
    ) -> None:
        """Record-0 x of 0 makes `+placement.x` and `-placement.x` predict the identical pixel.

        That is precisely why the 2026-09-19 run could not settle whether the rule's placement x
        is negated under mirroring: the frame that drew had record-0 x = 0. Every candidate frame
        here is far enough from zero that the two predictions differ by at least 8 pixels.
        """
        for frame in engine_probe.UNIT_ANCHOR_SUBJECT_STAND_FRAMES:
            _width, _height, x, _y = engine_probe.UNIT_ANCHOR_SUBJECT_FRAMES[frame]
            self.assertGreaterEqual(abs(x), 4, f"frame {frame} cannot discriminate a mirror")
        # Frame 0 is the MOVE fallback, deliberately not held to this: it is in the table so an
        # unexpected MOVE draw is still identifiable, not because it can settle the mirror sign.
        self.assertNotIn(0, engine_probe.UNIT_ANCHOR_SUBJECT_STAND_FRAMES)
        self.assertEqual(
            set(engine_probe.UNIT_ANCHOR_SUBJECT_FRAMES)
            - set(engine_probe.UNIT_ANCHOR_SUBJECT_STAND_FRAMES),
            {0},
        )

    def test_frame_table_covers_the_move_frame_and_all_five_stand_facings(self) -> None:
        self.assertEqual(set(engine_probe.UNIT_ANCHOR_SUBJECT_FRAMES), {0, 30, 31, 32, 33, 34})
        sizes = [(w, h) for w, h, _, _ in engine_probe.UNIT_ANCHOR_SUBJECT_FRAMES.values()]
        self.assertEqual(len(sizes), len(set(sizes)), "two candidate frames share a silhouette")

    def test_the_flag_offset_is_the_shipped_one(self) -> None:
        """A world-map army is a composite, and the flag is a second test of the same anchor.

        `gs\\PLAYER5.gs`'s `setupplayergraphics` attaches the player's flag with
        `"iface/liflagb.imp" 0 -40 setplayerflagimp`. The 2026-09-19 captures carry that flag as a
        detached 12x21 component, and it is solved by the same anchor the body is -- so the offset
        has to be recorded here rather than rediscovered each time.
        """
        self.assertEqual(engine_probe.UNIT_ANCHOR_FLAG_OFFSET, (0, -40))
        self.assertIn("{faith}", engine_probe.UNIT_ANCHOR_FLAG_IMP_TEMPLATE)
        self.assertTrue(
            engine_probe.UNIT_ANCHOR_FLAG_IMP_TEMPLATE.endswith("flagb.imp"),
            "the world-map flag is the b-zoom variant, like the unit body",
        )

    def test_logs_carry_enough_to_solve_the_rule_backwards_offline(self) -> None:
        """Every quantity the offline analysis needs is in the log, not just the captures."""
        needles = [
            '"army loc "zaloc" cell "zax0" "zay0" owner "zowner',
            '"owner faith "zowner getplayerfaith',
            '"control terrain type "zt1',
            '"rung0 orchard type "zt0" done"',
        ]
        for index in self.cells:
            needles += [
                f'"cell{index} target "zcell{index}" "zcx{index}" "zcy{index}',
                f'"cell{index} control licr2a-as-terrain done"',
                f'"cell{index} unit army "zarmy{index}" at "zaloc{index}'
                f'" expected "zcell{index}" facing "zfacing{index}',
                f'"cell{index} cleanup done"',
            ]
        for needle in needles:
            self.assertIn(needle, self.body, needle)


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


class UnitIndexProbeTest(unittest.TestCase):
    """The `unitindex` probe: does a unit type above index 154 register and draw?

    Its offline half is already settled -- the engine has no unit-type cap and nothing downstream
    narrows the index (`docs/new-units.md`). What this probe adds is the one thing no disassembly
    can supply: a unit type existing AT RUNTIME above 154.

    The assertions below are the ones whose absence would waste a sitting. Two matter most. The
    subject definition must sit inside `unittypedict begin ... end`, because `units\\easyunit.gs`'s
    machinery was loaded into that dict at boot and `add_unit_to_location` looks the symbol up
    there -- define it anywhere else and the probe fails for a reason that looks exactly like the
    answer being "no". And the control rung must use a SHIPPED unit, so that "declaring a type at
    hotkey time does not work" can never be mistaken for "index 155 does not work".
    """

    def setUp(self) -> None:
        self.body = engine_probe.unit_index_body()

    def _at(self, needle: str) -> int:
        index = self.body.find(needle)
        self.assertNotEqual(index, -1, f"{needle!r} missing from the body")
        return index

    def test_the_subject_is_defined_inside_unittypedict(self) -> None:
        """`begin_unit_definition` resolves only from inside `unittypedict`, and so must the key.

        **Observed in the corpus:** `gs\\unittype.gs` runs every `units/*.gs` inside
        `soundfxdict begin unittypedict begin ... end end`, so both the definition machinery and
        every unit symbol live in `unittypedict`. `add_unit_to_location`'s body does
        `unittypedict this_type get`, so a symbol defined anywhere else is invisible to it.
        """
        opened = self._at("unittypedict begin\nbegin_unit_definition".replace("\n", "\n\t\t\t"))
        defined = self._at(
            f"end_unit_definition /{engine_probe.UNIT_INDEX_SUBJECT_SYMBOL} exch def"
        )
        self.assertLess(opened, defined, "the definition must sit inside unittypedict begin")
        # ... and the dict is closed again before anything else runs.
        self.assertLess(defined, self._at("/zafter numunittypes def"))

    def test_the_control_rung_places_a_shipped_unit(self) -> None:
        """The control exists to separate two failures that look identical on screen.

        If declaring a unit type at hotkey time does not work, the subject draws nothing. If the
        placement path itself is broken in this session, the subject also draws nothing. The
        control -- a type that existed at boot, through the same `add_unit_to_location` -- tells
        them apart, so it must NOT be the type this probe defines.
        """
        self.assertNotEqual(
            engine_probe.UNIT_INDEX_CONTROL_SYMBOL,
            engine_probe.UNIT_INDEX_SUBJECT_SYMBOL,
        )
        control = self._at(
            f"unittypedict begin /{engine_probe.UNIT_INDEX_CONTROL_SYMBOL} end "
            f"0{{}}0 zcell0 zowner add_unit_to_location"
        )
        subject = self._at(
            f"unittypedict begin /{engine_probe.UNIT_INDEX_SUBJECT_SYMBOL} end "
            f"0{{}}0 zcell1 zowner add_unit_to_location"
        )
        # The control runs FIRST: a control observed after the subject cannot rescue it.
        self.assertLess(control, subject)
        # And the subject is not even defined until after the control has been placed.
        self.assertLess(control, self._at("begin_unit_definition"))

    def test_the_baseline_is_logged_before_anything_is_defined(self) -> None:
        """Every count in this run is read against `zbase`, so it must precede the definition.

        A baseline captured afterwards would already include the probe's own unit and the run
        would silently measure nothing.
        """
        self.assertLess(
            self._at("/zbase numunittypes def"), self._at("begin_unit_definition")
        )
        self.assertIn(
            f'expected "{engine_probe.UNIT_INDEX_EXPECTED_BASELINE}', self.body
        )

    def test_the_subject_is_not_placed_unless_the_count_actually_moved(self) -> None:
        """A silent append failure must not be captured and read as a negative result.

        `unittype` returns -1 from the append helper when `used == capacity`. If the count did not
        move, the type does not exist, and placing its symbol would photograph an empty cell that
        looks exactly like "index 155 does not draw".
        """
        gate = self._at("zafter zbase gt")
        self.assertLess(
            gate,
            self._at(
                f"unittypedict begin /{engine_probe.UNIT_INDEX_SUBJECT_SYMBOL} end"
            ),
        )
        self.assertIn("rung2 REFUSED -- numunittypes did not move", self.body)

    def test_every_placed_army_is_cleaned_up_behind_an_id_and_location_gate(self) -> None:
        """Never an unconditional `deletearmynow`, and never a sweep.

        The unit TYPE cannot be cleaned up -- there is no `deleteunittype` -- but every ARMY this
        probe places must be, by exact id, gated on that id being valid AND its reported location
        matching the cell this probe placed into.
        """
        for index in range(len(engine_probe.UNIT_INDEX_SEED_OFFSETS)):
            gate = f"zarmy{index} -1 ne zaloc{index} zcell{index} eq and"
            self.assertIn(gate, self.body, gate)
            self.assertLess(self._at(gate), self._at(f"zarmy{index} deletearmynow"))
            self.assertIn("cleanup REFUSED", self.body)
        # Exactly as many deletes as placements -- no stray, ungated cleanup.
        self.assertEqual(
            self.body.count("deletearmynow"),
            len(engine_probe.UNIT_INDEX_SEED_OFFSETS),
        )

    def test_the_subject_reuses_shipped_art_so_no_archive_gains_a_member(self) -> None:
        """The whole point of the cheap design: this run touches `gs\\hotkey.gs` and nothing else.

        `impfile_proc` merely NAMES an art file. Pointing it at a shipped one keeps `imp.mpq` and
        `pic.mpq` out of the run entirely, so a failure cannot be an archive-acceptance failure
        wearing a unit-index costume.
        """
        self.assertIn(
            f'/impfile_proc{{"{engine_probe.UNIT_INDEX_CONTROL_SYMBOL}"'
            "unittype_imp_filename}def",
            self.body,
        )

    def test_the_subject_field_set_matches_the_shipped_unit_it_was_copied_from(self) -> None:
        """`unitdict`'s key order IS the engine's field enum, so fields may not be invented.

        `end_unit_definition` writes each key at the index `unitdictxref` gives it. A field this
        project made up would be written into whatever slot it happened to land in. This asserts
        the field NAMES are a subset of those the shipped Elephant declares -- the file the set was
        copied from -- rather than checking the values, which are ours to choose.
        """
        shipped = {
            "name", "code", "flags", "race", "faith", "attack", "armor", "strength",
            "dexterity", "wisdom", "hit_points", "mps", "sight_radius",
            "stealth_noise_factor", "attack_recovery_ticks", "get_hit_recovery_ticks",
            "frames_per_grid", "health_bar_x", "health_bar_y", "morale_bar_x",
            "morale_bar_y", "impfile_proc", "level_procedure",
        }
        declared = {
            re.match(r"/(\w+)", field).group(1)
            for field in engine_probe.UNIT_INDEX_SUBJECT_FIELDS
        }
        self.assertEqual(declared - shipped, set(), "invented field names")

    def test_the_subject_code_is_one_no_shipped_unit_uses(self) -> None:
        """Keeps this run clear of the separately-open duplicate-(faith, code) question.

        **Observed in the corpus:** of the 37 values in `gs\\champion.gs`'s closed
        `unit_code_strings` enum, `WMT` is the one used by no shipped unit in any faith.
        """
        self.assertIn("/code WMT def", engine_probe.UNIT_INDEX_SUBJECT_FIELDS)
