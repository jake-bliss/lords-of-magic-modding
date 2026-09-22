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

    # --- states a real player reaches (Claude review of 11d5d5f) --------------------------------

    def test_steam_restoring_the_original_neither_blocks_uninstall_nor_reinstall(self) -> None:
        """'Verify integrity' puts the shipped ddraw.dll back while the record and backup remain."""
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.uninstall(self.game)
        self.assertEqual(self.dll(), ORIGINAL)
        self.assertEqual(sorted(p.name for p in self.game.iterdir()), ["ddraw.dll"])

        setup.install(self.game, PACK, self.record)
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        self.assertEqual(self.dll(), OURS)
        setup.uninstall(self.game)
        self.assertEqual(self.dll(), ORIGINAL)

    def test_steam_restoring_the_original_after_the_backup_was_lost_backs_it_up_again(self) -> None:
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        (self.game / setup.BACKUP_NAME).unlink()
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        setup.uninstall(self.game)
        self.assertEqual(self.dll(), ORIGINAL)

    def test_an_install_interrupted_after_the_backup_is_finished_by_the_next_run(self) -> None:
        """Game running on Windows: the backup is taken, then the copy fails."""
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        (self.game / setup.BACKUP_NAME).write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        self.assertEqual(self.dll(), OURS)
        setup.uninstall(self.game)
        self.assertEqual(self.dll(), ORIGINAL)

    def test_our_dll_copied_but_no_record_is_still_undoable(self) -> None:
        (self.game / "ddraw.dll").write_bytes(OURS)
        (self.game / setup.BACKUP_NAME).write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        setup.uninstall(self.game)
        self.assertEqual(self.dll(), ORIGINAL)

    def test_an_interrupted_uninstall_can_be_run_again(self) -> None:
        """The original was copied back; the backup and record were not yet removed."""
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.uninstall(self.game)
        self.assertEqual(sorted(p.name for p in self.game.iterdir()), ["ddraw.dll"])

    def test_upgrading_from_an_older_release_keeps_the_original_backup(self) -> None:
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        older = dict(self.record, ddraw_sha256=hashlib.sha256(b"old overlay").hexdigest())
        setup.install(self.game, PACK, self.record)
        # pretend the installed one is an older release's
        (self.game / "ddraw.dll").write_bytes(b"old overlay")
        record = json.loads((self.game / setup.RECORD_NAME).read_text())
        record["ddraw_sha256"] = older["ddraw_sha256"]
        (self.game / setup.RECORD_NAME).write_text(json.dumps(record))

        setup.install(self.game, PACK, self.record)
        self.assertEqual(self.dll(), OURS)
        setup.uninstall(self.game)
        self.assertEqual(self.dll(), ORIGINAL)

    def test_the_record_exists_before_our_dll_does(self) -> None:
        """So an interruption during the copy leaves something uninstall can act on."""
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        real_replace = setup.os.replace
        def replace_but_fail_on_the_dll(src, dst):
            if pathlib.Path(dst).name == "ddraw.dll":
                raise PermissionError("locked")
            return real_replace(src, dst)
        setup.os.replace = replace_but_fail_on_the_dll
        try:
            with self.assertRaises(PermissionError):
                setup.install(self.game, PACK, self.record)
        finally:
            setup.os.replace = real_replace
        self.assertEqual(self.dll(), ORIGINAL, "a failed replace must leave the original whole")
        self.assertTrue((self.game / setup.RECORD_NAME).exists())
        setup.uninstall(self.game)
        self.assertEqual(self.dll(), ORIGINAL)
        self.assertEqual(sorted(p.name for p in self.game.iterdir()), ["ddraw.dll"])

    def test_a_damaged_record_is_recovered_from_the_backup(self) -> None:
        """A crash mid-write once left an empty record, and both commands died on JSONDecodeError."""
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        (self.game / setup.RECORD_NAME).write_text("{\"ddraw_sha")
        with self.assertRaises(SystemExit):
            setup.uninstall(self.game)              # says to re-run install; removes nothing
        self.assertEqual(self.dll(), OURS)
        setup.install(self.game, PACK, self.record)
        setup.uninstall(self.game)
        self.assertEqual(self.dll(), ORIGINAL)
        self.assertEqual(sorted(p.name for p in self.game.iterdir()), ["ddraw.dll"])


if __name__ == "__main__":
    unittest.main()
