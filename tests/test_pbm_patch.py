"""Tests for tools/pbm_patch.py.

The property this tool exists for is that the file's length never changes, so that is asserted on
every path rather than in one test. The fixtures are built packet by packet instead of being copied
out of the game, both because game content is not committed and because a fixture shaped like the
corpus cannot fail on what the corpus hides -- here, specifically, a run that straddles the
rectangle edge, which the shipped images may or may not happen to contain at any chosen rectangle.
"""

import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

import pbm_patch  # noqa: E402


def chunk(identifier: bytes, body: bytes) -> bytes:
    padded = body + (b"\x00" if len(body) & 1 else b"")
    return identifier + struct.pack(">I", len(body)) + padded


def bmhd(width: int, height: int, compression: int = 1, planes: int = 8) -> bytes:
    return chunk(
        b"BMHD",
        struct.pack(
            ">HHhhBBBBHBBhh", width, height, 0, 0, planes, 0, compression, 0, 0, 1, 1,
            width, height,
        ),
    )


def form(*chunks: bytes) -> bytearray:
    payload = b"PBM " + b"".join(chunks)
    return bytearray(b"FORM" + struct.pack(">I", len(payload)) + payload)


def repeat(count: int, value: int) -> bytes:
    """A ByteRun1 repeat packet covering `count` pixels."""
    assert 2 <= count <= 128
    return bytes([257 - count, value])


def literal(values: bytes) -> bytes:
    assert 1 <= len(values) <= 128
    return bytes([len(values) - 1]) + values


class PatchTest(unittest.TestCase):
    def test_repaints_a_run_that_lies_wholly_inside(self):
        # One 8-pixel row, a single run covering all of it.
        data = form(bmhd(8, 1), chunk(b"BODY", repeat(8, 0x20)))
        before = bytes(data)
        result = pbm_patch.patch(data, 0x7F, (0, 0, 8, 1))
        self.assertEqual(result.runs_rewritten, 1)
        self.assertEqual(result.pixels_repainted, 8)
        self.assertEqual(len(data), len(before))
        self.assertIn(repeat(8, 0x7F), bytes(data))

    def test_skips_a_run_that_straddles_the_rectangle_edge(self):
        # Two runs of 4 across an 8-pixel row; the rectangle ends mid-way through the second.
        data = form(bmhd(8, 1), chunk(b"BODY", repeat(4, 0x20) + repeat(4, 0x30)))
        result = pbm_patch.patch(data, 0x7F, (0, 0, 6, 1))
        self.assertEqual(result.runs_rewritten, 1, "only the first run is wholly inside")
        self.assertEqual(result.pixels_repainted, 4)
        self.assertIn(repeat(4, 0x30), bytes(data), "the straddling run is left alone")

    def test_leaves_literal_packets_alone(self):
        body = literal(bytes([1, 2, 3, 4])) + repeat(4, 0x20)
        data = form(bmhd(8, 1), chunk(b"BODY", body))
        result = pbm_patch.patch(data, 0x7F, (0, 0, 8, 1))
        self.assertEqual(result.runs_rewritten, 1)
        self.assertIn(literal(bytes([1, 2, 3, 4])), bytes(data))

    def test_rows_advance_so_a_later_row_is_out_of_range(self):
        # Two rows of 4 pixels; only row 0 is in the rectangle.
        data = form(bmhd(4, 2), chunk(b"BODY", repeat(4, 0x20) + repeat(4, 0x30)))
        result = pbm_patch.patch(data, 0x7F, (0, 0, 4, 1))
        self.assertEqual(result.runs_rewritten, 1)
        self.assertIn(repeat(4, 0x30), bytes(data), "row 1 is outside the rectangle")

    def test_no_operation_packet_does_not_advance_the_column(self):
        data = form(bmhd(4, 1), chunk(b"BODY", bytes([128]) + repeat(4, 0x20)))
        result = pbm_patch.patch(data, 0x7F, (0, 0, 4, 1))
        self.assertEqual(result.pixels_repainted, 4)

    def test_a_run_overshooting_the_edge_by_one_pixel_is_still_skipped(self):
        # Boundary: the second run ends at column 7 and the rectangle ends at 6. Off-by-one in the
        # `<=` would repaint it, so the rectangle is asserted to be half-open at exactly x1.
        body = repeat(4, 0x20) + repeat(3, 0x30) + literal(bytes([7]))
        data = form(bmhd(8, 1), chunk(b"BODY", body))
        result = pbm_patch.patch(data, 0x7F, (0, 0, 6, 1))
        self.assertEqual(result.runs_rewritten, 1)
        self.assertIn(repeat(3, 0x30), bytes(data), "a run ending at x1 + 1 is outside")

    def test_a_run_starting_one_pixel_before_the_edge_is_skipped(self):
        # The mirror of the test above, on the left edge: a run starting at x0 - 1 is outside.
        body = literal(bytes([7])) + repeat(2, 0x20) + repeat(5, 0x30)
        data = form(bmhd(8, 1), chunk(b"BODY", body))
        result = pbm_patch.patch(data, 0x7F, (2, 0, 8, 1))
        self.assertEqual(result.runs_rewritten, 1)
        self.assertIn(repeat(2, 0x20), bytes(data), "a run starting at x0 - 1 is outside")

    def test_index_255_is_a_legal_palette_index(self):
        # 255 is the last entry of a 256-colour CMAP and several shipped images use it as the
        # transparent index, so the upper bound is inclusive.
        data = form(bmhd(8, 1), chunk(b"BODY", repeat(8, 0x20)))
        result = pbm_patch.patch(data, 255, (0, 0, 8, 1))
        self.assertEqual(result.runs_rewritten, 1)
        self.assertIn(repeat(8, 255), bytes(data))

    def test_length_never_changes_for_any_rectangle(self):
        body = repeat(4, 0x20) + literal(bytes([9, 9])) + repeat(2, 0x30)
        original = form(bmhd(8, 1), chunk(b"BODY", body))
        for x1 in range(1, 9):
            data = bytearray(original)
            try:
                pbm_patch.patch(data, 0x7F, (0, 0, x1, 1))
            except pbm_patch.PbmPatchError:
                continue
            self.assertEqual(len(data), len(original), f"length changed at x1={x1}")


class RefusalTest(unittest.TestCase):
    def test_refuses_an_uncompressed_body(self):
        data = form(bmhd(8, 1, compression=0), chunk(b"BODY", bytes(8)))
        with self.assertRaisesRegex(pbm_patch.PbmPatchError, "ByteRun1"):
            pbm_patch.patch(data, 1, (0, 0, 8, 1))

    def test_refuses_planes_other_than_eight(self):
        data = form(bmhd(8, 1, planes=4), chunk(b"BODY", repeat(8, 0x20)))
        with self.assertRaisesRegex(pbm_patch.PbmPatchError, "bitplane"):
            pbm_patch.patch(data, 1, (0, 0, 8, 1))

    def test_refuses_a_non_pbm_form(self):
        data = form(bmhd(8, 1), chunk(b"BODY", repeat(8, 0x20)))
        data[8:12] = b"ILBM"
        with self.assertRaisesRegex(pbm_patch.PbmPatchError, "FORM PBM"):
            pbm_patch.patch(data, 1, (0, 0, 8, 1))

    def test_refuses_a_missing_body(self):
        data = form(bmhd(8, 1))
        with self.assertRaisesRegex(pbm_patch.PbmPatchError, "BODY"):
            pbm_patch.patch(data, 1, (0, 0, 8, 1))

    def test_refuses_a_rectangle_outside_the_image(self):
        data = form(bmhd(8, 1), chunk(b"BODY", repeat(8, 0x20)))
        for rect in [(0, 0, 9, 1), (0, 0, 8, 2), (-1, 0, 8, 1), (4, 0, 4, 1)]:
            with self.subTest(rect=rect):
                with self.assertRaises(pbm_patch.PbmPatchError):
                    pbm_patch.patch(bytearray(data), 1, rect)

    def test_refuses_an_index_outside_a_byte(self):
        data = form(bmhd(8, 1), chunk(b"BODY", repeat(8, 0x20)))
        for index in (-1, 256):
            with self.subTest(index=index):
                with self.assertRaisesRegex(pbm_patch.PbmPatchError, "0..255"):
                    pbm_patch.patch(bytearray(data), index, (0, 0, 8, 1))


if __name__ == "__main__":
    unittest.main()
