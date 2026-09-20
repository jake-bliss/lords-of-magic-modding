"""Tests for the independent IMP record walk.

The fixtures are synthesised rather than copied from the game, so nothing proprietary enters the
repository. That also bounds what these tests can establish: they check the walk's arithmetic and
its refusals, **not** that its offsets match the shipped archive. The claim that it lands on the
right offsets rests on reproducing the three published GS5R3 control counts, which is what
`--expect-gs5r3-controls` is for and which only a real corpus run can exercise.
"""

from __future__ import annotations

import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from tools.imp_structure_scan import (  # noqa: E402
    FACING_RECORD_SIZE,
    FRAME_RECORD_SIZE,
    SEQUENCE_RECORD_SIZE,
    scan,
    walk,
)


def build_imp(sequences: list[list[list[tuple[int, int]]]]) -> bytes:
    """Lay out an IMP whose facings hold the given `(width, height)` frames.

    `sequences[s][f]` is one facing's frame list, so an empty list is a facing with no frames and
    a `(0, 0)` entry is a zero-dimension frame. The two are the shapes these tests keep apart.
    """
    header = bytearray(32)
    sequence_table = 32
    facing_table = sequence_table + len(sequences) * SEQUENCE_RECORD_SIZE
    facing_total = sum(len(facings) for facings in sequences)
    frame_table = facing_table + facing_total * FACING_RECORD_SIZE

    struct.pack_into("<H", header, 26, len(sequences))
    struct.pack_into("<I", header, 28, sequence_table)

    sequence_records = bytearray()
    facing_records = bytearray()
    frame_records = bytearray()
    facing_cursor = facing_table
    frame_cursor = frame_table

    for facings in sequences:
        record = bytearray(SEQUENCE_RECORD_SIZE)
        record[11] = len(facings)
        struct.pack_into("<I", record, 12, facing_cursor)
        sequence_records += record
        facing_cursor += len(facings) * FACING_RECORD_SIZE

        for frames in facings:
            facing = bytearray(FACING_RECORD_SIZE)
            struct.pack_into("<H", facing, 2, len(frames))
            struct.pack_into("<I", facing, 4, frame_cursor)
            facing_records += facing
            frame_cursor += len(frames) * FRAME_RECORD_SIZE

            for width, height in frames:
                frame = bytearray(FRAME_RECORD_SIZE)
                struct.pack_into("<H", frame, 2, width)
                struct.pack_into("<H", frame, 4, height)
                frame_records += frame

    return bytes(header + sequence_records + facing_records + frame_records)


class WalkTest(unittest.TestCase):
    def test_counts_each_record_table(self) -> None:
        data = build_imp([[[(8, 8), (8, 8)], [(8, 8)]], [[(8, 8)]]])
        self.assertEqual(
            walk(data),
            {
                "sequences": 2,
                "facings": 3,
                "zero_frame_facings": 0,
                "frames": 4,
                "zero_dimension_frames": 0,
            },
        )

    def test_an_empty_facing_and_a_zero_dimension_frame_are_different_shapes(self) -> None:
        """The exact conflation this tool was written to settle.

        A facing with no frames contributes to `zero_frame_facings` and nothing to `frames`; a
        frame with zero width and height contributes to `frames` and to `zero_dimension_frames`.
        A walk that counted either as the other would pass a test that only checked one of them.
        """
        empty_facing = walk(build_imp([[[]]]))
        self.assertEqual(empty_facing["zero_frame_facings"], 1)
        self.assertEqual(empty_facing["frames"], 0)
        self.assertEqual(empty_facing["zero_dimension_frames"], 0)

        blank_frame = walk(build_imp([[[(0, 0)]]]))
        self.assertEqual(blank_frame["zero_frame_facings"], 0)
        self.assertEqual(blank_frame["frames"], 1)
        self.assertEqual(blank_frame["zero_dimension_frames"], 1)

    def test_a_partially_blank_frame_is_not_counted_as_blank(self) -> None:
        tally = walk(build_imp([[[(0, 9), (9, 0)]]]))
        self.assertEqual(tally["zero_dimension_frames"], 0)
        self.assertEqual(tally["frames"], 2)

    def test_a_truncated_header_is_refused(self) -> None:
        with self.assertRaises(ValueError):
            walk(b"\x00" * 8)

    def test_a_file_with_no_sequences_is_refused(self) -> None:
        with self.assertRaises(ValueError):
            walk(bytes(32))

    def test_an_out_of_range_table_pointer_is_refused_not_silently_skipped(self) -> None:
        data = bytearray(build_imp([[[(8, 8)]]]))
        struct.pack_into("<I", data, 28, 0xFFFF)
        with self.assertRaises(ValueError):
            walk(bytes(data))


class ScanTest(unittest.TestCase):
    def test_totals_sum_over_members_and_name_the_blank_files(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "a.imp").write_bytes(build_imp([[[(8, 8), (0, 0)]]]))
            (root / "nested").mkdir()
            (root / "nested" / "b.imp").write_bytes(build_imp([[[(8, 8)], []]]))
            (root / "ignored.txt").write_bytes(b"not an imp")

            report = scan(root)

        self.assertEqual(report["totals"]["members"], 2)
        self.assertEqual(report["totals"]["sequences"], 2)
        self.assertEqual(report["totals"]["facings"], 3)
        self.assertEqual(report["totals"]["frames"], 3)
        self.assertEqual(report["totals"]["zero_dimension_frames"], 1)
        self.assertEqual(report["totals"]["zero_frame_facings"], 1)
        self.assertEqual(report["zero_dimension_files"], 1)
        self.assertEqual(report["skipped"], [])

    def test_an_unparseable_member_is_reported_rather_than_dropped(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "good.imp").write_bytes(build_imp([[[(8, 8)]]]))
            (root / "bad.imp").write_bytes(b"\x00" * 8)
            report = scan(root)

        self.assertEqual(report["totals"]["members"], 1)
        self.assertEqual([name for name, _ in report["skipped"]], ["bad.imp"])


if __name__ == "__main__":
    unittest.main()
