"""The HD overlay setup's install and uninstall: the player's ddraw.dll must always come back.

These drive `install`/`uninstall` directly against a temporary game folder. The extraction and
upscaling steps are covered by test_mpq_read.py and the end-to-end run in docs/hd-overlay.md."""

from __future__ import annotations

import hashlib
import json
import pathlib
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "tools" / "portrait-upscale"))
sys.path.insert(0, str(ROOT / "release" / "hd-overlay"))

import lomhd_setup as setup  # noqa: E402

ORIGINAL = b"the player's own ddraw.dll"
OURS = b"the overlay's ddraw.dll"
PACK = b"LOMHDPK1 pretend pack"


class InstallUninstall(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        self.game, self.release_dir = root / "game", root / "release"
        self.game.mkdir(); self.release_dir.mkdir()
        (self.release_dir / "ddraw.dll").write_bytes(OURS)
        self.record = {"version": "test", "ddraw_sha256": hashlib.sha256(OURS).hexdigest()}
        self._here = setup.HERE
        setup.HERE = self.release_dir
        self.addCleanup(setattr, setup, "HERE", self._here)

    def dll(self) -> bytes:
        return (self.game / "ddraw.dll").read_bytes()

    def test_install_then_uninstall_restores_the_original_exactly(self) -> None:
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        self.assertEqual(self.dll(), OURS)
        self.assertEqual((self.game / setup.BACKUP_NAME).read_bytes(), ORIGINAL)
        self.assertEqual((self.game / setup.PACK_NAME).read_bytes(), PACK)

        setup.uninstall(self.game)
        self.assertEqual(self.dll(), ORIGINAL)
        self.assertEqual(sorted(p.name for p in self.game.iterdir()), ["ddraw.dll"])

    def test_installing_twice_keeps_the_first_backup(self) -> None:
        """The second run finds OUR dll installed; backing that up would lose the original."""
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        setup.install(self.game, PACK + b"2", self.record)
        self.assertEqual((self.game / setup.BACKUP_NAME).read_bytes(), ORIGINAL)
        setup.uninstall(self.game)
        self.assertEqual(self.dll(), ORIGINAL)

    def test_a_game_without_a_ddraw_dll_gets_it_removed_again(self) -> None:
        setup.install(self.game, PACK, self.record)
        self.assertFalse((self.game / setup.BACKUP_NAME).exists())
        setup.uninstall(self.game)
        self.assertEqual(list(self.game.iterdir()), [])

    def test_a_dll_replaced_by_another_mod_is_left_alone(self) -> None:
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        (self.game / "ddraw.dll").write_bytes(b"someone else's dll")
        with self.assertRaises(SystemExit):
            setup.install(self.game, PACK, self.record)
        with self.assertRaises(SystemExit):
            setup.uninstall(self.game)
        self.assertEqual(self.dll(), b"someone else's dll")
        self.assertEqual((self.game / setup.BACKUP_NAME).read_bytes(), ORIGINAL)

    def test_a_stray_backup_is_never_overwritten(self) -> None:
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        (self.game / setup.BACKUP_NAME).write_bytes(b"an older backup")
        with self.assertRaises(SystemExit):
            setup.install(self.game, PACK, self.record)
        self.assertEqual((self.game / setup.BACKUP_NAME).read_bytes(), b"an older backup")
        self.assertEqual(self.dll(), ORIGINAL)

    def test_a_changed_backup_blocks_the_uninstall_and_removes_nothing(self) -> None:
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        (self.game / setup.BACKUP_NAME).write_bytes(b"damaged")
        with self.assertRaises(SystemExit):
            setup.uninstall(self.game)
        self.assertEqual(self.dll(), OURS)
        self.assertTrue((self.game / setup.PACK_NAME).exists())
        self.assertTrue((self.game / setup.RECORD_NAME).exists())

    def test_the_record_names_what_was_installed(self) -> None:
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        record = json.loads((self.game / setup.RECORD_NAME).read_text())
        self.assertEqual(record["backup_sha256"], hashlib.sha256(ORIGINAL).hexdigest())
        self.assertEqual(record["pack_sha256"], hashlib.sha256(PACK).hexdigest())
        self.assertTrue(record["had_ddraw"])


if __name__ == "__main__":
    unittest.main()
