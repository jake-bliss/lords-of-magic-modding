"""Tests for the sprite export's stale-export guard."""

import stat
import struct
import sys
import tempfile
import textwrap
import unittest
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools" / "hd-review"))

import sprite_originals  # noqa: E402


def chunk(kind: bytes, payload: bytes) -> bytes:
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))


def one_pixel_png(slots: bytes) -> bytes:
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 3, 0, 0, 0))
            + chunk(b"PLTE", slots + b"\x10\x20\x30") + chunk(b"IDAT", zlib.compress(b"\x00\x02"))
            + chunk(b"IEND", b""))


class DiscardStaleTest(unittest.TestCase):
    def setUp(self) -> None:
        self.out = Path(tempfile.mkdtemp())
        for folder in ("original", "ultrasharp", "anime2x"):
            (self.out / folder).mkdir()
            (self.out / folder / "sprite__tree2b.png").write_bytes(b"old")
            (self.out / folder / "portrait__hero.png").write_bytes(b"keep")

    def stamp(self, text: str) -> None:
        (self.out / "original" / sprite_originals.STAMP).write_text(text)

    def test_an_unstamped_export_is_removed_with_its_upscales(self) -> None:
        self.assertEqual(sprite_originals.discard_stale(self.out), 3)
        self.assertEqual(list(self.out.glob("*/sprite__*.png")), [])
        self.assertEqual(len(list(self.out.glob("*/portrait__*.png"))), 3)

    def test_a_current_export_is_kept(self) -> None:
        self.stamp(sprite_originals.EXPORT_VERSION + "\n")
        self.assertEqual(sprite_originals.discard_stale(self.out), 0)
        self.assertTrue((self.out / "original" / "sprite__tree2b.png").exists())

    def test_an_older_stamp_is_stale(self) -> None:
        self.stamp("imp-brg-2026-09-17\n")
        self.assertEqual(sprite_originals.discard_stale(self.out), 3)

    def test_folders_that_are_not_the_reviews_are_left_alone(self) -> None:
        (self.out / "notes").mkdir()
        (self.out / "notes" / "sprite__tree2b.png").write_bytes(b"not ours")
        elsewhere = Path(tempfile.mkdtemp())
        (elsewhere / "sprite__tree2b.png").write_bytes(b"not ours either")
        (self.out / "anime4x").symlink_to(elsewhere, target_is_directory=True)
        sprite_originals.discard_stale(self.out)
        self.assertTrue((self.out / "notes" / "sprite__tree2b.png").exists())
        self.assertTrue((elsewhere / "sprite__tree2b.png").exists())


class ViewerCheckTest(unittest.TestCase):
    """A stand-in viewer that exports a one-pixel PNG with the given palette slots 0 and 1."""

    def viewer(self, slots: bytes, fail_first: int = 0) -> Path:
        folder = Path(tempfile.mkdtemp())
        png = folder / "export.png"
        png.write_bytes(one_pixel_png(slots))
        counter = folder / "count"
        script = folder / "viewer"
        script.write_text(textwrap.dedent(f"""\
            #!{sys.executable}
            import pathlib, shutil, sys
            counter = pathlib.Path({str(counter)!r})
            n = int(counter.read_text()) if counter.exists() else 0
            counter.write_text(str(n + 1))
            if n < {fail_first}:
                sys.exit(1)
            shutil.copy({str(png)!r}, sys.argv[5])
            """))
        script.chmod(script.stat().st_mode | stat.S_IXUSR)
        return script

    def check(self, viewer: Path) -> bool:
        return sprite_originals.viewer_decodes_bgr(viewer, Path("imp.mpq"), ["a.imp", "b.imp"],
                                                   Path("list.txt"))

    def test_green_then_red_is_the_fixed_decode(self) -> None:
        self.assertTrue(self.check(self.viewer(b"\x00\xff\x00\xff\x00\x00")))

    def test_red_then_green_is_the_old_swapped_decode(self) -> None:
        self.assertFalse(self.check(self.viewer(b"\xff\x00\x00\x00\xff\x00")))

    def test_a_failed_export_moves_on_to_the_next_sprite(self) -> None:
        self.assertTrue(self.check(self.viewer(b"\x00\xff\x00\xff\x00\x00", fail_first=1)))

    def test_no_decisive_export_refuses_rather_than_guessing(self) -> None:
        with self.assertRaises(SystemExit):
            self.check(self.viewer(b"\x00\x00\x00\x08\x08\x08"))


if __name__ == "__main__":
    unittest.main()
