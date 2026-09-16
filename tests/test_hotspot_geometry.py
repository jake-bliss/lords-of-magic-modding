import unittest

from tools.hotspot_geometry import (
    Frame,
    axis_fit,
    missile_target_offset,
    parse_frames,
)


DESCRIBE_OUTPUT = "\n".join(
    [
        "record\tindex\towner\tlabels-or-flags\tmetadata-or-size\tfirst\tcount\tplacement",
        "sequence\t0\t-\tMOVE\t00800a01ff000000000000\tfacing:0;frame:0\tfacing:5;frame:30\t-",
        "frame\t0\tsequence:0;facing:0;offset:0\t0x00;direct\t24x67\t-\t2\t0:0:-24|7:0:-35",
        "frame\t1\tsequence:0;facing:0;offset:1\t0x00;direct\t22x66\t-\t2\t0:1:-27|7:0:-34",
        # A frame with no hotspot array reports its origin instead; it says nothing about anchors.
        "frame\t2\tsequence:0;facing:0;offset:2\t0x00;direct\t20x60\t-\t0\torigin:(3,-4)",
        "frame\t3\tsequence:0;facing:0;offset:3\t0x08;duplicate\t0x0\t-\t0\t-",
    ]
)


class ParseFramesTest(unittest.TestCase):
    def test_reads_dimensions_and_hotspot_records(self) -> None:
        frames = parse_frames(DESCRIBE_OUTPUT)
        self.assertEqual(len(frames), 2)
        self.assertEqual((frames[0].width, frames[0].height), (24, 67))
        self.assertEqual(frames[0].hotspots[0], (0, -24))
        self.assertEqual(frames[0].hotspots[7], (0, -35))

    def test_skips_frames_whose_placement_is_an_origin_rather_than_hotspots(self) -> None:
        # Those frames carry no hotspot array, so including them would dilute the fit.
        frames = parse_frames(DESCRIBE_OUTPUT)
        self.assertTrue(all(frame.hotspots for frame in frames))
        self.assertNotIn(20, [frame.width for frame in frames])


class AxisFitTest(unittest.TestCase):
    def test_a_hotspot_that_is_a_function_of_size_leaves_no_residual(self) -> None:
        # Exactly half the height, which is what "derivable from frame geometry" would look like.
        frames = [Frame(10, height, {0: (0, -height // 2)}) for height in (20, 40, 60, 80)]
        fit = axis_fit(frames, axis=1)
        self.assertAlmostEqual(fit["slope"], -0.5, places=6)
        self.assertAlmostEqual(fit["residual_stdev"], 0.0, places=6)

    def test_a_hotspot_independent_of_size_keeps_its_variance(self) -> None:
        frames = [
            Frame(10, 20, {0: (0, -5)}),
            Frame(10, 40, {0: (0, -30)}),
            Frame(10, 60, {0: (0, -7)}),
            Frame(10, 80, {0: (0, -28)}),
        ]
        fit = axis_fit(frames, axis=1)
        # The fit cannot explain much, so most of the spread survives it.
        self.assertGreater(fit["residual_stdev"], 0.7 * fit["raw_stdev"])

    def test_reports_the_frame_count_it_used(self) -> None:
        frames = [Frame(10, 20, {0: (1, -5)}), Frame(12, 24, {0: (2, -6)})]
        self.assertEqual(axis_fit(frames, axis=0)["frames"], 2)


class MissileTargetOffsetTest(unittest.TestCase):
    def test_measures_the_offset_from_the_cursor_hotspot(self) -> None:
        frames = [
            Frame(24, 67, {0: (0, -24), 7: (0, -35)}),
            Frame(22, 66, {0: (1, -27), 7: (0, -34)}),
        ]
        offset = missile_target_offset(frames)
        self.assertEqual(offset["frames"], 2)
        self.assertAlmostEqual(offset["mean_x"], -0.5)
        self.assertAlmostEqual(offset["mean_y"], -9.0)

    def test_reports_nothing_when_no_frame_carries_a_missile_target(self) -> None:
        self.assertEqual(missile_target_offset([Frame(10, 10, {0: (0, 0)})])["frames"], 0)


if __name__ == "__main__":
    unittest.main()
