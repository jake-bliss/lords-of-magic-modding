"""Tests for the engine capture reader and its component differencing."""

import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

import probe_captures  # noqa: E402


def write_capture(path: Path, width: int, height: int, pixels) -> None:
    """Write a BMP the way the engine does: bfOffBits lying, pixel bytes in R, G, B order."""
    stride = (width * 3 + 3) // 4 * 4
    body = bytearray()
    for y in range(height - 1, -1, -1):  # bottom-up
        row = bytearray()
        for x in range(width):
            row += bytes(pixels[y][x])
        row += b"\x00" * (stride - len(row))
        body += row
    header = bytearray(probe_captures.PIXEL_OFFSET)
    header[0:2] = b"BM"
    struct.pack_into("<I", header, 2, probe_captures.PIXEL_OFFSET + len(body))
    struct.pack_into("<I", header, 10, 14)  # the engine's bfOffBits, which is wrong
    struct.pack_into("<I", header, 14, 40)
    struct.pack_into("<ii", header, 18, width, height)
    struct.pack_into("<H", header, 26, 1)
    struct.pack_into("<H", header, 28, 24)
    path.write_bytes(bytes(header) + bytes(body))


class ProbeCapturesTest(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = Path(tempfile.mkdtemp())
        self.width, self.height = 12, 10
        self.blank = [[(10, 20, 30)] * self.width for _ in range(self.height)]

    def test_reads_pixels_in_written_order(self) -> None:
        pixels = [row[:] for row in self.blank]
        pixels[0][0] = (255, 0, 0)
        pixels[self.height - 1][self.width - 1] = (0, 0, 255)
        path = self.directory / "a.bmp"
        write_capture(path, self.width, self.height, pixels)
        capture = probe_captures.read_capture(path)
        self.assertEqual(capture.width, self.width)
        self.assertEqual(capture.height, self.height)
        # Read back unswapped: a red pixel written stays red, which is the whole point.
        self.assertEqual(capture.pixel(0, 0), (255, 0, 0))
        self.assertEqual(capture.pixel(self.width - 1, self.height - 1), (0, 0, 255))

    def test_separates_touching_and_distant_changes(self) -> None:
        before = self.directory / "before.bmp"
        after = self.directory / "after.bmp"
        write_capture(before, self.width, self.height, self.blank)
        pixels = [row[:] for row in self.blank]
        for y in range(2, 5):  # a 3x3 block
            for x in range(1, 4):
                pixels[y][x] = (200, 200, 200)
        pixels[8][10] = (1, 2, 3)  # a distant single pixel
        write_capture(after, self.width, self.height, pixels)

        components = probe_captures.changed_components(
            probe_captures.read_capture(before), probe_captures.read_capture(after)
        )
        self.assertEqual([c.size for c in components], [9, 1])
        self.assertEqual(components[0].bounds, (1, 2, 3, 3))
        self.assertEqual(components[1].bounds, (10, 8, 1, 1))

    def test_union_spans_a_sprite_split_by_its_own_transparent_gap(self) -> None:
        """One IMP frame can arrive as several components; the union is the silhouette.

        This is the 2026-09-19 `unitanchor` control in miniature: `licr2a.imp` frame 0 is 30x122
        and came back as 30x111 plus a detached 8x9 three rows lower. Read component-wise the
        control looked 11 pixels short of the frame it had in fact reproduced exactly.
        """
        before = self.directory / "gap_before.bmp"
        after = self.directory / "gap_after.bmp"
        write_capture(before, self.width, self.height, self.blank)
        pixels = [row[:] for row in self.blank]
        for x in range(2, 5):  # the sprite's body
            pixels[1][x] = (200, 200, 200)
            pixels[2][x] = (200, 200, 200)
        pixels[6][3] = (200, 200, 200)  # its detached tail, below a transparent band
        write_capture(after, self.width, self.height, pixels)

        components = probe_captures.changed_components(
            probe_captures.read_capture(before), probe_captures.read_capture(after)
        )
        self.assertEqual([c.bounds for c in components], [(2, 1, 3, 2), (3, 6, 1, 1)])
        # Neither component is the sprite. Their union is.
        self.assertEqual(probe_captures.union_bounds(components), (2, 1, 3, 6))

    def test_union_of_no_components_is_none(self) -> None:
        self.assertIsNone(probe_captures.union_bounds([]))

    def test_describe_reports_the_union_and_what_the_threshold_hid(self) -> None:
        """The union line is the fix; the dropped-component line is what makes it trustworthy.

        A sprite's outlying tail can be two or three pixels, which the reporting threshold hides.
        A union over only the *listed* components would then be silently short, which is the same
        class of error this whole change exists to stop.
        """
        before = self.directory / "thr_before.bmp"
        after = self.directory / "thr_after.bmp"
        write_capture(before, self.width, self.height, self.blank)
        pixels = [row[:] for row in self.blank]
        for y in range(1, 4):  # a 3x3 body, n=9
            for x in range(1, 4):
                pixels[y][x] = (200, 200, 200)
        pixels[8][5] = (7, 7, 7)  # a 1-pixel tail, below any sensible threshold
        write_capture(after, self.width, self.height, pixels)

        text = probe_captures.describe(before, after, minimum=5)
        self.assertIn("union of the reported components: top-left=(1,1) 3x3", text)
        self.assertIn("1 component(s) below n=5 not listed", text)
        self.assertIn("union including them: top-left=(1,1) 5x8", text)

    def test_describe_omits_the_threshold_line_when_nothing_was_dropped(self) -> None:
        before = self.directory / "clean_before.bmp"
        after = self.directory / "clean_after.bmp"
        write_capture(before, self.width, self.height, self.blank)
        pixels = [row[:] for row in self.blank]
        for y in range(1, 4):
            for x in range(1, 4):
                pixels[y][x] = (200, 200, 200)
        write_capture(after, self.width, self.height, pixels)

        text = probe_captures.describe(before, after, minimum=5)
        self.assertIn("union of the reported components: top-left=(1,1) 3x3", text)
        self.assertNotIn("not listed", text)

    def test_identical_captures_have_no_components(self) -> None:
        path = self.directory / "same.bmp"
        write_capture(path, self.width, self.height, self.blank)
        capture = probe_captures.read_capture(path)
        self.assertEqual(probe_captures.changed_components(capture, capture), [])

    def test_mismatched_sizes_are_rejected(self) -> None:
        small = self.directory / "small.bmp"
        large = self.directory / "large.bmp"
        write_capture(small, self.width, self.height, self.blank)
        write_capture(large, self.width + 2, self.height, [[(0, 0, 0)] * (self.width + 2)] * self.height)
        with self.assertRaises(ValueError):
            probe_captures.changed_components(
                probe_captures.read_capture(small), probe_captures.read_capture(large)
            )

    def test_top_down_rows_are_not_mirrored(self) -> None:
        """A negative height means top-down storage; mirroring it would invert every y report."""
        pixels = [row[:] for row in self.blank]
        pixels[0][0] = (255, 0, 0)
        bottom_up = self.directory / "bottom_up.bmp"
        top_down = self.directory / "top_down.bmp"
        write_capture(bottom_up, self.width, self.height, pixels)
        write_capture(top_down, self.width, self.height, pixels)
        raw = bytearray(top_down.read_bytes())
        struct.pack_into("<i", raw, 22, -self.height)  # same rows, declared top-down
        # Re-lay the payload in top-down order so the file is internally consistent.
        stride = (self.width * 3 + 3) // 4 * 4
        body = bytearray()
        for y in range(self.height):
            row = bytearray()
            for x in range(self.width):
                row += bytes(pixels[y][x])
            row += b"\x00" * (stride - len(row))
            body += row
        raw[probe_captures.PIXEL_OFFSET:] = body
        top_down.write_bytes(bytes(raw))

        self.assertEqual(
            probe_captures.read_capture(top_down).pixels,
            probe_captures.read_capture(bottom_up).pixels,
        )

    def test_truncated_pixel_data_is_rejected(self) -> None:
        path = self.directory / "short.bmp"
        write_capture(path, self.width, self.height, self.blank)
        path.write_bytes(path.read_bytes()[:-20])
        with self.assertRaises(ValueError):
            probe_captures.read_capture(path)

    def test_non_positive_dimensions_are_rejected(self) -> None:
        path = self.directory / "wide.bmp"
        write_capture(path, self.width, self.height, self.blank)
        raw = bytearray(path.read_bytes())
        struct.pack_into("<i", raw, 18, -self.width)
        path.write_bytes(bytes(raw))
        with self.assertRaises(ValueError):
            probe_captures.read_capture(path)

    def test_header_shorter_than_the_pixel_offset_is_rejected(self) -> None:
        path = self.directory / "stub.bmp"
        path.write_bytes(b"BM" + b"\x00" * 10)
        with self.assertRaises(ValueError):
            probe_captures.read_capture(path)

    def test_non_bmp_is_rejected(self) -> None:
        path = self.directory / "not.bmp"
        path.write_bytes(b"nope")
        with self.assertRaises(ValueError):
            probe_captures.read_capture(path)


if __name__ == "__main__":
    unittest.main()
