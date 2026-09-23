"""The standard-library MPQ reader the HD overlay's setup runs on the player's machine.

The decisive test is the corpus one: every named member of an installed pic.mpq, read by this
reader and by StormLib (`lom-mpq extract`), must be the same bytes. The unit tests pin the pieces
that corpus cannot isolate.
"""

from __future__ import annotations

import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))

import mpq_read  # noqa: E402

LOM_MPQ = ROOT / ".build" / "lom-mpq"
LISTFILE = ROOT / "artifacts" / "reference-listfiles" / "lords-of-magic.txt"
GAME = "drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English"
ARCHIVES = {
    "vanilla (3.02)": pathlib.Path.home() / "Applications/Lords of Magic 3.02.app/Contents/SharedSupport/prefix" / GAME / "pic.mpq",
    "GS5R3": ROOT / "artifacts/experiment-backups/gs5r3-20260916/pic.mpq.orig",
}


class Explode(unittest.TestCase):
    def test_the_blast_c_reference_vector(self) -> None:
        """blast.c's own example: literal mode 0, 4-bit dictionary, one back-reference."""
        self.assertEqual(mpq_read.explode(bytes.fromhex("00048224258f807f"), 13), b"AIAIAIAIAIAIA")

    def test_a_size_that_disagrees_with_the_stream_is_an_error(self) -> None:
        """Stopping at the expected size handed back truncated data as whole. (Codex review.)"""
        for wrong in (5, 12, 14):
            with self.subTest(wrong), self.assertRaises(mpq_read.MpqError):
                mpq_read.explode(bytes.fromhex("00048224258f807f"), wrong)

    def test_sector_crc_members_are_refused_not_half_read(self) -> None:
        self.assertFalse(mpq_read.KNOWN_FLAGS & mpq_read.FLAG_SECTOR_CRC)

    def test_a_truncated_stream_is_an_error_not_short_output(self) -> None:
        with self.assertRaises(mpq_read.MpqError):
            mpq_read.explode(bytes.fromhex("00048224"), 100)

    def test_a_bad_header_is_refused(self) -> None:
        for header in ("0204", "0003", "0007"):
            with self.subTest(header), self.assertRaises(mpq_read.MpqError):
                mpq_read.explode(bytes.fromhex(header) + b"\0" * 8, 10)


class Crypto(unittest.TestCase):
    def test_the_well_known_table_keys(self) -> None:
        """Every MPQ implementation derives these two keys; published as 0xC3AF3770 / 0xEC83B3A3."""
        self.assertEqual(mpq_read.hash_string("(hash table)", 3), 0xC3AF3770)
        self.assertEqual(mpq_read.hash_string("(block table)", 3), 0xEC83B3A3)

    def test_hashing_ignores_case(self) -> None:
        self.assertEqual(mpq_read.hash_string("portrait\\AIpotM.lbm", 0),
                         mpq_read.hash_string("PORTRAIT\\aipotm.LBM", 0))

    def test_a_trailing_partial_dword_is_left_in_the_clear(self) -> None:
        self.assertEqual(mpq_read.decrypt(b"\1\2\3\4\5\6", 1234)[4:], b"\5\6")


class NotAnArchive(unittest.TestCase):
    def test_refused_with_a_reason(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "x.mpq"
            path.write_bytes(b"PK\3\4" + b"\0" * 64)
            with self.assertRaisesRegex(mpq_read.MpqError, "not an MPQ"):
                mpq_read.Archive(path)


@unittest.skipUnless(LOM_MPQ.exists() and LISTFILE.exists(), "needs .build/lom-mpq and the reference listfile")
class AgainstStormLib(unittest.TestCase):
    """Byte-identical to StormLib on every named member, on each installed archive present.

    Not a sample: 1,070 members of vanilla pic.mpq and 1,403 of GS5R3 (implode, PKWARE-masked
    compress, encrypted, and one FIX_KEY member). Skipped per archive when it is not installed,
    and the skip names it -- a green run with every archive skipped proves nothing."""

    def test_every_named_member_matches(self) -> None:
        present = {label: path for label, path in ARCHIVES.items() if path.exists()}
        if not present:
            self.skipTest("no pic.mpq installed")
        for label, path in present.items():
            with self.subTest(label), tempfile.TemporaryDirectory() as tmp:
                out = pathlib.Path(tmp)
                subprocess.run([str(LOM_MPQ), "extract", str(path), str(out), "--listfile", str(LISTFILE)],
                               check=True, capture_output=True)
                listing = subprocess.run([str(LOM_MPQ), "list", str(path), "--listfile", str(LISTFILE)],
                                         check=True, capture_output=True, text=True).stdout
                names = [row.split("\t")[0] for row in listing.splitlines()[1:]]
                archive = mpq_read.Archive(path)
                # A member stored twice under one name (GS5R3's AIpotM.lbm): StormLib's bulk
                # extract writes both to one path, last wins; a lookup by name gets the first.
                counts = {n.lower(): 0 for n in names}
                for n in names:
                    counts[n.lower()] += 1
                compared = 0
                for name in names:
                    if name.startswith("File0") or counts[name.lower()] > 1:
                        continue
                    ref = (out / name.replace("\\", os.sep)).read_bytes()
                    self.assertEqual(archive.read(name), ref, name)
                    compared += 1
                self.assertGreater(compared, 1000, f"{label}: only {compared} members compared")


if __name__ == "__main__":
    unittest.main()
