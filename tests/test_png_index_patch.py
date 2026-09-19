"""The indexed-PNG editor, and the one property the engine ladder depends on.

The property is that a set of disjoint palette-index swaps cannot change the length of a
run-length-encoded payload, while a fill can. `RunLengthPropertyTest` restates it on made-up
indices, which is worth having and cannot fail on the rule itself being wrong.
`ImpEncoderLengthTest` therefore puts the ladder's ACTUAL swap set through the real IMP encoder on
the real member the ladder ships, and asserts the member's byte count both ways -- with a fill as
the negative control, so "the length held" cannot be a property of that one frame.
"""

import struct
import subprocess
import sys
import tempfile
import unittest
import zlib
from pathlib import Path

PROJECT_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PROJECT_DIR / "tools"))

from png_index_patch import (  # noqa: E402
    PngError,
    apply_fill,
    apply_swaps,
    histogram,
    main,
    read_indexed_png,
    write_indexed_png,
)

TOOL = PROJECT_DIR / "tools" / "png_index_patch.py"


def build_png(width: int, height: int, indices: bytes, *, filter_type: int = 0) -> bytes:
    """A minimal indexed PNG, written independently of the module under test.

    The palette is a gradient rather than anything meaningful; what matters is that this writer
    shares no code with `write_indexed_png`, so a round trip through the module is not a round trip
    through one implementation's own idea of the format.
    """
    assert len(indices) == width * height
    out = bytearray(b"\x89PNG\r\n\x1a\n")

    def emit(kind: bytes, payload: bytes) -> None:
        out.extend(struct.pack(">I", len(payload)))
        out.extend(kind)
        out.extend(payload)
        out.extend(struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF))

    emit(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 3, 0, 0, 0))
    emit(b"PLTE", bytes(value for index in range(256) for value in (index, 255 - index, 0)))
    raw = bytearray()
    previous = bytearray(width)
    for row in range(height):
        line = indices[row * width : (row + 1) * width]
        raw.append(filter_type)
        if filter_type == 0:
            raw.extend(line)
        elif filter_type == 2:
            raw.extend(bytes((value - up) & 0xFF for value, up in zip(line, previous)))
        else:  # pragma: no cover - the tests only use 0 and 2
            raise AssertionError(f"fixture cannot write filter {filter_type}")
        previous = bytearray(line)
    emit(b"IDAT", zlib.compress(bytes(raw)))
    emit(b"IEND", b"")
    return bytes(out)


class ReadWriteTest(unittest.TestCase):
    def test_reads_indices_written_by_an_independent_writer(self) -> None:
        indices = bytes(range(16))
        image = read_indexed_png(build_png(4, 4, indices))

        self.assertEqual((image.width, image.height), (4, 4))
        self.assertEqual(bytes(image.indices), indices)

    def test_undoes_the_up_filter(self) -> None:
        indices = bytes([1, 2, 3, 4, 9, 9, 9, 9])
        image = read_indexed_png(build_png(4, 2, indices, filter_type=2))

        self.assertEqual(bytes(image.indices), indices)

    def test_a_round_trip_keeps_the_palette_bytes_exactly(self) -> None:
        original = build_png(3, 3, bytes(9))
        image = read_indexed_png(original)
        again = read_indexed_png(write_indexed_png(image))

        self.assertEqual(again.palette(), image.palette())
        self.assertEqual(bytes(again.indices), bytes(image.indices))

    def test_an_ancillary_chunk_survives_the_round_trip(self) -> None:
        # The IMP exporter writes tRNS beside PLTE. Losing it would not break the import -- the
        # importer reads neither -- but it would mean the file this tool hands back is not the file
        # the exporter produced, and a later reader comparing them would be told nothing useful.
        original = bytearray(build_png(2, 2, bytes(4)))
        payload = bytes([0, 255])
        chunk = struct.pack(">I", len(payload)) + b"tRNS" + payload + struct.pack(
            ">I", zlib.crc32(b"tRNS" + payload) & 0xFFFFFFFF
        )
        insert_at = original.index(b"IDAT") - 4
        original[insert_at:insert_at] = chunk

        rewritten = write_indexed_png(read_indexed_png(bytes(original)))

        self.assertIn(b"tRNS", rewritten)
        self.assertIn(payload, rewritten)

    def test_a_truecolour_png_is_refused_by_name(self) -> None:
        header = struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0)
        data = bytearray(b"\x89PNG\r\n\x1a\n")
        data.extend(struct.pack(">I", len(header)))
        data.extend(b"IHDR")
        data.extend(header)
        data.extend(struct.pack(">I", zlib.crc32(b"IHDR" + header) & 0xFFFFFFFF))

        with self.assertRaises(PngError) as raised:
            read_indexed_png(bytes(data))
        self.assertIn("colour type", str(raised.exception))

    def test_a_corrupt_chunk_crc_is_refused(self) -> None:
        data = bytearray(build_png(2, 2, bytes(4)))
        data[-1] ^= 0xFF

        with self.assertRaises(PngError) as raised:
            read_indexed_png(bytes(data))
        self.assertIn("CRC", str(raised.exception))


class SwapTest(unittest.TestCase):
    def test_swapping_is_symmetric_and_touches_nothing_else(self) -> None:
        image = read_indexed_png(build_png(2, 2, bytes([1, 2, 3, 1])))
        moved = apply_swaps(image, [(1, 2)])

        self.assertEqual(bytes(image.indices), bytes([2, 1, 3, 2]))
        self.assertEqual(moved, 3)

    def test_overlapping_swaps_are_refused(self) -> None:
        image = read_indexed_png(build_png(1, 1, bytes([0])))
        with self.assertRaises(ValueError) as raised:
            apply_swaps(image, [(1, 2), (2, 3)])
        self.assertIn("disjoint", str(raised.exception))

    def test_an_index_outside_the_alphabet_is_refused(self) -> None:
        image = read_indexed_png(build_png(1, 1, bytes([0])))
        with self.assertRaises(ValueError):
            apply_swaps(image, [(1, 300)])


class FillTest(unittest.TestCase):
    def test_fill_changes_only_the_rectangle(self) -> None:
        image = read_indexed_png(build_png(3, 3, bytes(9)))
        changed = apply_fill(image, 7, (1, 1, 3, 3))

        self.assertEqual(changed, 4)
        self.assertEqual(
            bytes(image.indices), bytes([0, 0, 0, 0, 7, 7, 0, 7, 7])
        )

    def test_a_rectangle_past_the_edge_is_refused(self) -> None:
        image = read_indexed_png(build_png(3, 3, bytes(9)))
        with self.assertRaises(ValueError):
            apply_fill(image, 7, (0, 0, 4, 3))

    def test_an_inverted_rectangle_is_refused(self) -> None:
        image = read_indexed_png(build_png(3, 3, bytes(9)))
        with self.assertRaises(ValueError):
            apply_fill(image, 7, (2, 0, 1, 3))


class HistogramTest(unittest.TestCase):
    def test_counts_are_ordered_by_frequency(self) -> None:
        image = read_indexed_png(build_png(2, 2, bytes([5, 5, 5, 9])))
        self.assertEqual(histogram(image), [(5, 3), (9, 1)])


class RunLengthPropertyTest(unittest.TestCase):
    """Swaps preserve run boundaries; a fill destroys them.

    This is the property the ladder's rung 2 rests on. What is checked here is the *boundary*
    structure -- where neighbouring bytes stop being equal -- because that, and nothing else, is
    what `imp::encode_rle` keys its packets on. It is a restatement of the rule and it cannot fail
    on the rule being wrong, which is why `ImpEncoderLengthTest` below puts the same edit through
    the real encoder on a real member.
    """

    @staticmethod
    def run_lengths(indices: bytes) -> list[int]:
        runs = []
        position = 0
        while position < len(indices):
            length = 1
            while (
                position + length < len(indices)
                and indices[position + length] == indices[position]
            ):
                length += 1
            runs.append(length)
            position += length
        return runs

    def test_a_permutation_leaves_the_run_structure_identical(self) -> None:
        indices = bytes([1, 1, 1, 2, 2, 3, 1, 1, 4, 4, 4, 4])
        image = read_indexed_png(build_png(4, 3, indices))
        before = self.run_lengths(bytes(image.indices))

        apply_swaps(image, [(1, 9), (2, 8)])

        self.assertNotEqual(bytes(image.indices), indices, "the swap did nothing")
        self.assertEqual(self.run_lengths(bytes(image.indices)), before)

    def test_a_fill_merges_runs(self) -> None:
        indices = bytes([1, 2, 3, 4, 5, 6, 7, 8, 9])
        image = read_indexed_png(build_png(3, 3, indices))

        apply_fill(image, 1, (0, 0, 3, 3))

        self.assertEqual(self.run_lengths(bytes(image.indices)), [9])


VIEWER = PROJECT_DIR / "spikes" / "asset-viewer" / "target" / "debug" / "lom-asset-viewer"
GAME_DIR = (
    Path.home()
    / "Applications"
    / "Steambuild 32 64bit DXVK.app"
    / "Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps"
    / "common/Lords of Magic Special Edition/English"
)
IMP_LISTFILE = PROJECT_DIR / "reports" / "member-names" / "all-profiles-imp-recovered.txt"
LADDER_MEMBER = "iface\\cursors.imp"
LADDER_FRAME = "111"


@unittest.skipUnless(
    VIEWER.is_file() and (GAME_DIR / "imp.mpq").is_file(),
    "needs the built viewer and an installed vanilla profile",
)
class ImpEncoderLengthTest(unittest.TestCase):
    """The same edit, through the real IMP encoder, on the member the ladder actually ships.

    A swap must leave the member's byte count alone and a fill must not. Asserting it here rather
    than only in the build script is what keeps the property from silently lapsing: the swap set in
    `scripts/build-acceptance-ladder.sh` is chosen for what it looks like, and the reason it is
    *allowed* is this.
    """

    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.root = Path(self._temporary.name)
        self.addCleanup(self._temporary.cleanup)
        self.template = self.root / "cursors.imp"
        self.png = self.root / "frame.png"
        self.run_viewer(
            "--extract", str(GAME_DIR / "imp.mpq"), LADDER_MEMBER, str(self.template),
            "--listfile", str(IMP_LISTFILE),
        )
        self.run_viewer(
            "--export-imp-frame", str(GAME_DIR / "imp.mpq"), LADDER_MEMBER, LADDER_FRAME,
            str(self.png), "--listfile", str(IMP_LISTFILE),
        )

    def run_viewer(self, *arguments: str) -> subprocess.CompletedProcess:
        result = subprocess.run(
            [str(VIEWER), *arguments], capture_output=True, text=True, check=False
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def import_frame(self, png: Path, name: str) -> Path:
        output = self.root / name
        self.run_viewer("--import-png-imp", str(png), str(self.template), LADDER_FRAME, str(output))
        return output

    def test_the_ladders_swap_set_keeps_the_members_byte_count(self) -> None:
        edited = self.root / "swapped.png"
        self.assertEqual(
            main(
                [str(self.png), str(edited)]
                + [
                    argument
                    for pair in ("56=2", "41=3", "99=4", "98=6", "120=7", "157=8", "139=9", "243=15")
                    for argument in ("--swap", pair)
                ]
            ),
            0,
        )
        written = self.import_frame(edited, "swapped.imp")

        self.assertNotEqual(written.read_bytes(), self.template.read_bytes())
        self.assertEqual(written.stat().st_size, self.template.stat().st_size)

    def test_a_fill_over_the_same_frame_does_change_the_members_byte_count(self) -> None:
        # The negative control. Without it, "the swap preserved the length" could be a property of
        # this frame rather than of the swap.
        edited = self.root / "filled.png"
        self.assertEqual(main([str(self.png), str(edited), "--fill", "251"]), 0)
        written = self.import_frame(edited, "filled.imp")

        self.assertNotEqual(written.stat().st_size, self.template.stat().st_size)


class CommandLineTest(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.root = Path(self._temporary.name)
        self.addCleanup(self._temporary.cleanup)

    def write(self, name: str, indices: bytes, width: int, height: int) -> Path:
        path = self.root / name
        path.write_bytes(build_png(width, height, indices))
        return path

    def test_compare_to_reports_identical_files_as_zero_difference(self) -> None:
        left = self.write("a.png", bytes([1, 2, 3, 4]), 2, 2)
        right = self.write("b.png", bytes([1, 2, 3, 4]), 2, 2)

        self.assertEqual(main([str(left), "--compare-to", str(right)]), 0)

    def test_compare_to_exits_non_zero_on_any_differing_pixel(self) -> None:
        left = self.write("a.png", bytes([1, 2, 3, 4]), 2, 2)
        right = self.write("b.png", bytes([1, 2, 3, 5]), 2, 2)

        self.assertEqual(main([str(left), "--compare-to", str(right)]), 1)

    def test_refuses_to_overwrite_an_existing_output(self) -> None:
        source = self.write("a.png", bytes([1, 2, 3, 4]), 2, 2)
        existing = self.root / "out.png"
        existing.write_bytes(b"not a png")

        self.assertEqual(main([str(source), str(existing), "--swap", "1=2"]), 1)
        self.assertEqual(existing.read_bytes(), b"not a png")

    def test_the_command_line_writes_what_the_library_would(self) -> None:
        source = self.write("a.png", bytes([1, 1, 2, 2]), 2, 2)
        output = self.root / "out.png"

        result = subprocess.run(
            [sys.executable, str(TOOL), str(source), str(output), "--swap", "1=7"],
            capture_output=True,
            text=True,
            check=False,
            env={"PYTHONDONTWRITEBYTECODE": "1", "PATH": "/usr/bin:/bin"},
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("pixels-changed=2", result.stdout)
        self.assertEqual(
            bytes(read_indexed_png(output.read_bytes()).indices), bytes([7, 7, 2, 2])
        )


if __name__ == "__main__":
    unittest.main()
