"""Tests for the sprite export's stale-cache guard."""

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools" / "hd-review"))

import sprite_originals  # noqa: E402


class DiscardStaleTest(unittest.TestCase):
    def setUp(self) -> None:
        self.out = Path(tempfile.mkdtemp())
        for folder in ("original", "ultrasharp", "anime2x"):
            (self.out / folder).mkdir()
            (self.out / folder / "sprite__tree2b.png").write_bytes(b"old")
            (self.out / folder / "portrait__hero.png").write_bytes(b"keep")

    def test_an_unstamped_export_is_removed_with_its_upscales(self) -> None:
        self.assertEqual(sprite_originals.discard_stale(self.out), 3)
        self.assertEqual(list(self.out.glob("*/sprite__*.png")), [])
        self.assertEqual(len(list(self.out.glob("*/portrait__*.png"))), 3)

    def test_a_current_export_is_kept(self) -> None:
        sprite_originals.discard_stale(self.out)
        (self.out / "original" / "sprite__tree2b.png").write_bytes(b"new")
        self.assertEqual(sprite_originals.discard_stale(self.out), 0)
        self.assertTrue((self.out / "original" / "sprite__tree2b.png").exists())

    def test_an_older_stamp_is_stale(self) -> None:
        (self.out / "original" / sprite_originals.STAMP).write_text("imp-brg-2026-09-17\n")
        self.assertEqual(sprite_originals.discard_stale(self.out), 3)


if __name__ == "__main__":
    unittest.main()
