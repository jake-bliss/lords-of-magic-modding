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


if __name__ == "__main__":
    unittest.main()
