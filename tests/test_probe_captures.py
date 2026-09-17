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

    def test_non_bmp_is_rejected(self) -> None:
        path = self.directory / "not.bmp"
        path.write_bytes(b"nope")
        with self.assertRaises(ValueError):
            probe_captures.read_capture(path)


if __name__ == "__main__":
    unittest.main()
