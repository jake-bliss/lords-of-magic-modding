"""The projection checked against the measurements it was derived from.

Every number here is an observation from the 2026-09-17 flatground run, logged by the probe or read
off a capture. If the module drifts from what the engine did, these fail.
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

import map_projection as mp  # noqa: E402

# (phase, cell, output1, output2 with z=0, output3, measured drawn top).
# The control at (35,41) stands on ground that is flat in every phase, so its rows isolate the
# camera: its elevation never changes while its output 2 moves by 40 pixels.
OBSERVED = [
    ("flat", (26, 32), 1027.2, 999.2, 49.3532, 26),
    ("flat", (29, 32), 1070.4, 1042.4, 151.177, 69),
    ("flat", (35, 32), 1156.8, 1128.8, 354.823, 155),
    ("flat", (38, 32), 1200.0, 1172.0, 456.647, 198),
    ("flat", (41, 32), 1243.2, 1215.2, 558.47, 242),
    ("flat", (35, 41), 1286.4, 1258.4, 49.3532, 285),
    ("plateau", (35, 41), 1286.4, 1298.4, 49.3532, 325),
    ("spike", (35, 41), 1286.4, 1278.4, 49.3532, 305),
]

SPIKE_NEIGHBOURHOOD = {
    (-1, -1): 1.0, (0, -1): 1.0, (1, -1): 1.0,
    (-1, 0): 1.0, (0, 0): 2.0, (1, 0): 1.5,
    (-1, 1): 1.0, (0, 1): 1.0, (1, 1): 1.0,
}
PLATEAU_NEIGHBOURHOOD = {offset: 2.0 for offset in SPIKE_NEIGHBOURHOOD}
FLAT_NEIGHBOURHOOD = {offset: 0.0 for offset in SPIKE_NEIGHBOURHOOD}


class ScreenYTest(unittest.TestCase):
    def test_drawn_top_is_output2_plus_a_fixed_offset(self) -> None:
        """Slope one, to under a pixel, on every flat-ground observation."""
        for phase, cell, _, output2, _, top in OBSERVED:
            self.assertAlmostEqual(mp.screen_y(output2), top, delta=1.0,
                                   msg=f"{phase} {cell}")

    def test_the_offset_survives_moving_the_camera(self) -> None:
        """The control's ground never changes, yet its output 2 moves 40 pixels between phases.

        Its drawn top moves by exactly the same 40, which is what proves output 2 already contains
        the camera scroll -- and why the '-80' an earlier write-up recorded as a constant was that
        run's scroll.
        """
        control = {phase: (output2, top) for phase, cell, _, output2, _, top in OBSERVED
                   if cell == (35, 41)}
        self.assertEqual(len(control), 3)
        offsets = {phase: top - output2 for phase, (output2, top) in control.items()}
        self.assertAlmostEqual(max(offsets.values()) - min(offsets.values()), 0.0, delta=0.5)
        self.assertAlmostEqual(control["plateau"][1] - control["flat"][1],
                               control["plateau"][0] - control["flat"][0], delta=0.5)

    def test_output3_is_screen_x_and_is_independent_of_elevation(self) -> None:
        for phase, cell, _, _, output3, _ in OBSERVED:
            x, y = cell
            self.assertAlmostEqual(
                mp.map2screen(x, y, 0.0, 0.0, 0.0, 49.3532 - mp.SCREEN_X_PER_STEP * (26 - 32))[2],
                output3, delta=0.01, msg=f"{phase} {cell}")

    def test_output1_and_output3_ignore_z(self) -> None:
        a = mp.map2screen(30, 40, 0.0, 10.0, 80.0, 5.0)
        b = mp.map2screen(30, 40, 2.0, 10.0, 80.0, 5.0)
        self.assertEqual(a[0], b[0])
        self.assertEqual(a[2], b[2])

    def test_the_z_coefficient_is_the_measured_one(self) -> None:
        """Against the engine's own numbers, not against the constant itself.

        Each pair is one logged `map2screen` call made twice on the same cell in the same frame --
        once with z = 0 and once with z = the cell's elevation of 2.0. Asserting
        `a[1] - b[1] == 2 * PIXELS_PER_ELEVATION` would have passed for any value of the constant.
        """
        logged_pairs = [
            (1039.2, 998.471), (1082.4, 1041.67), (1125.6, 1084.87),
            (1168.8, 1128.07), (1212.0, 1171.27), (1255.2, 1214.47),
        ]
        for at_zero, at_two in logged_pairs:
            self.assertAlmostEqual((at_zero - at_two) / 2.0, mp.PIXELS_PER_ELEVATION, delta=0.01)


class EffectiveElevationTest(unittest.TestCase):
    def test_a_uniform_neighbourhood_gives_back_the_cells_own_elevation(self) -> None:
        """The half of the result that is not in doubt."""
        self.assertEqual(mp.effective_elevation(PLATEAU_NEIGHBOURHOOD), 2.0)
        self.assertEqual(mp.effective_elevation(FLAT_NEIGHBOURHOOD), 0.0)
        self.assertTrue(mp.is_uniform(PLATEAU_NEIGHBOURHOOD))
        self.assertTrue(mp.is_uniform(FLAT_NEIGHBOURHOOD))

    def test_a_spike_does_not(self) -> None:
        """Same cell elevation, different neighbourhood, different drawn height.

        This is the whole experiment. The measured value was 1.395 (28.4 pixels at 20.3625 per
        unit); the model returns 1.375, which is three of the five sprites exactly and the other
        two within a pixel.
        """
        self.assertFalse(mp.is_uniform(SPIKE_NEIGHBOURHOOD))
        self.assertEqual(SPIKE_NEIGHBOURHOOD[(0, 0)], 2.0)
        self.assertAlmostEqual(mp.effective_elevation(SPIKE_NEIGHBOURHOOD), 1.395, delta=0.05)
        self.assertLess(mp.effective_elevation(SPIKE_NEIGHBOURHOOD), 2.0)

    def test_the_corner_mean_is_excluded_by_the_measurement(self) -> None:
        """The rival model, kept because ruling it out is why `max` is there at all.

        The corner mean predicts 26.7 pixels of displacement. The smallest of the five measurements
        was 28. One rival excluded is not the same as a kernel established -- see the docstring.
        """
        corners = mp.corner_heights(SPIKE_NEIGHBOURHOOD)
        mean = sum(corners) / 4.0
        self.assertAlmostEqual(mean, 1.3125, places=4)
        self.assertLess(mean * mp.PIXELS_PER_ELEVATION, 28.0)
        self.assertAlmostEqual(max(corners) * mp.PIXELS_PER_ELEVATION, 28.0, delta=0.5)

    def test_the_measured_spike_displacement_matches_the_model(self) -> None:
        """Reconstructing the run: flat top, minus the camera, against the spike top."""
        measured_pixels = [29, 28, 28, 28, 29]  # places 0, 1, 3, 4, 5
        predicted = (2.0 - mp.effective_elevation(SPIKE_NEIGHBOURHOOD)) * mp.PIXELS_PER_ELEVATION
        predicted = 2.0 * mp.PIXELS_PER_ELEVATION - predicted
        for observed in measured_pixels:
            self.assertAlmostEqual(predicted, observed, delta=1.1)


if __name__ == "__main__":
    unittest.main()
