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

    def test_the_ddraw_ini_cnc_ddraw_creates_is_set_aside_never_deleted(self) -> None:
        """Windows Steam installs ship no ddraw.dll or ddraw.ini; cnc-ddraw writes an ini on its
        first run, which the player may then tune. Uninstall moves it out of the game's way and
        keeps the bytes. (Codex review: deleting it could destroy a player's settings.)"""
        setup.install(self.game, PACK, self.record)
        (self.game / "ddraw.ini").write_text("[ddraw]\nrenderer=auto\nmaxfps=144\n")
        setup.install(self.game, PACK, self.record)                          # a re-run keeps had_ini
        setup.uninstall(self.game)
        self.assertEqual(sorted(p.name for p in self.game.iterdir()), ["ddraw.ini.lomhd-saved"])
        self.assertEqual((self.game / "ddraw.ini.lomhd-saved").read_text(),
                         "[ddraw]\nrenderer=auto\nmaxfps=144\n")

    def test_an_earlier_set_aside_ini_is_not_overwritten(self) -> None:
        (self.game / "ddraw.ini.lomhd-saved").write_text("older")
        setup.install(self.game, PACK, self.record)
        (self.game / "ddraw.ini").write_text("newer")
        setup.uninstall(self.game)
        self.assertEqual((self.game / "ddraw.ini.lomhd-saved").read_text(), "older")
        self.assertEqual((self.game / "ddraw.ini.lomhd-saved2").read_text(), "newer")

    def test_a_players_own_ddraw_ini_is_kept(self) -> None:
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        (self.game / "ddraw.ini").write_text("[ddraw]\nrenderer=opengl\n")
        setup.install(self.game, PACK, self.record)
        setup.uninstall(self.game)
        self.assertEqual((self.game / "ddraw.ini").read_text(), "[ddraw]\nrenderer=opengl\n")


class UpscalePlan(unittest.TestCase):
    """upscale_all with the model stubbed: what each image is upscaled FROM."""

    def setUp(self) -> None:
        import struct
        import lbm_png
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.work = pathlib.Path(tmp.name) / "lomhd_work"
        for name, value in (("WORK", self.work), ("say", lambda text: None)):
            self.addCleanup(setattr, setup, name, getattr(setup, name))
            setattr(setup, name, value)
        self.palette = [(i, i, i) for i in range(256)]
        self.lbm_png, self.struct = lbm_png, struct
        self.seen: dict[str, bytes] = {}

        def render(option, inputs, dest, esrgan, models):
            for key, png in inputs.items():
                self.seen[key] = png.read_bytes()
            return len(inputs)

        def run(cmd, check=False):         # magick PPM -> PNG, stubbed as a copy
            pathlib.Path(cmd[2]).write_bytes(pathlib.Path(cmd[1]).read_bytes())

        for target, name, value in ((setup.hd_upscale, "render", render), (setup.subprocess, "run", run)):
            self.addCleanup(setattr, target, name, getattr(target, name))
            setattr(target, name, value)

    def extract(self, shade: int) -> None:
        folder = self.work / "originals" / "building"
        folder.mkdir(parents=True, exist_ok=True)
        header = self.struct.pack(">HHhhBBBBHBBhh", 40, 6, 0, 0, 8, 0, 1, 0, 0, 1, 1, 40, 6)
        self.lbm_png.encode(folder / "aagtwr0a.lbm", 40, 6, bytes([shade]) * 240, self.palette,
                            [(b"BMHD", header), (b"CMAP", b""), (b"BODY", b"")])

    def test_a_second_run_upscales_this_install_not_the_last_one(self) -> None:
        """Vanilla then GS5R3: a name both share, a different picture. The PNG cache kept the
        vanilla picture, and the pack could not tell, since it checks against this run's art."""
        self.extract(10)
        setup.upscale_all({"building": ["aagtwr0a"]}, pathlib.Path("esrgan"), pathlib.Path("models"))
        first = self.seen.pop("aagtwr0a")
        self.extract(200)
        setup.upscale_all({"building": ["aagtwr0a"]}, pathlib.Path("esrgan"), pathlib.Path("models"))
        self.assertNotEqual(self.seen["aagtwr0a"], first)
        self.assertIn(bytes([200, 200, 200]), self.seen["aagtwr0a"])

    def test_images_the_overlay_would_refuse_are_skipped_before_upscaling(self) -> None:
        """A mod install's oversized building must not cost 20-60 minutes of upscaling and then
        stop the pack writer; extraction leaves it out."""
        import io
        members = {}
        for name, (w, h) in {"portrait\\aicavp00.lbm": (70, 67), "lbm\\building\\aagtwr0a.lbm": (228, 180),
                             "lbm\\building\\huge.lbm": (641, 67), "lbm\\building\\flat.lbm": (70, 3),
                             "lbm\\plain.lbm": (640, 480)}.items():
            buf = io.BytesIO()
            header = self.struct.pack(">HHhhBBBBHBBhh", w, h, 0, 0, 8, 0, 1, 0, 0, 1, 1, w, h)
            path = self.work.parent / "member.lbm"
            pixels = bytes(w * h) if "plain" in name else bytes(i % 251 for i in range(w * h))
            self.lbm_png.encode(path, w, h, pixels, self.palette,
                                [(b"BMHD", header), (b"CMAP", b""), (b"BODY", b"")])
            members[name] = path.read_bytes()

        class Archive:
            def __init__(self, path): pass
            def listfile(self): return list(members)
            def __contains__(self, name): return name in members
            def read(self, name): return members[name]

        self.addCleanup(setattr, setup.mpq_read, "Archive", setup.mpq_read.Archive)
        setup.mpq_read.Archive = Archive
        found = setup.extract_images(self.work.parent)
        self.assertEqual({g: v for g, v in found.items() if v},
                         {"portrait": ["aicavp00"], "building": ["aagtwr0a"]},
                         "too big, too short and too plain (a flat 640x480) are all left out")

    def test_the_players_own_picks_win_over_the_shipped_ones(self) -> None:
        """--review saves to my-upscale-choices.json; a plain run must install with it, and
        deleting it must fall back to the shipped picks."""
        import json
        mine, shipped = self.work.parent / "mine.json", self.work.parent / "shipped.json"
        shipped.write_text(json.dumps({"choices": {"building__aagtwr0a": "anime2x"}}))
        for name, value in (("MY_CHOICES", mine), ("SHIPPED_CHOICES", shipped)):
            self.addCleanup(setattr, setup, name, getattr(setup, name))
            setattr(setup, name, value)
        options = []
        setup.hd_upscale.render = lambda option, inputs, *rest: options.append(option)
        self.extract(10)
        setup.upscale_all({"building": ["aagtwr0a"]}, pathlib.Path("esrgan"), pathlib.Path("models"))
        mine.write_text(json.dumps({"choices": {"building__aagtwr0a": "anime4x"}}))
        setup.upscale_all({"building": ["aagtwr0a"]}, pathlib.Path("esrgan"), pathlib.Path("models"))
        mine.unlink()
        setup.upscale_all({"building": ["aagtwr0a"]}, pathlib.Path("esrgan"), pathlib.Path("models"))
        self.assertEqual(options, ["anime2x", "anime4x", "anime2x"])


if __name__ == "__main__":
    unittest.main()
