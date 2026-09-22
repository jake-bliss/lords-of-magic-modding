"""Guards for the Python LBM codec in tools/portrait-upscale/lbm_png.py.

Both of the behaviours pinned here were WRONG in the first version of that module and were caught
in review rather than by a test, which is why they now have one:

  * rows are padded to an even byte count, so an odd width is not a flat stream;
  * chunks the module does not model are carried through, because `CRNG` colour cycling is in 645
    of the 749 shipped portraits and is functional data.

The synthetic cases run everywhere. The corpus case runs only with `LOM_GAME_DIR` set, because a
test that only ever sees fixtures this module itself generated cannot fail on what the real members
do -- the failure mode this repository has hit repeatedly.
"""
from __future__ import annotations

import os
import pathlib
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "tools" / "portrait-upscale"))
import lbm_png  # noqa: E402


def build_pbm(width: int, height: int, indices: bytes, extra_chunks=()) -> bytes:
    """A minimal uncompressed FORM PBM, written independently of the encoder under test."""
    header = struct.pack(">HHhhBBBBHBBhh", width, height, 0, 0, 8, 0, 0, 0, 0, 1, 1, width, height)
    stride = (width + 1) & ~1
    body = bytearray()
    for y in range(height):
        row = bytearray(indices[y * width:(y + 1) * width])
        row.extend(b"\x00" * (stride - width))
        body += row
    palette = b"".join(bytes((i, (i * 7) & 255, (i * 13) & 255)) for i in range(256))

    out = bytearray()
    for chunk_id, payload in ((b"BMHD", header), (b"CMAP", palette), *extra_chunks, (b"BODY", bytes(body))):
        out += chunk_id + struct.pack(">I", len(payload)) + payload
        if len(payload) & 1:
            out += b"\x00"
    return b"FORM" + struct.pack(">I", 4 + len(out)) + b"PBM " + bytes(out)


class RowPadding(unittest.TestCase):
    def test_odd_width_round_trips(self):
        """An odd width is the case a flat-stream decoder gets wrong, one pixel per row."""
        width, height = 35, 9
        indices = bytes((x * 3 + y * 11) & 255 for y in range(height) for x in range(width))
        with tempfile.TemporaryDirectory() as td:
            source = pathlib.Path(td) / "odd.lbm"
            source.write_bytes(build_pbm(width, height, indices))
            w, h, got, palette, chunks = lbm_png.decode(source)
            self.assertEqual((w, h), (width, height))
            self.assertEqual(got, indices)

            dest = pathlib.Path(td) / "out.lbm"
            lbm_png.encode(dest, width, height, indices, palette, chunks)
            self.assertEqual(lbm_png.decode(dest)[2], indices)

    def test_even_width_round_trips(self):
        width, height = 70, 67
        indices = bytes((x ^ y) & 255 for y in range(height) for x in range(width))
        with tempfile.TemporaryDirectory() as td:
            source = pathlib.Path(td) / "even.lbm"
            source.write_bytes(build_pbm(width, height, indices))
            _, _, got, palette, chunks = lbm_png.decode(source)
            self.assertEqual(got, indices)
            dest = pathlib.Path(td) / "out.lbm"
            lbm_png.encode(dest, width, height, indices, palette, chunks)
            self.assertEqual(lbm_png.decode(dest)[2], indices)

    def test_row_bytes_rule(self):
        self.assertEqual(lbm_png.row_bytes(70), 70)
        self.assertEqual(lbm_png.row_bytes(35), 36)
        self.assertEqual(lbm_png.row_bytes(1), 2)


class ChunkPreservation(unittest.TestCase):
    def setUp(self):
        self.width, self.height = 8, 4
        self.indices = bytes(range(self.width * self.height))
        self.crng = b"\x00\x00\x00\x08\x00\x01\x02\x03"
        self.dpps = b"\xde\xad\xbe\xef"
        self.tiny = b"\x01\x02\x03\x04"

    def _round_trip(self, td):
        source = pathlib.Path(td) / "in.lbm"
        source.write_bytes(build_pbm(self.width, self.height, self.indices, extra_chunks=(
            (b"CRNG", self.crng), (b"DPPS", self.dpps), (b"TINY", self.tiny))))
        _, _, indices, palette, chunks = lbm_png.decode(source)
        dest = pathlib.Path(td) / "out.lbm"
        lbm_png.encode(dest, self.width, self.height, indices, palette, chunks)
        return dict(lbm_png.iter_chunks(dest.read_bytes()))

    def test_crng_and_dpps_survive(self):
        with tempfile.TemporaryDirectory() as td:
            written = self._round_trip(td)
        self.assertEqual(written.get(b"CRNG"), self.crng)
        self.assertEqual(written.get(b"DPPS"), self.dpps)

    def test_tiny_is_dropped_because_it_is_derived(self):
        with tempfile.TemporaryDirectory() as td:
            written = self._round_trip(td)
        self.assertNotIn(b"TINY", written)

    def test_pixels_and_dimensions_are_rewritten(self):
        with tempfile.TemporaryDirectory() as td:
            written = self._round_trip(td)
        self.assertEqual(struct.unpack(">HH", written[b"BMHD"][:4]), (self.width, self.height))
        self.assertEqual(written[b"BMHD"][10], 1, "encoder only emits ByteRun1")


class ByteRun1(unittest.TestCase):
    def _decode(self, packed: bytes, length: int) -> bytes:
        """An independent unpacker, so the encoder is not checked against itself."""
        out = bytearray()
        i = 0
        while i < len(packed) and len(out) < length:
            control = packed[i]
            i += 1
            if control < 128:
                out += packed[i:i + control + 1]
                i += control + 1
            elif control > 128:
                out += bytes([packed[i]]) * (257 - control)
                i += 1
        return bytes(out)

    def test_long_run_and_long_literal(self):
        for row in (b"\x07" * 300, bytes(range(200)), b"", b"\x01",
                    b"\x05" * 128 + bytes(range(130)) + b"\x09" * 129):
            with self.subTest(length=len(row)):
                self.assertEqual(self._decode(lbm_png.byterun1(row), len(row)), row)

    def test_pseudorandom_rows(self):
        state = 12345
        for _ in range(300):
            state = (state * 1103515245 + 12345) & 0x7fffffff
            length = 1 + state % 300
            row = bytearray()
            for _ in range(length):
                state = (state * 1103515245 + 12345) & 0x7fffffff
                row.append((state >> 16) % 5)      # small alphabet forces runs and literals to mix
            self.assertEqual(self._decode(lbm_png.byterun1(bytes(row)), length), bytes(row))


class RejectsWhatItCannotHandle(unittest.TestCase):
    def test_non_pbm(self):
        with tempfile.TemporaryDirectory() as td:
            bad = pathlib.Path(td) / "bad.lbm"
            bad.write_bytes(b"NOTFORM" + b"\x00" * 32)
            with self.assertRaises(ValueError):
                lbm_png.decode(bad)

    def test_index_outside_palette(self):
        with tempfile.TemporaryDirectory() as td:
            source = pathlib.Path(td) / "in.lbm"
            source.write_bytes(build_pbm(4, 2, bytes(8)))
            _, _, _, palette, chunks = lbm_png.decode(source)
            with self.assertRaises(ValueError):
                lbm_png.encode(pathlib.Path(td) / "out.lbm", 4, 2,
                               bytes([250] * 8), palette[:16], chunks)


@unittest.skipUnless(os.environ.get("LOM_GAME_DIR"), "needs LOM_GAME_DIR with an extracted pic.mpq")
class AgainstTheCorpus(unittest.TestCase):
    """Round-trip real members, because fixtures this module made cannot surprise it."""

    def test_shipped_portraits_round_trip_with_their_chunks(self):
        root = pathlib.Path(os.environ["LOM_GAME_DIR"])
        members = sorted(p for p in root.rglob("*.lbm") if p.is_file())[:40]
        if not members:
            self.skipTest(f"no .lbm members under {root}")
        with tempfile.TemporaryDirectory() as td:
            for member in members:
                with self.subTest(member=member.name):
                    w, h, indices, palette, chunks = lbm_png.decode(member)
                    dest = pathlib.Path(td) / "rt.lbm"
                    lbm_png.encode(dest, w, h, indices, palette, chunks)
                    rw, rh, rindices, _, rchunks = lbm_png.decode(dest)
                    self.assertEqual((rw, rh), (w, h))
                    self.assertEqual(rindices, indices)
                    kept = {cid for cid, _ in chunks} - lbm_png.DERIVED_CHUNKS
                    self.assertEqual({cid for cid, _ in rchunks}, kept)


if __name__ == "__main__":
    unittest.main()
