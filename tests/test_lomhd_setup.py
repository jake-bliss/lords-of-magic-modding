"""The HD overlay setup's install and uninstall: the player's ddraw.dll must always come back.

These drive `install`/`uninstall` directly against a temporary game folder. The extraction and
upscaling steps are covered by test_mpq_read.py and the end-to-end run in docs/hd-overlay.md."""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import os
import pathlib
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock
import types
import zipfile

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

    def test_uninstall_removes_the_last_setup_summary(self) -> None:
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, PACK, self.record)
        (self.game / setup.SUMMARY_NAME).write_text("a stored summary\n")
        (self.game / (setup.SUMMARY_NAME + ".lomhd-part")).write_text("stale partial write\n")
        setup.uninstall(self.game)
        self.assertFalse((self.game / setup.SUMMARY_NAME).exists())
        self.assertFalse((self.game / (setup.SUMMARY_NAME + ".lomhd-part")).exists())

    def test_a_pack_installed_from_a_file_arrives_whole(self) -> None:
        """Setup installs the pack from the file the writer streamed to; every other test passes
        bytes. (Claude review, 2026-09-23: copying nothing, or hashing the path, passed.)"""
        pack = self.release_dir / "made.pack"
        pack.write_bytes(PACK * 1000)
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        setup.install(self.game, pack, self.record)
        self.assertEqual((self.game / setup.PACK_NAME).read_bytes(), PACK * 1000)
        record = json.loads((self.game / setup.RECORD_NAME).read_text())
        self.assertEqual(record["pack_sha256"], hashlib.sha256(PACK * 1000).hexdigest())

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

    def test_an_upgrade_interrupted_before_the_new_dll_is_still_undoable(self) -> None:
        """The upgrade writes its record (naming the new DLL), then stops before copying the DLL: the
        OLD overlay DLL is left in place. Re-running and uninstalling must both still know it as
        ours. (Codex review, 2026-09-23.)"""
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        old = dict(self.record, ddraw_sha256=hashlib.sha256(b"old overlay").hexdigest())
        (self.release_dir / "ddraw.dll").write_bytes(b"old overlay")
        setup.install(self.game, PACK, old)                 # the older release, installed
        (self.release_dir / "ddraw.dll").write_bytes(OURS)
        real_write = setup.write_atomically

        def stop_at_the_dll(path, data):
            if path.name == "ddraw.dll":
                raise KeyboardInterrupt
            real_write(path, data)

        setup.write_atomically = stop_at_the_dll
        try:
            with self.assertRaises(KeyboardInterrupt):
                setup.install(self.game, PACK, self.record)
        finally:
            setup.write_atomically = real_write
        self.assertEqual(self.dll(), b"old overlay")
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
        # No picks but the defaults, whatever the shipped file says today.
        shipped = pathlib.Path(tmp.name) / "no-picks.json"
        shipped.write_text('{"choices": {}}')
        for name, value in (("SHIPPED_CHOICES", shipped), ("MY_CHOICES", pathlib.Path(tmp.name) / "none.json")):
            self.addCleanup(setattr, setup, name, getattr(setup, name))
            setattr(setup, name, value)
        # One at a time, so the stubs below see calls in order (LOMHD_JOBS=1 is the serial path).
        jobs = mock.patch.dict(os.environ, {setup.hd_upscale.JOBS_ENV: "1"})
        jobs.start()
        self.addCleanup(jobs.stop)
        self.palette = [(i, i, i) for i in range(256)]
        self.lbm_png, self.struct = lbm_png, struct
        self.seen: dict[str, bytes] = {}

        def render(option, inputs, dest, esrgan, models):
            for key, png in inputs.items():
                self.seen[key] = png.read_bytes()
            return len(inputs)

        def run(cmd, check=False, **kwargs):     # magick PPM -> PNG, stubbed as a copy
            pathlib.Path(cmd[2].removeprefix("PNG:")).write_bytes(pathlib.Path(cmd[1]).read_bytes())

        for target, name, value in ((setup.hd_upscale, "render", render), (setup.subprocess, "run", run)):
            self.addCleanup(setattr, target, name, getattr(target, name))
            setattr(target, name, value)

    def extract(self, shade: int, group: str = "building", stem: str = "aagtwr0a") -> None:
        folder = setup.WORK / "originals" / group
        folder.mkdir(parents=True, exist_ok=True)
        header = self.struct.pack(">HHhhBBBBHBBhh", 40, 6, 0, 0, 8, 0, 1, 0, 0, 1, 1, 40, 6)
        self.lbm_png.encode(folder / f"{stem}.lbm", 40, 6, bytes([shade]) * 240, self.palette,
                            [(b"BMHD", header), (b"CMAP", b""), (b"BODY", b"")])

    def counting_stubs(self) -> list:
        """The upscaler and every program stubbed to make real outputs, each call noted: ("magick",
        png name), (option, name) for a render, ("approved", name) for upscale.py."""
        made: list = []

        def render(option, inputs, dest, esrgan, models):
            dest.mkdir(parents=True, exist_ok=True)
            for key, png in inputs.items():
                made.append((option, key))
                (dest / f"{key}.png").write_bytes(b"upscale of " + png.read_bytes())
            return len(inputs)

        def run(cmd, check=False, **kwargs):
            if cmd[0] == "magick":                        # PPM -> PNG, as a copy
                made.append(("magick", pathlib.Path(cmd[2]).stem))
                pathlib.Path(cmd[2].removeprefix("PNG:")).write_bytes(pathlib.Path(cmd[1]).read_bytes())
            else:                                         # upscale.py: approved portraits
                originals, dest = pathlib.Path(cmd[2]), pathlib.Path(cmd[3])
                for name in pathlib.Path(cmd[cmd.index("--names") + 1]).read_text().splitlines():
                    stem = name.split("\\")[-1][:-4]
                    made.append(("approved", stem))
                    (dest / "portrait").mkdir(parents=True, exist_ok=True)
                    (dest / "portrait" / f"{stem}.lbm").write_bytes((originals / "portrait" / f"{stem}.lbm").read_bytes())
            return types.SimpleNamespace(returncode=0)

        setup.hd_upscale.render, setup.subprocess.run = render, run
        return made

    def test_a_rerun_makes_nothing_and_a_change_remakes_exactly_what_it_changed(self) -> None:
        """Until 2026-09-30 every run re-rendered every picture (33 minutes with nothing new). Now
        only what is missing is made: after a changed original, that picture; after a changed pick,
        that picture with its new option. On threads or not, the same."""
        for jobs in ("1", "4"):
            with self.subTest(jobs=jobs), mock.patch.dict(os.environ, {setup.hd_upscale.JOBS_ENV: jobs}):
                setup.WORK = self.work / f"jobs-{jobs}"
                shipped = setup.WORK / "shipped.json"
                setup.WORK.mkdir(parents=True)
                setup.SHIPPED_CHOICES = shipped
                picks = {"building__aagtwr0a": "anime2x", "building__abldg": "anime4x"}
                shipped.write_text(json.dumps({"choices": picks}))
                found = {"building": ["aagtwr0a", "abldg"], "portrait": ["aicavp00"]}
                self.extract(10)
                self.extract(20, stem="abldg")
                self.extract(30, "portrait", "aicavp00")
                made = self.counting_stubs()
                e, m = pathlib.Path("esrgan"), pathlib.Path("models")

                def rerun() -> list:
                    made.clear()
                    folders = setup.upscale_all(found, e, m)
                    self.assertEqual(sorted(p.name for folder in folders for p in folder.iterdir()),
                                     ["aagtwr0a.png", "abldg.png", "aicavp00.lbm"], "one upscale per picture")
                    return sorted(made)

                self.assertEqual(rerun(), [("anime2x", "aagtwr0a"), ("anime4x", "abldg"), ("approved", "aicavp00"),
                                           ("magick", "aagtwr0a"), ("magick", "abldg")])
                kept = (setup.WORK / "upscaled" / "anime2x" / "aagtwr0a.png").stat().st_mtime_ns
                self.assertEqual(rerun(), [], "nothing new: nothing made")
                self.extract(21, stem="abldg")
                self.assertEqual(rerun(), [("anime4x", "abldg"), ("magick", "abldg")])
                self.assertIn(bytes([21, 21, 21]), (setup.WORK / "upscaled" / "anime4x" / "abldg.png").read_bytes())
                self.extract(31, "portrait", "aicavp00")
                self.assertEqual(rerun(), [("approved", "aicavp00")])
                picks["building__aagtwr0a"] = "ultrasharp"
                shipped.write_text(json.dumps({"choices": picks}))
                self.assertEqual(rerun(), [("ultrasharp", "aagtwr0a")], "its PNG is kept; only the upscale")
                self.assertFalse((setup.WORK / "upscaled" / "anime2x" / "aagtwr0a.png").exists(),
                                 "the upscale by the old pick goes")
                self.assertTrue((setup.WORK / "png" / "aagtwr0a.png").exists())
                picks["building__aagtwr0a"] = "anime2x"
                shipped.write_text(json.dumps({"choices": picks}))
                self.assertEqual(rerun(), [("anime2x", "aagtwr0a")], "back again: made again, not revived")
                self.assertNotEqual((setup.WORK / "upscaled" / "anime2x" / "aagtwr0a.png").stat().st_mtime_ns, kept)

    def test_another_installs_pictures_leave_every_folder(self) -> None:
        """The stale-picture guard, from the other side: a picture this install does not have (a
        vanilla-only name, a removed mod's) keeps no PNG and no upscale, so the pack cannot take it
        for this game's."""
        self.extract(10)
        self.extract(20, stem="abldg")
        self.extract(30, "portrait", "aicavp00")
        self.counting_stubs()
        e, m = pathlib.Path("esrgan"), pathlib.Path("models")
        setup.upscale_all({"building": ["aagtwr0a", "abldg"], "portrait": ["aicavp00"]}, e, m)
        folders = setup.upscale_all({"building": ["aagtwr0a"], "portrait": []}, e, m)
        left = sorted(str(p.relative_to(setup.WORK)) for folder in ("upscaled", "png")
                      for p in (setup.WORK / folder).rglob("*") if p.is_file())
        self.assertEqual(left, ["png/aagtwr0a.lbm", "png/aagtwr0a.png", "upscaled/recipe.json",
                                "upscaled/ultrasharp-tta/aagtwr0a.png"])
        self.assertEqual([f.name for f in folders], ["ultrasharp-tta"])

    def kept_until(self, change) -> list:
        """What a rerun makes after `change()`, with the upscale code pinned to a file of its own
        (so only `change` can move the recipe), after a control rerun that makes nothing."""
        self.extract(10)
        made = self.counting_stubs()
        e, m = pathlib.Path("esrgan"), pathlib.Path("models")
        code = self.work.parent / "upscale.py"
        code.write_text("the pipeline\n")
        with mock.patch.object(setup, "upscale_code", lambda: [code]):
            setup.upscale_all({"building": ["aagtwr0a"]}, e, m)
            made.clear()
            setup.upscale_all({"building": ["aagtwr0a"]}, e, m)
            self.assertEqual(made, [], "the control: nothing changed, nothing made")
            with change(code):
                setup.upscale_all({"building": ["aagtwr0a"]}, e, m)
        return sorted(made)

    def test_a_change_to_the_upscale_code_makes_everything_again_pngs_too(self) -> None:
        """Kept upscales are a release's, and so are the PNGs they are made from (lbm_to_png):
        a changed pipeline remakes both, with no number to remember to bump."""
        @contextlib.contextmanager
        def edited(code):
            code.write_text("the pipeline, changed\n")
            yield

        self.assertEqual(self.kept_until(edited), [("magick", "aagtwr0a"), ("ultrasharp-tta", "aagtwr0a")])

    def test_a_changed_option_makes_everything_again(self) -> None:
        change = lambda code: mock.patch.dict(setup.hd_upscale.OPTIONS,  # noqa: E731
                                              {"ultrasharp-tta": ("another-model", 4, ["-x"])})
        self.assertEqual(self.kept_until(change), [("magick", "aagtwr0a"), ("ultrasharp-tta", "aagtwr0a")])

    def test_a_changed_model_pin_makes_everything_again(self) -> None:
        url, _ = setup.DOWNLOADS["ultrasharp-4x.bin"]
        change = lambda code: mock.patch.dict(setup.DOWNLOADS, {"ultrasharp-4x.bin": (url, "0" * 64)})  # noqa: E731
        self.assertEqual(self.kept_until(change), [("magick", "aagtwr0a"), ("ultrasharp-tta", "aagtwr0a")])

    def test_the_upscale_code_is_the_files_that_make_upscales(self) -> None:
        import inspect
        code = setup.upscale_code()
        self.assertEqual([p.name for p in code], ["hd_upscale.py", "lbm_png.py", "upscale.py"])
        self.assertTrue(all(p.is_file() for p in code), code)
        self.assertIn(pathlib.Path(inspect.getsourcefile(setup.hd_upscale.lbm_to_png)).resolve(), code,
                      "the PNGs the upscales are made from are made by hashed code too")
        # And setup calls that copy: a helper of its own would sit in unhashed lomhd_setup.py.
        self.assertFalse(hasattr(setup, "lbm_to_png"), "setup must use hd_upscale.lbm_to_png")

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
                             "lbm\\plain.lbm": (640, 480), "lbm\\start01.lbm": (640, 480),
                             "portrait\\black.lbm": (70, 67), "lbm\\black.lbm": (640, 480)}.items():
            buf = io.BytesIO()
            header = self.struct.pack(">HHhhBBBBHBBhh", w, h, 0, 0, 8, 0, 1, 0, 0, 1, 1, w, h)
            path = self.work.parent / "member.lbm"
            pixels = bytes(w * h) if "plain" in name else bytes((i * 7 + w) % 251 for i in range(w * h))
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
                         {"portrait": ["aicavp00", "black"], "building": ["aagtwr0a"], "screen": ["start01"]},
                         "too big, too short and too plain are left out; a real screen is kept; of two "
                         "pictures named black, the portrait is kept and the screen left out")
        kept = setup.WORK / "originals" / "building" / "aagtwr0a.lbm"
        before = kept.stat().st_mtime_ns
        time.sleep(0.01)
        del members["lbm\\start01.lbm"]
        found = setup.extract_images(self.work.parent)
        self.assertEqual(sorted(str(p.relative_to(setup.WORK / "originals")).replace("\\", "/")
                                for p in (setup.WORK / "originals").rglob("*") if p.is_file()),
                         ["building/aagtwr0a.lbm", "portrait/aicavp00.lbm", "portrait/black.lbm"],
                         "a rerun keeps exactly this install's pictures: the gone screen is removed")
        self.assertEqual(kept.stat().st_mtime_ns, before, "an unchanged picture is not written again")

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

    def test_review_renders_survive_a_rerun_and_go_when_the_picture_changes(self) -> None:
        """--review resumes: an unchanged picture keeps its renders; a changed one loses them."""
        setup.hd_upscale.render = lambda *a, **k: 0
        self.extract(10)
        found = {"building": ["aagtwr0a"]}
        review = setup.render_review(found, pathlib.Path("esrgan"), pathlib.Path("models"))
        render = review / "anime2x" / "building__aagtwr0a.png"
        render.parent.mkdir(parents=True, exist_ok=True)
        render.write_bytes(b"a finished render")
        setup.render_review(found, pathlib.Path("esrgan"), pathlib.Path("models"))
        self.assertTrue(render.exists(), "an unchanged picture must keep its renders")
        self.assertEqual(sorted(p.name for p in (review / "original").iterdir()),
                         ["building__aagtwr0a.lbm", "building__aagtwr0a.png"], "no stray temp files")
        self.extract(200)
        setup.render_review(found, pathlib.Path("esrgan"), pathlib.Path("models"))
        self.assertFalse(render.exists(), "a changed picture must lose its old renders")
        setup.render_review({"building": []}, pathlib.Path("esrgan"), pathlib.Path("models"))
        self.assertEqual(list((review / "original").iterdir()), [],
                         "a picture this install does not have leaves the page")


# --- sprites ---------------------------------------------------------------------------------------

class WindowsMagick(unittest.TestCase):
    """A console keeps the PATH it opened with, so right after `winget install ImageMagick` setup
    must find magick.exe itself (a tester was told it was missing until they reopened the console)."""

    def setUp(self) -> None:
        self.tmp = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.registry: "dict[str, str]" = {}
        fake = types.ModuleType("winreg")
        fake.HKEY_LOCAL_MACHINE, fake.HKEY_CURRENT_USER = "HKLM", "HKCU"

        class Key:
            def __init__(self, root: str) -> None:
                self.root = root

            def __enter__(self) -> "Key":
                if self.root not in registry:
                    raise OSError("no such key")
                return self

            def __exit__(self, *exc: object) -> None:
                return None

        registry = self.registry
        fake.OpenKey = lambda root, key: Key(root)
        fake.QueryValueEx = lambda handle, name: (registry[handle.root], 2)
        patcher = mock.patch.dict(sys.modules, {"winreg": fake})
        patcher.start()
        self.addCleanup(patcher.stop)

    def install(self, folder: str) -> pathlib.Path:
        path = self.tmp / folder
        path.mkdir(parents=True)
        (path / "magick.exe").write_bytes(b"")
        return path

    def test_a_folder_only_in_the_registry_path_is_found(self) -> None:
        where = self.install("Apps/ImageMagick-7.1.2-Q16-HDRI")
        self.registry["HKLM"] = f"C:\\Windows;{where}"
        with mock.patch.dict(os.environ, {"ProgramFiles": str(self.tmp / "none")}):
            self.assertEqual(setup.windows_magick_dirs(), [str(where)])

    def test_the_default_install_folder_is_found_without_any_path_entry(self) -> None:
        where = self.install("Program Files/ImageMagick-7.1.2-Q16-HDRI")
        with mock.patch.dict(os.environ, {"ProgramFiles": str(self.tmp / "Program Files")}):
            self.assertIn(str(where), setup.windows_magick_dirs())

    def test_a_quoted_registry_entry_is_found(self) -> None:
        where = self.install("Tools/ImageMagick 7")
        self.registry["HKCU"] = f'"{where}" ; C:\\Windows'
        with mock.patch.dict(os.environ, {"ProgramFiles": str(self.tmp / "none")}):
            self.assertEqual(setup.windows_magick_dirs(), [str(where)])

    def test_check_magick_puts_the_found_folder_on_this_process_path(self) -> None:
        """The fix itself: later bare `magick` calls (here and in tools/) inherit this PATH."""
        answers = iter(["", "Version: ImageMagick 7.1.2-0 Q16-HDRI"])
        with mock.patch.object(setup.os, "name", "nt"), \
                mock.patch.object(setup, "windows_magick_dirs", return_value=["X"]), \
                mock.patch.object(setup, "magick_version", side_effect=lambda: next(answers)), \
                mock.patch.object(setup, "fail", side_effect=AssertionError("reported missing")), \
                mock.patch.dict(os.environ, {"PATH": "orig"}):
            setup.check_magick()
            self.assertEqual(os.environ["PATH"], os.pathsep.join(["X", "orig"]))

    def test_a_path_entry_without_magick_exe_is_ignored(self) -> None:
        (self.tmp / "ImageMagick-old").mkdir()
        self.registry["HKCU"] = str(self.tmp / "ImageMagick-old")
        with mock.patch.dict(os.environ, {"ProgramFiles": str(self.tmp / "none")}):
            self.assertEqual(setup.windows_magick_dirs(), [])


class RetryCommand(unittest.TestCase):
    """The command a failed run tells the player to type. It must repeat this run's choices: the
    install record is written only when a run finishes, so a bare retry falls back to the last
    install's sprite mode. (Claude + Codex review.)"""

    def command(self, animated: bool, **flags: object) -> str:
        args = argparse.Namespace(**{"sprites": False, "no_sprites": False, "terrain": False,
                                     "force_terrain_folder": False, "game": None, **flags})
        return setup.retry_command(args, animated, pathlib.Path("C:/Games/LOM"))

    def test_no_sprites_is_repeated_so_the_remembered_mode_cannot_come_back(self) -> None:
        self.assertIn("--no-sprites", self.command(False, no_sprites=True).split())

    def test_a_remembered_animated_run_is_spelled_out(self) -> None:
        self.assertIn("--sprites", self.command(True).split())

    def test_every_other_flag_is_repeated(self) -> None:
        line = self.command(False, terrain=True, force_terrain_folder=True, game="C:/Games/LOM")
        for flag in ("--terrain", "--force-terrain-folder", "--game"):
            self.assertIn(flag, line.split())

    def test_a_plain_static_run_stays_plain(self) -> None:
        self.assertEqual(self.command(False), "python lomhd_setup.py")


class Sprites(unittest.TestCase):
    """plan_sprites / build_pack against a fake imp.mpq of tiny real IMP files, the upscaler stubbed
    to write real PNGs at 2x."""

    def setUp(self) -> None:
        sys.path.insert(0, str(ROOT / "tests"))
        from test_hd_sprites import frame, imp_file
        import hd_sprites
        self.frame, self.imp_file, self.hd_sprites = frame, imp_file, hd_sprites
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.base = pathlib.Path(tmp.name)
        self.game = self.base / "game"
        self.game.mkdir()
        (self.game / "imp.mpq").write_bytes(b"one imp.mpq")
        self.names = self.base / "imp-names.txt"
        self.shipped, self.mine = self.base / "shipped.json", self.base / "mine.json"
        for name, value in (("WORK", self.base / "lomhd_work"), ("say", lambda text: None),
                            ("IMP_NAMES", self.names), ("SHIPPED_CHOICES", self.shipped),
                            ("MY_CHOICES", self.mine)):
            self.addCleanup(setattr, setup, name, getattr(setup, name))
            setattr(setup, name, value)
        self.members: dict[str, bytes] = {}
        members = self.members

        class Archive:
            def __init__(self, path): pass
            def __contains__(self, name): return name.lower() in members
            def read(self, name): return members[name.lower()]

        self.addCleanup(setattr, setup.mpq_read, "Archive", setup.mpq_read.Archive)
        setup.mpq_read.Archive = Archive
        self.rendered: list[str] = []

        def render(option, inputs, dest, esrgan, models):
            dest.mkdir(parents=True, exist_ok=True)
            for key, src in inputs.items():
                self.rendered.append(key)
                w, h = setup.hd_upscale.png_size(src)
                hd_sprites.write_png_rgba(dest / f"{key}.png", w * 2, h * 2, bytes([9, 9, 9, 255]) * (w * h * 4))
            return len(inputs)

        self.addCleanup(setattr, setup.hd_upscale, "render", setup.hd_upscale.render)
        setup.hd_upscale.render = render

    def add(self, member: str, *frames, origins=None) -> None:
        self.members[member.lower()] = self.imp_file(list(frames), origins=origins)
        self.names.write_text("".join(f"{m}\n" for m in self.members))

    def picks(self, shipped: dict, mine: dict | None = None) -> None:
        self.shipped.write_text(json.dumps({"choices": shipped}))
        if mine is not None:
            self.mine.write_text(json.dumps({"choices": mine}))

    def test_the_players_sprite_picks_win_per_sprite(self) -> None:
        """A my-upscale-choices.json from before sprites were built has no sprite__ keys; it must
        not take the shipped sprite picks away (choices_file swaps the whole file for pictures)."""
        self.picks({"sprite__a": "anime2x", "sprite__b": "anime2x", "building__x": "ultrasharp"},
                   {"sprite__b": "anime4x", "building__x": "anime2x"})
        self.assertEqual(setup.sprite_choices(), {"sprite__a": "anime2x", "sprite__b": "anime4x"})
        self.mine.write_text(json.dumps({"choices": {"building__x": "anime2x"}}))
        self.assertEqual(setup.sprite_choices(), {"sprite__a": "anime2x", "sprite__b": "anime2x"})
        self.add("imp\\b.imp", self.frame(20, 6, 3))
        self.mine.write_text(json.dumps({"choices": {"sprite__b": "anime4x"}}))
        plan, _, _ = setup.plan_sprites(self.game, animated=False)
        self.assertEqual([(s.name, s.option) for s in plan.static], [("b", "anime4x")])

    def renders_of(self, root: pathlib.Path, stem_prefix: str) -> list:
        return sorted(p.name for p in (root / "render").rglob("*.png") if p.name.startswith(stem_prefix))

    def test_a_static_only_run_keeps_the_animated_renders(self) -> None:
        """A --no-sprites run (or a plain one before sprites were remembered) plans no animated
        sprite -- and must not prune the hours of renders a --sprites run made for them."""
        self.picks({"sprite__cav": "anime2x", "sprite__one": "anime2x"})
        self.add("units\\\\cav.imp", self.frame(20, 6, 3), self.frame(20, 6, 4))
        self.add("imp\\\\one.imp", self.frame(20, 6, 5))
        plan, root, _ = setup.plan_sprites(self.game, animated=True)
        setup.upscale_sprites(plan.static + plan.animated, root, pathlib.Path("e"), pathlib.Path("m"))
        cav = plan.animated[0].frames[0].stem.split("__")[0]
        before = self.renders_of(root, cav)
        self.assertEqual(len(before), 2)
        plan, root, _ = setup.plan_sprites(self.game, animated=False)
        self.assertEqual(plan.animated, [])
        self.assertEqual(self.renders_of(root, cav), before)

    def test_present_members_left_out_this_run_keep_their_renders(self) -> None:
        """Ambiguous (two members, one name) or undecodable today: still in imp.mpq, so their
        work is kept for when they resolve again -- not pruned as if a mod had removed them."""
        self.picks({"sprite__tree": "anime2x"})
        self.add("imp\\\\tree.imp", self.frame(20, 6, 3))
        plan, root, _ = setup.plan_sprites(self.game, animated=False)
        setup.upscale_sprites(plan.static, root, pathlib.Path("e"), pathlib.Path("m"))
        tree = plan.static[0].frames[0].stem.split("__")[0]
        self.add("aura\\\\tree.imp", self.frame(20, 6, 7))                 # now ambiguous
        plan, root, _ = setup.plan_sprites(self.game, animated=False)
        self.assertEqual(plan.static, [])
        self.assertIn("ambiguous", plan.skipped[0])
        self.assertEqual(len(self.renders_of(root, tree)), 1)
        del self.members["aura\\\\tree.imp"]
        good = self.members["imp\\\\tree.imp"]
        self.members["imp\\\\tree.imp"] = good[:40]                         # present, undecodable
        plan, root, _ = setup.plan_sprites(self.game, animated=False)
        self.assertIn("could not read", plan.skipped[0])
        self.assertEqual(len(self.renders_of(root, tree)), 0,
                         "its bytes changed, so its old work goes: it is a different member now")
        self.members["imp\\\\tree.imp"] = good

    def test_a_member_whose_bytes_cannot_be_read_prunes_nothing(self) -> None:
        import zlib
        self.picks({"sprite__tree": "anime2x", "sprite__rock": "anime2x"})
        self.add("imp\\\\tree.imp", self.frame(20, 6, 3))
        self.add("imp\\\\rock.imp", self.frame(20, 6, 4))
        plan, root, _ = setup.plan_sprites(self.game, animated=False)
        setup.upscale_sprites(plan.static, root, pathlib.Path("e"), pathlib.Path("m"))
        rock = next(s for s in plan.static if s.name == "rock").frames[0].stem.split("__")[0]
        members = self.members

        class Damaged:
            def __init__(self, path): pass
            def __contains__(self, name): return name.lower() in members
            def read(self, name):
                if "rock" in name:
                    return zlib.decompress(b"damaged")
                return members[name.lower()]

        setup.mpq_read.Archive = Damaged
        plan, root, _ = setup.plan_sprites(self.game, animated=False)
        self.assertEqual([s.name for s in plan.static], ["tree"])
        self.assertEqual(len(self.renders_of(root, rock)), 1, "its key is unknown: nothing pruned")

    def test_changing_one_member_re_renders_only_that_member(self) -> None:
        """A mod that repaints one sprite changes imp.mpq; every other sprite keeps its render (hours
        of them, with --sprites). The changed one is rendered afresh and its old work removed."""
        self.picks({"sprite__tree": "anime2x", "sprite__rock": "anime2x"})
        self.add("imp\\tree.imp", self.frame(20, 6, 3))
        self.add("imp\\rock.imp", self.frame(20, 6, 4))
        plan, root, _ = setup.plan_sprites(self.game, animated=False)
        setup.upscale_sprites(plan.static, root, pathlib.Path("e"), pathlib.Path("m"))
        old = {s.name: s.frames[0].stem for s in plan.static}
        self.assertEqual(sorted(self.rendered), sorted(old.values()))
        self.rendered.clear()
        self.add("imp\\rock.imp", self.frame(20, 6, 9))                  # the mod's repaint
        (self.game / "imp.mpq").write_bytes(b"a modded imp.mpq")
        plan, root, _ = setup.plan_sprites(self.game, animated=False)
        setup.upscale_sprites(plan.static, root, pathlib.Path("e"), pathlib.Path("m"))
        new = {s.name: s.frames[0].stem for s in plan.static}
        self.assertEqual(new["tree"], old["tree"])
        self.assertNotEqual(new["rock"], old["rock"])
        self.assertEqual(self.rendered, [new["rock"]], "only the changed member is rendered again")
        leftovers = [p.name for p in root.rglob("*.png") if p.name.startswith(old["rock"].split("__")[0])]
        self.assertEqual(leftovers, [], "the changed member's old work is removed")

    def test_a_member_that_will_not_decompress_is_left_out_not_fatal(self) -> None:
        """A damaged member (here, zlib data that does not inflate) is one sprite's problem: the
        others -- and the pictures -- still install."""
        import zlib
        self.picks({"sprite__tree": "anime2x", "sprite__bad": "anime2x"})
        self.add("imp\\tree.imp", self.frame(20, 6, 3))
        self.add("imp\\bad.imp", self.frame(20, 6, 4))
        members = self.members

        class Damaged:
            def __init__(self, path): pass
            def __contains__(self, name): return name.lower() in members
            def read(self, name):
                if "bad" in name:
                    return zlib.decompress(b"not zlib at all")
                return members[name.lower()]

        setup.mpq_read.Archive = Damaged
        plan, root, read_sprite = setup.plan_sprites(self.game, animated=False)
        self.assertEqual([s.name for s in plan.static], ["tree"])
        self.assertEqual(len(plan.skipped), 1)
        self.assertTrue(plan.skipped[0].startswith("bad: could not read imp\\bad.imp (not readable from "
                                                   "the archive (error:"), plan.skipped[0])
        self.assertIn("could not be read", setup.summarise_skips(plan.skipped))

    def test_the_sprite_mode_is_remembered_until_turned_off(self) -> None:
        """A plain rerun after --sprites (after --review, say) must not quietly drop hours of
        animated sprites; --no-sprites turns them off; a fresh install is static only."""
        self.assertEqual(setup.sprite_mode(self.game, False, False)[0], False, "fresh: static only")
        dll = self.game / "ddraw.dll"
        dll.write_bytes(b"the player's own ddraw.dll")
        release_dir = self.base / "release"
        release_dir.mkdir()
        (release_dir / "ddraw.dll").write_bytes(OURS)
        self.addCleanup(setattr, setup, "HERE", setup.HERE)
        setup.HERE = release_dir
        ours = {"ddraw_sha256": hashlib.sha256(OURS).hexdigest(), "version": "t"}
        self.assertTrue(setup.sprite_mode(self.game, True, False)[0])
        setup.install(self.game, b"pack", ours, True)
        on, why = setup.sprite_mode(self.game, False, False)
        self.assertTrue(on)
        self.assertIn("remembered", why)
        setup.install(self.game, b"pack", ours, on)                       # a plain rerun keeps it
        self.assertTrue(setup.sprite_mode(self.game, False, False)[0])
        self.assertFalse(setup.sprite_mode(self.game, False, True)[0], "--no-sprites wins")
        setup.install(self.game, b"pack", ours, False)
        self.assertFalse(setup.sprite_mode(self.game, False, False)[0], "and is remembered too")
        setup.install(self.game, b"pack", ours, True)
        setup.uninstall(self.game)
        self.assertFalse(setup.sprite_mode(self.game, False, False)[0], "uninstall forgets it")

    def test_a_plain_rerun_after_sprites_keeps_the_animated_records(self) -> None:
        """Through main's own choice: the rerun plans (and so packs) the animated sprite again."""
        self.picks({"sprite__cav": "anime2x", "sprite__one": "anime2x"})
        self.add("units\\cav.imp", self.frame(20, 6, 3), self.frame(20, 6, 4))
        self.add("imp\\one.imp", self.frame(20, 6, 5))
        (self.game / setup.RECORD_NAME).write_text(json.dumps(
            {"ddraw_sha256": "x", "had_ddraw": False, "backup_sha256": None, "sprites": True}))
        on, _ = setup.sprite_mode(self.game, False, False)
        plan, _, _ = setup.plan_sprites(self.game, on)
        self.assertEqual(([s.name for s in plan.static], [s.name for s in plan.animated]), (["one"], ["cav"]))
        off, _ = setup.sprite_mode(self.game, False, True)
        plan, _, _ = setup.plan_sprites(self.game, off)
        self.assertEqual(plan.animated, [])

    def test_animated_sprites_resume_where_a_run_stopped(self) -> None:
        self.picks({"sprite__cav": "anime2x"})
        self.add("units\\cav.imp", self.frame(20, 6, 3), self.frame(20, 6, 4))
        plan, root, _ = setup.plan_sprites(self.game, animated=True)
        setup.upscale_sprites(plan.animated, root, pathlib.Path("esrgan"), pathlib.Path("models"))
        self.assertEqual(len(self.rendered), 2)
        plan, root, _ = setup.plan_sprites(self.game, animated=True)
        setup.upscale_sprites(plan.animated, root, pathlib.Path("esrgan"), pathlib.Path("models"))
        self.assertEqual(len(self.rendered), 2, "nothing rendered twice")

    def test_a_game_without_imp_mpq_is_refused(self) -> None:
        (self.game / "imp.mpq").unlink()
        with self.assertRaises(SystemExit):
            setup.check_imp(self.game)

    @unittest.skipUnless(shutil.which("magick"), "no ImageMagick (magick) on PATH")
    def test_a_damaged_upscale_is_made_again_once_and_counted(self) -> None:
        """The tester's case: a render in the cache from an earlier run is damaged. A plain rerun
        finds it, makes it again, and packs the clean one; one that comes out damaged every time
        is left out and named."""
        from test_hd_sprites import decode_rgba, doubled
        self.picks({"sprite__tree": "anime2x", "sprite__rock": "anime2x"})
        self.add("imp\\tree.imp", self.frame(40, 10, 3))
        self.add("imp\\rock.imp", self.frame(40, 10, 6))
        always_bad = set()

        def render(option, inputs, dest, esrgan, models):
            dest.mkdir(parents=True, exist_ok=True)
            for key, src in inputs.items():
                self.rendered.append(key)
                w, h, rgba = decode_rgba(src)
                hd = doubled(w, h, rgba)
                if key in always_bad or len(self.rendered) == 1:
                    hd = bytes(len(hd))                       # right size, wrong pixels
                self.hd_sprites.write_png_rgba(dest / f"{key}.png", w * 2, h * 2, hd)
            return len(inputs)

        setup.hd_upscale.render = render
        plan, root, read_sprite = setup.plan_sprites(self.game, animated=False)
        setup.upscale_sprites(plan.static, root, pathlib.Path("e"), pathlib.Path("m"))
        first, second = sorted(self.rendered)[0], sorted(self.rendered)[1]
        self.assertEqual(self.rendered[0], first, "the first render written is the damaged one")
        always_bad.add(second)
        (root / "render" / "anime2x" / f"{second}.png").write_bytes(
            (root / "render" / "anime2x" / f"{first}.png").read_bytes())   # a damaged cache entry too
        pack_path = self.base / "lomhd_portraits.pack"
        pictures: dict = {}
        count, skipped, sprite_skipped, static, _ = setup.build_pack(
            pack_path, plan, root, read_sprite, [], [], pathlib.Path("e"), pathlib.Path("m"), pictures)
        self.assertEqual(count, 1)
        self.assertEqual(self.rendered[2:], [first, second], "each damaged one made again, once")
        self.assertEqual((static["damaged"], static["remade"]), (2, 1))
        self.assertEqual(len(sprite_skipped), 1)
        self.assertIn("upscale looked damaged", sprite_skipped[0])
        self.assertEqual(setup.summarise_skips(sprite_skipped), "1 upscale looked damaged")
        self.assertEqual(setup.damage_summary([static, pictures], sprite_skipped + skipped),
                         "2 upscales looked damaged and were made again: 1 came out clean, 1 still "
                         "looked damaged and were left out (the original shows for those; running "
                         "setup again tries them once more).")

    def test_a_picture_is_made_again_by_the_option_that_made_it(self) -> None:
        calls = []
        self.addCleanup(setattr, setup.subprocess, "run", setup.subprocess.run)
        setup.hd_upscale.render = lambda option, inputs, dest, e, m: calls.append(("render", option, dict(inputs), dest))
        setup.subprocess.run = lambda cmd, **kw: calls.append(("approved", cmd[cmd.index("--names") + 1],
                                                                 pathlib.Path(cmd[cmd.index("--names") + 1]).read_text()))
        up = setup.WORK / "upscaled"
        for path in (up / "anime2x" / "aagtwr0a.png", up / "approved" / "portrait" / "aicavp00.lbm"):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"damaged")
            setup.rerender_picture(path, pathlib.Path("e"), pathlib.Path("m"))
            self.assertFalse(path.exists())
        self.assertEqual(calls[0], ("render", "anime2x", {"aagtwr0a": setup.WORK / "png" / "aagtwr0a.png"},
                                    up / "anime2x"))
        self.assertEqual((calls[1][0], calls[1][2]), ("approved", "portrait\\aicavp00.lbm\n"))

    def test_the_summary_says_nothing_was_damaged(self) -> None:
        self.assertEqual(setup.damage_summary([{"damaged": 0}, {}], []), "No upscale looked damaged.")
        self.assertEqual(setup.damage_summary([{"unjudged": 160}], []),
                         "No upscale looked damaged (160 too small to check).")

    def test_the_summary_counts_remakes_that_failed_outright(self) -> None:
        got = setup.damage_summary([{"damaged": 2, "remade": 0, "failed": 1, "unjudged": 3}, {"damaged": 1}],
                                   ["a: anime2x render is missing",
                                    "b: upscale looked damaged (1.20 against 0.9, made twice); the original shows"])
        self.assertEqual(got, "3 upscales looked damaged and were made again: 0 came out clean, 1 still "
                              "looked damaged and were left out, 1 could not be made again and were left "
                              "out (the original shows for those; running setup again tries them once "
                              "more). (3 too small to check)")
        self.assertNotIn("No upscale", setup.damage_summary([{"damaged": 1, "failed": 1}], []))

    def test_the_dev_sprite_limit_never_drops_the_unit_icon_sheets(self) -> None:
        self.picks({"sprite__bat": "anime2x", "sprite__fiicons": "anime2x", "sprite__zzz": "anime2x"})
        for member in ("units\\bat.imp", "iface\\fiicons.imp", "units\\zzz.imp"):
            self.add(member, self.frame(20, 6, len(member)), self.frame(20, 6, len(member) + 1))
        said: list[str] = []
        self.addCleanup(setattr, setup, "say", setup.say)
        setup.say = said.append
        with mock.patch.dict(os.environ, {setup.SPRITE_LIMIT_ENV: "1"}):
            plan, _, _ = setup.plan_sprites(self.game, animated=True)
        self.assertEqual([s.name for s in plan.animated], ["bat", "fiicons"])
        self.add("iface\\aiicons.imp", self.frame(20, 6, 30), self.frame(20, 6, 31))   # sorts before bat
        self.picks({"sprite__bat": "anime2x", "sprite__aiicons": "anime2x", "sprite__fiicons": "anime2x",
                    "sprite__zzz": "anime2x"})
        with mock.patch.dict(os.environ, {setup.SPRITE_LIMIT_ENV: "1"}):
            plan, _, _ = setup.plan_sprites(self.game, animated=True)
        self.assertEqual([s.name for s in plan.animated], ["aiicons", "bat", "fiicons"],
                         "a sheet never takes one of the limit's places")
        self.assertIn(f"{setup.SPRITE_LIMIT_ENV}=1: only 1 animated sprites, plus the unit icon sheets",
                      said[-1])

    @unittest.skipUnless(shutil.which("magick"), "no ImageMagick (magick) on PATH")
    def test_a_plain_run_packs_the_unit_icon_sheets_with_their_strip_windows(self) -> None:
        """No --sprites: the army strip's figures are built anyway, each frame followed by the rows
        the strip shows of it, in the sheet's group; an animated sprite is not."""
        import hd_portrait_pack as pack
        self.picks({"sprite__fiicons": "ultrasharp-tta", "sprite__cav": "anime2x"})
        self.add("iface\\fiicons.imp", self.frame(20, 6, 3), self.frame(20, 6, 4), origins=[(0, -21), (1, 0)])
        self.add("units\\cav.imp", self.frame(20, 6, 6), self.frame(20, 6, 7))
        plan, root, read_sprite = setup.plan_sprites(self.game, animated=False)
        setup.upscale_sprites(plan.static + plan.animated, root, pathlib.Path("e"), pathlib.Path("m"))
        pack_path = self.base / "lomhd_portraits.pack"
        count, _, sprite_skipped, _, moving = setup.build_pack(pack_path, plan, root, read_sprite, [], [])
        got = [(name, small[:2], group) for name, small, _, _, _, group in pack.read(pack_path.read_bytes())]
        # frame 0: its top at 440 - 21 - 3 = 416, so the strip (418-457) shows rows 2-5; frame 1 is
        # shown whole, and its own record is all the strip needs.
        self.assertEqual(got, [("anim__fiicons#000", (20, 6), 1), ("strip__fiicons#000", (20, 4), 1),
                               ("anim__fiicons#001", (20, 6), 1)])
        self.assertEqual((count, moving["packed"], moving["strip"], sprite_skipped), (3, 2, 1, []))

    @unittest.skipUnless(shutil.which("magick"), "no ImageMagick (magick) on PATH")
    def test_the_pack_is_the_same_byte_for_byte_whatever_lomhd_jobs_says(self) -> None:
        """Step 5 on worker processes (LOMHD_JOBS=3) writes the very pack it writes one at a time
        (LOMHD_JOBS=1): static, animated and mirrored sprites, strip crops, pictures, and a damaged
        render the content check makes again."""
        import contextlib
        import lbm_png
        from test_hd_sprites import CountingPool, decode_rgba, doubled, figure

        def render(option, inputs, dest, esrgan, models):
            dest.mkdir(parents=True, exist_ok=True)
            for key, src in inputs.items():
                w, h, rgba = decode_rgba(src)
                self.hd_sprites.write_png_rgba(dest / f"{key}.png", w * 2, h * 2, doubled(w, h, rgba))
            return len(inputs)

        setup.hd_upscale.render = render
        self.picks({"sprite__tree": "anime2x", "sprite__rock": "anime2x", "sprite__cav": "anime2x",
                    "sprite__fiicons": "anime2x"})
        self.add("imp\\tree.imp", self.frame(40, 10, 3))
        self.add("imp\\rock.imp", self.frame(40, 10, 6))
        self.add("units\\cav.imp", self.frame(40, 10, 7), self.frame(40, 10, 8))
        self.add("iface\\fiicons.imp", figure(39, 91, 1), figure(48, 108, 2), origins=[(2, 19), (-1, 34)])
        plan, root, read_sprite = setup.plan_sprites(self.game, animated=True)
        setup.upscale_sprites(plan.static + plan.animated, root, pathlib.Path("e"), pathlib.Path("m"))
        damaged = root / "render" / "anime2x" / f"{next(s for s in plan.static if s.name == 'rock').frames[0].stem}.png"
        self.hd_sprites.write_png_rgba(damaged, 80, 20, bytes(80 * 20 * 4))     # right size, wrong pixels
        bad = damaged.read_bytes()
        originals, upscaled = self.base / "originals", self.base / "upscaled"
        originals.mkdir()
        upscaled.mkdir()
        palette = [(i, (i * 3) % 256, 255 - i) for i in range(256)]
        header = struct.pack(">HHhhBBBBHBBhh", 40, 6, 0, 0, 8, 0, 1, 0, 0, 1, 1, 40, 6)
        for name, seed in (("aagtwr0a", 0), ("abldg", 90)):
            lbm_png.encode(originals / f"{name}.lbm", 40, 6, bytes((i + seed) % 256 for i in range(240)), palette,
                           [(b"BMHD", header), (b"CMAP", b""), (b"BODY", b"")])
            up = bytearray(b"".join(bytes(palette[((y // 2) * 40 + x // 2 + seed) % 256]) + b"\xff"
                                    for y in range(12) for x in range(80)))
            up[0] ^= 1
            self.hd_sprites.write_png_rgba(upscaled / f"{name}.png", 80, 12, bytes(up))
        real_pool, pools = setup.hd_upscale.process_pool, []

        @contextlib.contextmanager
        def counting_pool():
            with real_pool() as pool:
                pools.append(None if pool is None else CountingPool(pool))
                yield pools[-1]

        def build(jobs: str):
            damaged.write_bytes(bad)                  # both runs find it damaged, and make it again
            out, pictures = self.base / f"jobs-{jobs}.pack", {}
            with mock.patch.dict(os.environ, {setup.hd_upscale.JOBS_ENV: jobs}), \
                    mock.patch.object(setup.hd_upscale, "process_pool", counting_pool):
                result = setup.build_pack(out, plan, root, read_sprite, [originals], [upscaled],
                                          pathlib.Path("e"), pathlib.Path("m"), pictures)
            return out.read_bytes(), result, pictures

        one, three = build("1"), build("3")
        self.assertEqual(three[0], one[0])
        self.assertEqual(three[1:], one[1:])
        count, skipped, sprite_skipped, static, moving = one[1]
        self.assertEqual((static["damaged"], static["remade"], moving["strip"], skipped, sprite_skipped),
                         (1, 1, 2, [], []), "the remake and strip crops were on the path compared")
        self.assertEqual(count, 10)          # 2 static, 2 cav, 2 fiicons + 2 strip, 2 pictures
        self.assertIsNone(pools[0], "LOMHD_JOBS=1: no worker processes at all")
        self.assertEqual(sorted({name for name, _ in pools[1].mapped}), ["frame_streams", "picture_streams"])

    @unittest.skipUnless(shutil.which("magick"), "no ImageMagick (magick) on PATH")
    def test_the_pack_holds_sprites_and_pictures_and_uninstall_restores_everything(self) -> None:
        import hd_portrait_pack as pack
        import lbm_png
        self.picks({"sprite__tree": "anime2x", "sprite__cav": "ultrasharp", "sprite__glow": "anime2x",
                    "sprite__plain": "original"})
        self.add("imp\\tree.imp", self.frame(20, 6, 3))
        self.add("imp\\plain.imp", self.frame(20, 6, 5))
        self.add("units\\cav.imp", self.frame(20, 6, 6), self.frame(20, 6, 7))
        self.add("aura\\glow.imp", self.frame(20, 6, 8), self.frame(20, 6, 6))  # frame 1 repeats cav's
        plan, root, read_sprite = setup.plan_sprites(self.game, animated=True)
        setup.upscale_sprites(plan.static + plan.animated, root, pathlib.Path("e"), pathlib.Path("m"))
        originals, upscaled = self.base / "originals", self.base / "upscaled"
        originals.mkdir()
        upscaled.mkdir()
        palette = [(i, (i * 3) % 256, 255 - i) for i in range(256)]
        header = struct.pack(">HHhhBBBBHBBhh", 40, 6, 0, 0, 8, 0, 1, 0, 0, 1, 1, 40, 6)
        lbm_png.encode(originals / "aagtwr0a.lbm", 40, 6, bytes(range(240)), palette,
                       [(b"BMHD", header), (b"CMAP", b""), (b"BODY", b"")])
        # A plausible upscale (each pixel doubled, one nudged so it is not a bare repeat): the
        # content check would leave out an unrelated one.
        up = bytearray(b"".join(bytes(palette[(y // 2) * 40 + x // 2]) + b"\xff"
                                for y in range(12) for x in range(80)))
        up[0] ^= 1
        self.hd_sprites.write_png_rgba(upscaled / "aagtwr0a.png", 80, 12, bytes(up))
        pack_path = self.base / "lomhd_portraits.pack"
        count, skipped, sprite_skipped, static, moving = setup.build_pack(
            pack_path, plan, root, read_sprite, [originals], [upscaled])
        got = [(name, flags, group) for name, _, _, flags, _, group in pack.read(pack_path.read_bytes())]
        masked, mirror = pack.FLAG_MASKED, pack.FLAG_MASKED | pack.FLAG_MIRROR
        self.assertEqual(got, [("sprite__tree", masked, 0),
                               ("anim__cav#000", mirror, 1), ("anim__cav#001", mirror, 1),
                               ("anim__glow#000", masked, 2),
                               ("aagtwr0a", 0, 0)])
        self.assertEqual((count, skipped, static["packed"], moving["sprites"], moving["packed"]),
                         (5, [], 1, 2, 3))
        self.assertEqual(sprite_skipped, ["plain: picked 'original' in review (no upscale beat it)"])
        self.assertEqual(plan.counts["repeats"], 1)

        # The combined pack installs and uninstalls like any other.
        dll, original = self.game / "ddraw.dll", b"the player's own ddraw.dll"
        dll.write_bytes(original)
        before = sorted(p.name for p in self.game.iterdir())
        release_dir = self.base / "release"
        release_dir.mkdir()
        (release_dir / "ddraw.dll").write_bytes(OURS)
        self.addCleanup(setattr, setup, "HERE", setup.HERE)
        setup.HERE = release_dir
        setup.install(self.game, pack_path, {"ddraw_sha256": hashlib.sha256(OURS).hexdigest(), "version": "t"})
        self.assertEqual((self.game / setup.PACK_NAME).read_bytes(), pack_path.read_bytes())
        self.assertEqual(json.loads((self.game / setup.RECORD_NAME).read_text())["pack_sha256"],
                         hashlib.sha256(pack_path.read_bytes()).hexdigest())
        setup.uninstall(self.game)
        self.assertEqual(sorted(p.name for p in self.game.iterdir()), before)
        self.assertEqual(dll.read_bytes(), original)


# --- HD terrain (--terrain) ----------------------------------------------------------------------

def fake_exe(body: bytes = b"\x90" * 16) -> bytes:
    """MZ + PE header + one .text section at VA 0x401000, raw 0x200: enough for exe_patch."""
    import struct
    img = bytearray(0x200 + 0x100)
    img[0:2] = b"MZ"
    struct.pack_into("<I", img, 0x3C, 0x40)
    img[0x40:0x44] = b"PE\0\0"
    struct.pack_into("<H", img, 0x40 + 6, 1)
    struct.pack_into("<H", img, 0x40 + 20, 0x60)
    struct.pack_into("<I", img, 0x40 + 24 + 28, 0x400000)
    entry = 0x40 + 24 + 0x60
    img[entry:entry + 8] = b".text\0\0\0"
    struct.pack_into("<IIII", img, entry + 8, 0x100, 0x1000, 0x100, 0x200)
    img[0x210:0x210 + len(body)] = body
    return bytes(img)


PRISTINE_EXE = fake_exe(bytes.fromhex("c1e510 c1e210 c1fb07") + b"\x90" * 7)


def terrain_set(sha: str, *edits: tuple[int, str, str]) -> dict:
    return {"target": {"sha256": sha},
            "patch": [{"va": va, "old": old, "new": new, "what": f"edit at {va:#x}"} for va, old, new in edits]}


class TerrainInstall(unittest.TestCase):
    """install_terrain / uninstall_terrain against a temporary game with a synthetic lomse.exe, two
    synthetic patch sets in the shipped JSON form, and a stand-in for the built art."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        self.game, self.release_dir, self.built = root / "game", root / "release", root / "built"
        for d in (self.game, self.release_dir / "exe_patches", self.built):
            d.mkdir(parents=True)
        (self.release_dir / "ddraw.dll").write_bytes(OURS)
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        (self.game / "lomse.exe").write_bytes(PRISTINE_EXE)
        sha = hashlib.sha256(PRISTINE_EXE).hexdigest()
        sets = {"terrain-hybrid-2x": terrain_set(sha, (0x401010, "c1 e5 10", "c1 e5 11"),
                                                 (0x401013, "c1 e2 10", "c1 e2 11")),
                "terrain-stride-1024": terrain_set(sha, (0x401016, "c1 fb 07", "c1 fb 06")),
                "fix-mirror-narrow": terrain_set(sha, (0x40101a, "90", "7e"))}
        for name, body in sets.items():
            (self.release_dir / "exe_patches" / f"{name}.json").write_text(json.dumps(body))

        def build(names):
            return setup.exe_patch.apply(
                PRISTINE_EXE, [setup.exe_patch.load_set(self.release_dir / "exe_patches" / f"{n}.json")
                               for n in names])

        self.patched = build(setup.TERRAIN_SETS + setup.FIX_SETS)
        self.fixed = build(setup.FIX_SETS)
        self.patched_before_the_fix = build(setup.TERRAIN_SETS)       # what 0.4.0-0.5.0 wrote
        (self.built / "tilesa01.lbm").write_bytes(b"pretend 2x atlas")
        (self.built / "tilesa01.til").write_bytes(b"TILESIZE= 64, 64")
        self.record = {"version": "test", "ddraw_sha256": hashlib.sha256(OURS).hexdigest()}
        for name, value in (("HERE", self.release_dir), ("say", lambda text: None),
                            ("PRISTINE_EXE_SHA256", sha),
                            ("PATCHED_EXE_SHA256", hashlib.sha256(self.patched).hexdigest()),
                            ("FIXED_EXE_SHA256", hashlib.sha256(self.fixed).hexdigest()),
                            ("EARLIER_PATCHED_EXE_SHA256S",
                             (hashlib.sha256(self.patched_before_the_fix).hexdigest(),))):
            self.addCleanup(setattr, setup, name, getattr(setup, name))
            setattr(setup, name, value)

    def exe(self) -> bytes:
        return (self.game / "lomse.exe").read_bytes()

    def listing(self) -> list[str]:
        return sorted(p.name for p in self.game.iterdir())

    def install_all(self) -> None:
        setup.install(self.game, PACK, self.record)
        setup.install_terrain(self.game, self.built)

    def assert_installed(self) -> None:
        self.assertEqual(self.exe(), self.patched)
        self.assertEqual((self.game / setup.EXE_BACKUP_NAME).read_bytes(), PRISTINE_EXE)
        til = self.game / setup.TERRAIN_DIR / "til"
        self.assertEqual({p.name: p.read_bytes() for p in til.iterdir()},
                         {p.name: p.read_bytes() for p in self.built.iterdir()})

    def assert_uninstalled(self) -> None:
        self.assertEqual(self.exe(), PRISTINE_EXE)
        self.assertEqual((self.game / "ddraw.dll").read_bytes(), ORIGINAL)
        self.assertEqual(self.listing(), ["ddraw.dll", "lomse.exe"])

    def test_the_synthetic_sets_really_change_the_exe(self) -> None:
        """Otherwise every test below would pass with a patch that did nothing."""
        self.assertNotEqual(self.patched, PRISTINE_EXE)
        self.assertEqual(len(self.patched), len(PRISTINE_EXE))

    def test_install_then_uninstall_round_trip(self) -> None:
        self.install_all()
        self.assert_installed()
        record = json.loads((self.game / setup.RECORD_NAME).read_text())["terrain"]
        self.assertEqual(record["exe_original_sha256"], hashlib.sha256(PRISTINE_EXE).hexdigest())
        self.assertEqual(record["exe_patched_sha256"], hashlib.sha256(self.patched).hexdigest())
        self.assertEqual(record["terrain_sha256"], setup.terrain_digest(self.game / setup.TERRAIN_DIR / "til"))
        setup.uninstall(self.game)
        self.assert_uninstalled()

    def test_a_rerun_changes_nothing_and_keeps_the_first_backup(self) -> None:
        self.install_all()
        self.install_all()
        self.assert_installed()
        setup.install(self.game, PACK, self.record)          # a plain re-run, without --terrain
        self.assertIn("terrain", json.loads((self.game / setup.RECORD_NAME).read_text()),
                      "a plain re-run must not forget the terrain it did not touch")
        self.assert_installed()
        setup.uninstall(self.game)
        self.assert_uninstalled()

    def test_changed_art_replaces_the_whole_folder(self) -> None:
        self.install_all()
        (self.built / "tilesb01.lbm").write_bytes(b"an atlas the last build did not have")
        (self.built / "tilesa01.lbm").write_bytes(b"a newer 2x atlas")
        setup.install_terrain(self.game, self.built)
        self.assert_installed()
        self.assertFalse((self.game / (setup.TERRAIN_DIR + ".lomhd-old")).exists())
        self.assertFalse((self.game / (setup.TERRAIN_DIR + ".lomhd-part")).exists())
        (self.built / "tilesb01.lbm").unlink()
        setup.install_terrain(self.game, self.built)
        self.assert_installed()                  # tilesb01 went with the folder it was in

    # --- the art folder swap (cross-review of 7f1cce3) ------------------------------------------

    def fail_rename(self, which: str, error: BaseException = PermissionError("locked"),
                    also_rollback: bool = False):
        """os.replace failing on one rename of the folder swap: `which` is "aside" (lomhd_terrain
        -> .lomhd-old) or "into-place" (.lomhd-part -> lomhd_terrain)."""
        real = setup.os.replace
        dest = self.game / setup.TERRAIN_DIR

        def replace(src, dst):
            src, dst = pathlib.Path(src), pathlib.Path(dst)
            if which == "aside" and src == dest:
                raise error
            if which == "into-place" and dst == dest and src.name.endswith(".lomhd-part"):
                raise error
            if also_rollback and dst == dest and src.name.endswith(".lomhd-old"):
                raise PermissionError("still locked")
            return real(src, dst)

        setup.os.replace = replace
        self.addCleanup(setattr, setup.os, "replace", real)
        return real

    def old_art(self) -> dict:
        til = self.game / setup.TERRAIN_DIR / "til"
        return {p.name: p.read_bytes() for p in til.iterdir()}

    def test_a_failed_rename_either_way_leaves_the_patched_exe_its_art(self) -> None:
        for which in ("aside", "into-place"):
            with self.subTest(which):
                self.install_all()
                before = self.old_art()
                (self.built / "tilesa01.lbm").write_bytes(b"art for " + which.encode())
                real = self.fail_rename(which)
                with self.assertRaises(SystemExit) as stopped:
                    setup.install_terrain(self.game, self.built)
                setup.os.replace = real
                self.assertIn("run python lomhd_setup.py --terrain again", str(stopped.exception))
                self.assertEqual(self.exe(), self.patched)
                self.assertEqual(self.old_art(), before, "the patched exe must keep a folder")
                self.assertFalse((self.game / (setup.TERRAIN_DIR + ".lomhd-old")).exists())
                self.assertFalse((self.game / (setup.TERRAIN_DIR + ".lomhd-part")).exists())
                setup.install_terrain(self.game, self.built)       # the re-run finishes the job
                self.assert_installed()

    def test_an_interrupted_swap_is_put_back_by_the_next_run(self) -> None:
        """Killed between the two renames, with the rollback failing too: .lomhd-old and no folder.
        The next run restores it FIRST -- here it then fails its own copy, and the patched exe must
        still have its art."""
        self.install_all()
        before = self.old_art()
        (self.built / "tilesa01.lbm").write_bytes(b"a newer 2x atlas")
        real = self.fail_rename("into-place", KeyboardInterrupt(), also_rollback=True)
        with self.assertRaises(KeyboardInterrupt):
            setup.install_terrain(self.game, self.built)
        setup.os.replace = real
        self.assertFalse((self.game / setup.TERRAIN_DIR).exists())
        self.assertTrue((self.game / (setup.TERRAIN_DIR + ".lomhd-old")).exists())

        real_copytree = setup.shutil.copytree
        setup.shutil.copytree = lambda *a, **k: (_ for _ in ()).throw(OSError("disk full"))
        try:
            with self.assertRaises(OSError):
                setup.install_terrain(self.game, self.built)
        finally:
            setup.shutil.copytree = real_copytree
        self.assertEqual(self.old_art(), before, "the old folder must be back before anything else")
        setup.install_terrain(self.game, self.built)
        self.assert_installed()
        self.assertFalse((self.game / (setup.TERRAIN_DIR + ".lomhd-old")).exists())

    def test_uninstall_puts_an_interrupted_swap_back_even_when_it_then_stops(self) -> None:
        self.install_all()
        os.replace(self.game / setup.TERRAIN_DIR, self.game / (setup.TERRAIN_DIR + ".lomhd-old"))
        (self.game / setup.EXE_BACKUP_NAME).write_bytes(b"damaged")
        with self.assertRaises(SystemExit):
            setup.uninstall(self.game)
        self.assertEqual(self.exe(), self.patched)
        self.assertTrue((self.game / setup.TERRAIN_DIR / "til" / "tilesa01.lbm").exists(),
                        "the patched exe still needs its art")

    # --- a lomhd_terrain this mod did not make --------------------------------------------------

    def test_a_folder_put_there_by_hand_is_refused_unless_forced(self) -> None:
        setup.install(self.game, PACK, self.record)
        hand = self.game / setup.TERRAIN_DIR / "til"
        hand.mkdir(parents=True)
        (hand / "tilesa01.lbm").write_bytes(b"the player's own art")
        with self.assertRaises(SystemExit) as stopped:
            setup.install_terrain(self.game, self.built)
        self.assertIn("--force-terrain-folder", str(stopped.exception))
        self.assertEqual((hand / "tilesa01.lbm").read_bytes(), b"the player's own art")
        self.assertEqual(self.exe(), PRISTINE_EXE)
        self.assertNotIn("terrain", json.loads((self.game / setup.RECORD_NAME).read_text()))
        with self.assertRaises(SystemExit):                  # main's early check says the same
            setup.check_terrain_folder(self.game, setup.read_record(self.game), False)
        setup.install_terrain(self.game, self.built, force_folder=True)
        self.assert_installed()

    def test_a_folder_the_player_edited_is_refused_unless_forced(self) -> None:
        self.install_all()
        stray = self.game / setup.TERRAIN_DIR / "til" / "stray.lbm"
        stray.write_bytes(b"the player's edit")
        (self.built / "tilesa01.lbm").write_bytes(b"a newer 2x atlas")
        with self.assertRaises(SystemExit):
            setup.install_terrain(self.game, self.built)
        self.assertEqual(stray.read_bytes(), b"the player's edit")
        setup.install_terrain(self.game, self.built, force_folder=True)
        self.assert_installed()

    def test_uninstall_restores_the_exe_but_keeps_a_folder_it_does_not_own(self) -> None:
        self.install_all()
        edited = self.game / setup.TERRAIN_DIR / "til" / "tilesa01.lbm"
        edited.write_bytes(b"the player's retouched atlas")
        setup.uninstall(self.game)
        self.assertEqual(self.exe(), PRISTINE_EXE)
        self.assertEqual((self.game / "ddraw.dll").read_bytes(), ORIGINAL)
        self.assertEqual(edited.read_bytes(), b"the player's retouched atlas")
        self.assertEqual(self.listing(), ["ddraw.dll", setup.TERRAIN_DIR, "lomse.exe"])

    def test_a_folder_the_player_added_to_is_left_not_crashed_on(self) -> None:
        """A subfolder inside til cannot be hashed as a file: uninstall must treat the folder as
        not ours and finish, not stop half way with the exe restored and the overlay still in."""
        self.install_all()
        custom = self.game / setup.TERRAIN_DIR / "til" / "custom"
        custom.mkdir()
        setup.uninstall(self.game)
        self.assertEqual(self.exe(), PRISTINE_EXE)
        self.assertEqual((self.game / "ddraw.dll").read_bytes(), ORIGINAL)
        self.assertTrue(custom.is_dir())
        self.assertEqual(self.listing(), ["ddraw.dll", setup.TERRAIN_DIR, "lomse.exe"])

    def test_the_command_puts_an_interrupted_swap_back_before_the_long_steps(self) -> None:
        """Recovery runs first in main, so a failure in the download or the build (here: the
        release check) cannot leave the patched exe without its art. (Codex review.)"""
        self.install_all()
        before = self.old_art()
        (self.game / setup.TERRAIN_DIR).rename(self.game / (setup.TERRAIN_DIR + ".lomhd-old"))
        (self.game / "pic.mpq").write_bytes(b"")
        argv, real_release = sys.argv, setup.release
        sys.argv = ["lomhd_setup.py", "--game", str(self.game), "--terrain"]
        setup.release = lambda: (_ for _ in ()).throw(SystemExit("the download failed"))
        try:
            with self.assertRaises(SystemExit):
                setup.main()
        finally:
            sys.argv, setup.release = argv, real_release
        self.assertEqual(self.old_art(), before)
        self.assertEqual(self.exe(), self.patched)

    def test_a_run_stopped_after_the_record_still_owns_the_folder_it_left(self) -> None:
        """The record names the NEW art before the folder is swapped; the folder left in place by
        a run stopped between them is still this mod's, for a re-run and for uninstall."""
        self.install_all()
        (self.built / "tilesa01.lbm").write_bytes(b"a newer 2x atlas")
        real_copytree = setup.shutil.copytree
        setup.shutil.copytree = lambda *a, **k: (_ for _ in ()).throw(KeyboardInterrupt())
        try:
            with self.assertRaises(KeyboardInterrupt):
                setup.install_terrain(self.game, self.built)
        finally:
            setup.shutil.copytree = real_copytree
        setup.uninstall(self.game)
        self.assert_uninstalled()

    # --- writes to files the running game holds -------------------------------------------------

    def test_the_exe_backup_is_written_whole_or_not_at_all(self) -> None:
        setup.install(self.game, PACK, self.record)
        real = setup.os.replace

        def stop_at_the_backup(src, dst):
            if pathlib.Path(dst).name == setup.EXE_BACKUP_NAME:
                raise KeyboardInterrupt
            return real(src, dst)

        setup.os.replace = stop_at_the_backup
        try:
            with self.assertRaises(KeyboardInterrupt):
                setup.install_terrain(self.game, self.built)
        finally:
            setup.os.replace = real
        self.assertFalse((self.game / setup.EXE_BACKUP_NAME).exists())
        self.assertEqual(self.exe(), PRISTINE_EXE)
        setup.install_terrain(self.game, self.built)
        self.assert_installed()
        setup.uninstall(self.game)
        self.assert_uninstalled()

    def test_a_locked_exe_stops_install_and_uninstall_with_a_message(self) -> None:
        """Windows: the running game holds lomse.exe, and the rename over it is refused."""
        setup.install(self.game, PACK, self.record)
        real = setup.os.replace

        def locked(src, dst):
            if pathlib.Path(dst).name == setup.EXE_NAME:
                raise PermissionError(13, "Access is denied")
            return real(src, dst)

        setup.os.replace = locked
        try:
            with self.assertRaises(SystemExit) as stopped:
                setup.install_terrain(self.game, self.built)
            self.assertIn("Close Lords of Magic", str(stopped.exception))
            self.assertNotIn("lomse.exe.lomhd-part", self.listing())
            self.assertEqual(self.exe(), PRISTINE_EXE)
            setup.os.replace = real
            setup.install_terrain(self.game, self.built)
            setup.os.replace = locked
            with self.assertRaises(SystemExit) as stopped:
                setup.uninstall(self.game)
            self.assertIn("--uninstall again", str(stopped.exception))
            self.assertNotIn("lomse.exe.lomhd-part", self.listing())
        finally:
            setup.os.replace = real
        self.assert_installed()
        setup.uninstall(self.game)
        self.assert_uninstalled()

    @unittest.skipIf(hasattr(os, "geteuid") and os.geteuid() == 0, "root can open a read-only file")
    def test_uninstall_checks_the_exe_is_writable_before_changing_anything(self) -> None:
        self.install_all()
        exe = self.game / "lomse.exe"
        exe.chmod(0o444)
        self.addCleanup(exe.chmod, 0o644)
        with self.assertRaises(SystemExit):
            setup.uninstall(self.game)
        self.assertEqual(self.exe(), self.patched)
        self.assertEqual((self.game / "ddraw.dll").read_bytes(), OURS)
        self.assertTrue((self.game / setup.TERRAIN_DIR).exists())

    def test_a_failed_imagemagick_step_names_the_cache_to_delete(self) -> None:
        import subprocess
        for name, value in (("extract_terrain", lambda game: self.built),
                            ("WORK", self.release_dir / "lomhd_work")):
            self.addCleanup(setattr, setup, name, getattr(setup, name))
            setattr(setup, name, value)

        def build(*args):
            raise subprocess.CalledProcessError(1, ["magick", "tile.png"])

        self.addCleanup(setattr, setup.terrain_hd, "build", setup.terrain_hd.build)
        setup.terrain_hd.build = build
        with self.assertRaises(SystemExit) as stopped:
            setup.build_terrain(self.game, pathlib.Path("esrgan"), pathlib.Path("models"))
        self.assertIn("lomhd_work", str(stopped.exception))

    def test_an_unknown_exe_is_refused_before_anything_is_touched(self) -> None:
        setup.install(self.game, PACK, self.record)
        (self.game / "lomse.exe").write_bytes(fake_exe(b"another version"))
        before = self.listing()
        with self.assertRaises(SystemExit):
            setup.install_terrain(self.game, self.built)
        self.assertEqual(self.exe(), fake_exe(b"another version"))
        self.assertEqual(self.listing(), before)
        self.assertNotIn("terrain", json.loads((self.game / setup.RECORD_NAME).read_text()))

    def test_an_already_patched_exe_without_its_backup_is_refused(self) -> None:
        setup.install(self.game, PACK, self.record)
        (self.game / "lomse.exe").write_bytes(self.patched)
        with self.assertRaises(SystemExit):
            setup.install_terrain(self.game, self.built)
        self.assertFalse((self.game / setup.TERRAIN_DIR).exists())

    def test_a_stray_exe_backup_is_never_overwritten(self) -> None:
        setup.install(self.game, PACK, self.record)
        (self.game / setup.EXE_BACKUP_NAME).write_bytes(b"something else")
        with self.assertRaises(SystemExit):
            setup.install_terrain(self.game, self.built)
        self.assertEqual((self.game / setup.EXE_BACKUP_NAME).read_bytes(), b"something else")
        self.assertEqual(self.exe(), PRISTINE_EXE)

    def test_an_install_interrupted_before_the_exe_is_finished_by_a_rerun(self) -> None:
        """The art is in place and the exe is not yet patched: harmless (the DLL serves the folder
        only to the patched exe), and the next run patches it."""
        setup.install(self.game, PACK, self.record)
        real_write = setup.write_atomically

        def stop_at_the_exe(path, data):
            if path.name == "lomse.exe":
                raise KeyboardInterrupt
            real_write(path, data)

        setup.write_atomically = stop_at_the_exe
        try:
            with self.assertRaises(KeyboardInterrupt):
                setup.install_terrain(self.game, self.built)
        finally:
            setup.write_atomically = real_write
        self.assertEqual(self.exe(), PRISTINE_EXE)
        self.assertTrue((self.game / setup.TERRAIN_DIR / "til" / "tilesa01.lbm").exists())
        self.assertIn("terrain", json.loads((self.game / setup.RECORD_NAME).read_text()))
        setup.install_terrain(self.game, self.built)
        self.assert_installed()
        setup.uninstall(self.game)
        self.assert_uninstalled()

    def test_the_exe_is_written_last(self) -> None:
        """Any failure before the exe step leaves it pristine: art without the patch is harmless,
        the patch without the art is not."""
        setup.install(self.game, PACK, self.record)
        real_copytree = setup.shutil.copytree

        def fail_copy(*args, **kwargs):
            raise OSError("disk full")

        setup.shutil.copytree = fail_copy
        try:
            with self.assertRaises(OSError):
                setup.install_terrain(self.game, self.built)
        finally:
            setup.shutil.copytree = real_copytree
        self.assertEqual(self.exe(), PRISTINE_EXE)
        self.assertFalse((self.game / setup.EXE_BACKUP_NAME).exists())
        setup.uninstall(self.game)
        self.assert_uninstalled()

    def test_uninstall_after_steam_restored_the_original_exe(self) -> None:
        """'Verify integrity' puts the original lomse.exe back; uninstall drops the backup and the
        folder and does not complain."""
        self.install_all()
        (self.game / "lomse.exe").write_bytes(PRISTINE_EXE)
        setup.uninstall(self.game)
        self.assert_uninstalled()

    def test_a_reinstall_after_steam_restored_the_original_exe(self) -> None:
        self.install_all()
        (self.game / "lomse.exe").write_bytes(PRISTINE_EXE)
        self.install_all()
        self.assert_installed()

    def test_uninstall_leaves_an_exe_someone_else_changed_and_removes_nothing(self) -> None:
        """Still holding this mod's edits (another patch stacked on ours): the folder is still
        needed, so nothing is removed, and the message names the way out."""
        self.install_all()
        stacked = bytearray(self.patched)
        stacked[0x21F] ^= 0xFF                      # outside every terrain site
        (self.game / "lomse.exe").write_bytes(bytes(stacked))
        before = self.listing()
        with self.assertRaises(SystemExit) as stopped:
            setup.uninstall(self.game)
        self.assertIn("Verify integrity", str(stopped.exception))
        self.assertIn("--uninstall again", str(stopped.exception))
        self.assertEqual(self.exe(), bytes(stacked))
        self.assertEqual(self.listing(), before)
        self.assertEqual((self.game / setup.EXE_BACKUP_NAME).read_bytes(), PRISTINE_EXE)
        self.assertEqual((self.game / "ddraw.dll").read_bytes(), OURS)

    def test_uninstall_goes_ahead_when_the_changed_exe_holds_none_of_our_edits(self) -> None:
        """Replaced outright (another version, a game update): the folder is inert. It goes, being
        ours; the DLL is uninstalled; the exe and its original's backup are left."""
        for replacement in (fake_exe(b"another version!"), b"not even a PE file"):
            with self.subTest(replacement[:8]):
                self.install_all()
                (self.game / "lomse.exe").write_bytes(replacement)
                setup.uninstall(self.game)
                self.assertEqual(self.exe(), replacement)
                self.assertEqual((self.game / "ddraw.dll").read_bytes(), ORIGINAL)
                self.assertEqual((self.game / setup.EXE_BACKUP_NAME).read_bytes(), PRISTINE_EXE)
                self.assertEqual(self.listing(), ["ddraw.dll", "lomse.exe", setup.EXE_BACKUP_NAME])
                (self.game / setup.EXE_BACKUP_NAME).unlink()
                (self.game / "lomse.exe").write_bytes(PRISTINE_EXE)

    def test_uninstall_with_a_changed_exe_backup_restores_nothing(self) -> None:
        self.install_all()
        (self.game / setup.EXE_BACKUP_NAME).write_bytes(b"damaged")
        with self.assertRaises(SystemExit):
            setup.uninstall(self.game)
        self.assertEqual(self.exe(), self.patched)
        self.assertTrue((self.game / setup.TERRAIN_DIR).exists(),
                        "the patched exe still needs its art")

    def test_uninstall_without_terrain_leaves_the_exe_alone(self) -> None:
        """A game that never had --terrain: its exe, whatever it is, is not this mod's business."""
        (self.game / "lomse.exe").write_bytes(b"a different lomse.exe")
        setup.install(self.game, PACK, self.record)
        setup.uninstall(self.game)
        self.assertEqual(self.exe(), b"a different lomse.exe")
        self.assertEqual(self.listing(), ["ddraw.dll", "lomse.exe"])

    def test_the_exe_is_restored_even_when_another_mod_replaced_the_dll(self) -> None:
        """The other mod's ddraw.dll cannot serve lomhd_terrain, so the patched exe would draw
        scrambled terrain: the exe half is undone before the DLL refusal."""
        self.install_all()
        (self.game / "ddraw.dll").write_bytes(b"someone else's dll")
        with self.assertRaises(SystemExit):
            setup.uninstall(self.game)
        self.assertEqual(self.exe(), PRISTINE_EXE)
        self.assertFalse((self.game / setup.TERRAIN_DIR).exists())

    # --- the Shade fix (fix-mirror-narrow), on every install -------------------------------------

    def sha(self, data: bytes) -> str:
        return hashlib.sha256(data).hexdigest()

    def on_disk(self) -> dict:
        return json.loads((self.game / setup.RECORD_NAME).read_text())

    def install_plain(self) -> str:
        setup.install(self.game, PACK, self.record)
        return setup.fix_exe(self.game)

    def assert_record_names_the_exe(self) -> None:
        """Whatever path wrote the exe, the record names it and every exe this game had from us."""
        exe = self.on_disk()["exe"]
        self.assertEqual(exe["original_sha256"], self.sha(PRISTINE_EXE))
        self.assertEqual(exe["patched_sha256"], self.sha(self.exe()))
        self.assertIn(self.sha(self.exe()), exe["patched_sha256s"])

    def test_the_fix_and_the_terrain_are_different_edits(self) -> None:
        self.assertNotIn(self.fixed, (PRISTINE_EXE, self.patched, self.patched_before_the_fix))
        self.assertNotEqual(self.patched, self.patched_before_the_fix)

    def test_a_plain_install_applies_the_fix_and_uninstall_takes_it_out(self) -> None:
        self.assertIn("fixed", self.install_plain())
        self.assertEqual(self.exe(), self.fixed)
        self.assertEqual((self.game / setup.EXE_BACKUP_NAME).read_bytes(), PRISTINE_EXE)
        self.assertNotIn("terrain", self.on_disk())
        self.assert_record_names_the_exe()
        self.install_plain()                                   # a re-run changes nothing
        self.assertEqual(self.exe(), self.fixed)
        self.assertEqual((self.game / setup.EXE_BACKUP_NAME).read_bytes(), PRISTINE_EXE)
        said = []
        with mock.patch.object(setup, "say", said.append):
            setup.uninstall(self.game)
        self.assert_uninstalled()
        self.assertIn("The Shade crash fix was removed: lomse.exe is your original again.", said)

    def test_a_plain_install_leaves_an_exe_it_does_not_know_alone(self) -> None:
        for other in (fake_exe(b"another version"), b"not even a PE file"):
            with self.subTest(other[:8]):
                (self.game / "lomse.exe").write_bytes(other)
                self.assertIn("not applied", self.install_plain())
                self.assertEqual(self.exe(), other)
                self.assertFalse((self.game / setup.EXE_BACKUP_NAME).exists())
                self.assertNotIn("exe", self.on_disk())
                setup.uninstall(self.game)
                self.assertEqual(self.exe(), other)
                self.assertEqual(self.listing(), ["ddraw.dll", "lomse.exe"])

    def test_a_plain_install_never_overwrites_a_stray_exe_backup(self) -> None:
        (self.game / setup.EXE_BACKUP_NAME).write_bytes(b"something else")
        self.assertIn("not applied", self.install_plain())
        self.assertEqual(self.exe(), PRISTINE_EXE)
        self.assertEqual((self.game / setup.EXE_BACKUP_NAME).read_bytes(), b"something else")

    def test_terrain_installs_the_fix_too(self) -> None:
        self.install_all()
        self.assertEqual(self.exe(), self.patched)
        self.assertEqual(self.on_disk()["terrain"]["exe_patched_sha256"], self.sha(self.patched))
        self.assert_record_names_the_exe()

    def test_terrain_on_top_of_a_plain_install_keeps_the_first_backup(self) -> None:
        self.install_plain()
        setup.install_terrain(self.game, self.built)
        self.assert_installed()
        self.assert_record_names_the_exe()
        self.assertEqual(set(self.on_disk()["exe"]["patched_sha256s"]),
                         {self.sha(self.fixed), self.sha(self.patched)})
        setup.uninstall(self.game)
        self.assert_uninstalled()

    def test_a_plain_rerun_after_terrain_keeps_the_terrain_in_the_exe(self) -> None:
        """Taking the terrain edits out would leave the installed art unused: the fix goes on top."""
        self.install_all()
        self.assertIn("HD terrain kept", self.install_plain())
        self.assert_installed()
        setup.uninstall(self.game)
        self.assert_uninstalled()

    def as_installed_by_0_5_0(self) -> None:
        """The game as 0.5.0 --terrain left it: the terrain exe without the fix, a record whose
        terrain section names only that exe, and no exe section."""
        self.install_all()
        (self.game / "lomse.exe").write_bytes(self.patched_before_the_fix)
        record = self.on_disk()
        del record["exe"]
        old = self.sha(self.patched_before_the_fix)
        record["terrain"].update(exe_patched_sha256=old, exe_patched_sha256s=[old])
        (self.game / setup.RECORD_NAME).write_text(json.dumps(record))

    def test_upgrading_a_0_5_0_terrain_install_adds_the_fix(self) -> None:
        for rerun in ("plain", "terrain"):
            with self.subTest(rerun):
                self.as_installed_by_0_5_0()
                if rerun == "plain":
                    self.install_plain()
                else:
                    self.install_all()
                self.assert_installed()
                terrain = self.on_disk()["terrain"]
                self.assertEqual(terrain["exe_patched_sha256"], self.sha(self.patched))
                self.assertEqual(set(terrain["exe_patched_sha256s"]),
                                 {self.sha(self.patched), self.sha(self.patched_before_the_fix)})
                self.assert_record_names_the_exe()
                setup.uninstall(self.game)
                self.assert_uninstalled()

    def test_a_0_5_0_exe_is_ours_even_when_the_record_forgot_it(self) -> None:
        """EARLIER_PATCHED_EXE_SHA256S: an exe 0.5.0 wrote is recognised without a record naming it."""
        self.as_installed_by_0_5_0()
        record = self.on_disk()
        record["terrain"].update(exe_patched_sha256s=[])
        del record["terrain"]["exe_patched_sha256"]
        (self.game / setup.RECORD_NAME).write_text(json.dumps(record))
        setup.uninstall(self.game)
        self.assert_uninstalled()

    def test_uninstalling_a_0_5_0_install_restores_the_original(self) -> None:
        self.as_installed_by_0_5_0()
        setup.uninstall(self.game)
        self.assert_uninstalled()

    def test_an_interrupted_fix_is_finished_by_the_next_run(self) -> None:
        setup.install(self.game, PACK, self.record)
        real_write = setup.write_atomically

        def stop_at_the_exe(path, data):
            if path.name == "lomse.exe":
                raise KeyboardInterrupt
            real_write(path, data)

        setup.write_atomically = stop_at_the_exe
        try:
            with self.assertRaises(KeyboardInterrupt):
                setup.fix_exe(self.game)
        finally:
            setup.write_atomically = real_write
        self.assertEqual(self.exe(), PRISTINE_EXE)
        setup.fix_exe(self.game)
        self.assertEqual(self.exe(), self.fixed)
        setup.uninstall(self.game)
        self.assert_uninstalled()

    def test_a_fixed_exe_holds_none_of_the_terrain_edits(self) -> None:
        """So an exe changed on top of the fix alone does not pin a lomhd_terrain folder in place."""
        self.assertFalse(setup.terrain_edits_present(self.fixed))
        self.assertTrue(setup.terrain_edits_present(self.patched))

    def said_by(self, action) -> list:
        said = []
        with mock.patch.object(setup, "say", said.append):
            action()
        return said

    @unittest.skipIf(hasattr(os, "geteuid") and os.geteuid() == 0, "root can open a read-only file")
    def test_a_read_only_exe_does_not_stop_a_plain_install(self) -> None:
        """On 0.5.0 that game installed fine: the fix is skipped, with a note, and nothing else."""
        exe = self.game / "lomse.exe"
        exe.chmod(0o444)
        self.addCleanup(exe.chmod, 0o644)
        setup.check_writable(self.game, False)                    # what a plain run checks
        with self.assertRaises(SystemExit):
            setup.check_writable(self.game, True)                 # --terrain still refuses
        note = self.install_plain()
        self.assertIn("not applied: lomse.exe is read-only (the Shade crash fix needs to change it)", note)
        self.assertEqual(self.exe(), PRISTINE_EXE)
        self.assertFalse((self.game / setup.EXE_BACKUP_NAME).exists())
        self.assertEqual((self.game / "ddraw.dll").read_bytes(), OURS)
        setup.uninstall(self.game)                                # the exe is not ours: not required
        self.assertEqual(self.exe(), PRISTINE_EXE)
        self.assertEqual((self.game / "ddraw.dll").read_bytes(), ORIGINAL)
        self.assertEqual(self.listing(), ["ddraw.dll", "lomse.exe"])

    def test_a_refused_exe_write_is_a_note_not_a_stop(self) -> None:
        """The game holds lomse.exe (Windows): the overlay stays installed, the fix is reported not
        applied, and --terrain still stops with its own message."""
        setup.install(self.game, PACK, self.record)
        real = setup.os.replace

        def locked(src, dst):
            if pathlib.Path(dst).name == setup.EXE_NAME:
                raise PermissionError(13, "Access is denied")
            return real(src, dst)

        with mock.patch.object(setup.os, "replace", locked):
            note = setup.fix_exe(self.game)
            with self.assertRaises(SystemExit) as stopped:
                setup.install_terrain(self.game, self.built)
        self.assertIn("could not write lomse.exe (is the game running?)", note)
        self.assertEqual(self.exe(), PRISTINE_EXE)
        self.assertNotIn("lomse.exe.lomhd-part", self.listing())
        self.assertIn("run python lomhd_setup.py --terrain again", str(stopped.exception))
        self.assertEqual(setup.fix_exe(self.game)[:15], "lomse.exe fixed", "the next run applies it")
        setup.uninstall(self.game)
        self.assert_uninstalled()

    def test_a_retry_command_without_flags_has_no_double_space(self) -> None:
        with mock.patch.object(setup, "write_atomically", side_effect=PermissionError(13, "denied")):
            with self.assertRaises(SystemExit) as stopped:
                setup.write_game_file(self.game / "lomse.exe", b"x", "", "Nothing changed.")
        self.assertIn("run python lomhd_setup.py again.", str(stopped.exception))

    def test_a_fix_only_install_uninstalls_without_its_backup(self) -> None:
        """The one edit is reverted and the result verified against the original's hash."""
        for label, damage in (("missing", lambda b: b.unlink()), ("wrong", lambda b: b.write_bytes(b"junk"))):
            with self.subTest(label):
                self.install_plain()
                backup = self.game / setup.EXE_BACKUP_NAME
                damage(backup)
                note = setup.fix_exe(self.game)                   # a rerun says what is true
                self.assertIn("already has the Shade crash fix", note)
                self.assertNotIn("not applied", note)
                said = self.said_by(lambda: setup.uninstall(self.game))
                self.assertEqual(self.exe(), PRISTINE_EXE)
                self.assertEqual((self.game / "ddraw.dll").read_bytes(), ORIGINAL)
                self.assertIn("The Shade crash fix was removed: lomse.exe is your original again.", said)
                if label == "wrong":
                    self.assertEqual(backup.read_bytes(), b"junk", "not ours to delete")
                    backup.unlink()
                self.assertEqual(self.listing(), ["ddraw.dll", "lomse.exe"])

    def test_only_a_fixed_exe_is_rebuilt(self) -> None:
        self.assertEqual(setup.unfixed(self.fixed), PRISTINE_EXE)
        self.assertIsNone(setup.unfixed(PRISTINE_EXE), "no edit to revert")
        self.assertIsNone(setup.unfixed(b"not a PE file"))

    def test_uninstall_after_steam_restored_a_fixed_exe_says_so(self) -> None:
        self.install_plain()
        (self.game / "lomse.exe").write_bytes(PRISTINE_EXE)
        said = self.said_by(lambda: setup.uninstall(self.game))
        self.assertFalse(any("fix was removed" in line for line in said), said)
        self.assertIn("lomse.exe was already your original (Steam may have put it back); its backup was "
                      "removed.", said)
        self.assert_uninstalled()

    def test_the_players_terrain_picks_win_per_atlas(self) -> None:
        mine, shipped = self.release_dir / "mine.json", self.release_dir / "shipped.json"
        shipped.write_text(json.dumps({"choices": {"terrain__a": "anime2x", "terrain__b": "anime2x",
                                                   "building__x": "ultrasharp"}}))
        mine.write_text(json.dumps({"choices": {"terrain__b": "anime4x", "building__x": "anime2x"}}))
        for name, value in (("MY_CHOICES", mine), ("SHIPPED_CHOICES", shipped)):
            self.addCleanup(setattr, setup, name, getattr(setup, name))
            setattr(setup, name, value)
        self.assertEqual(setup.terrain_choices(), {"terrain__a": "anime2x", "terrain__b": "anime4x"})
        mine.unlink()
        self.assertEqual(setup.terrain_choices(), {"terrain__a": "anime2x", "terrain__b": "anime2x"})

    def test_extraction_takes_the_listed_members_and_refuses_a_missing_one(self) -> None:
        members = {"til\\tilesa01.lbm": b"atlas", "til\\tilesa01.til": b"til"}
        (self.release_dir / "terrain-names.txt").write_text("til\\tilesa01.lbm\ntil\\TILESA01.til\n")

        class Archive:
            def __init__(self, path): pass
            def __contains__(self, name): return name in members
            def read(self, name): return members[name]

        self.addCleanup(setattr, setup.mpq_read, "Archive", setup.mpq_read.Archive)
        self.addCleanup(setattr, setup, "WORK", setup.WORK)
        setup.mpq_read.Archive, setup.WORK = Archive, self.release_dir / "work"
        src = setup.extract_terrain(self.game)
        self.assertEqual({p.name: p.read_bytes() for p in src.iterdir()},
                         {"tilesa01.lbm": b"atlas", "tilesa01.til": b"til"})
        del members["til\\tilesa01.til"]
        with self.assertRaises(SystemExit):
            setup.extract_terrain(self.game)


class TerrainArtChecks(unittest.TestCase):
    """check_terrain_art: rebuild.sh's refusals, on atlases small enough to build in a test (the
    stride is lowered to 128 so a 2x atlas of two 64px tiles counts as full width)."""

    def setUp(self) -> None:
        import struct
        import lbm_png
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        self.src, self.out = root / "src", root / "out"
        self.src.mkdir(); self.out.mkdir()
        (root / "terrain-names.txt").write_text("til\\a01.lbm\ntil\\a01.til\n")
        for name, value in (("HERE", root), ("TERRAIN_STRIDE", 128), ("say", lambda text: None)):
            self.addCleanup(setattr, setup, name, getattr(setup, name))
            setattr(setup, name, value)
        self.struct, self.lbm_png = struct, lbm_png
        self.lbm(self.src / "a01.lbm", 64, 32)
        self.lbm(self.out / "a01.lbm", 128, 64)
        (self.out / "a01.til").write_bytes(b"LBM= a01.lbm\r\nTILESIZE= 64, 64\r\nTILES= 2, 1\r\n")

    def lbm(self, path: pathlib.Path, w: int, h: int) -> None:
        header = self.struct.pack(">HHhhBBBBHBBhh", w, h, 0, 0, 8, 0, 1, 0, 0, 1, 1, w, h)
        self.lbm_png.encode(path, w, h, bytes(w * h), [(i, i, i) for i in range(256)],
                            [(b"BMHD", header), (b"CMAP", b""), (b"BODY", b"")])

    def test_a_whole_build_passes(self) -> None:
        setup.check_terrain_art(self.src, self.out)

    def test_an_atlas_not_at_the_patched_stride_is_refused(self) -> None:
        setup.TERRAIN_STRIDE = 1024
        with self.assertRaises(SystemExit):
            setup.check_terrain_art(self.src, self.out)

    def test_an_atlas_not_twice_its_original_is_refused(self) -> None:
        self.lbm(self.src / "a01.lbm", 64, 64)
        with self.assertRaises(SystemExit):
            setup.check_terrain_art(self.src, self.out)

    def test_a_tilesize_left_undoubled_is_refused(self) -> None:
        (self.out / "a01.til").write_bytes(b"LBM= a01.lbm\r\nTILESIZE= 32, 32\r\nTILES= 2, 1\r\n")
        with self.assertRaises(SystemExit):
            setup.check_terrain_art(self.src, self.out)

    def test_a_missing_til_is_refused(self) -> None:
        (self.out / "a01.til").unlink()
        with self.assertRaises(SystemExit):
            setup.check_terrain_art(self.src, self.out)

    def test_the_shipped_names_are_the_26_tilesets_and_20_atlases(self) -> None:
        setup.HERE = ROOT / "release" / "hd-overlay"
        names = setup.terrain_names()
        self.assertEqual(len(names), 46)
        self.assertEqual(sum(n.endswith(".til") for n in names), 26)
        self.assertEqual(sum(n.endswith(".lbm") for n in names), 20)
        self.assertTrue(all(n.startswith("til\\") and n == n.lower() for n in names))
        self.assertFalse({"til\\thite01.lbm", "til\\ttype01.lbm"} & set(names),
                         "the data maps are not textures and are never doubled")


class ShippedPatchSets(unittest.TestCase):
    """The release ships the terrain sets and the fix as JSON (Python 3.9 has no tomllib)."""

    SETS = ROOT / "tools" / "exe_patches"

    def test_json_sets_match_the_toml_sets_edit_for_edit(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            for name in setup.TERRAIN_SETS + setup.FIX_SETS:
                with self.subTest(name):
                    toml = self.SETS / f"{name}.toml"
                    as_json = pathlib.Path(tmp) / f"{name}.json"
                    as_json.write_text(setup.exe_patch.to_json(toml))
                    a, b = setup.exe_patch.load_set(toml), setup.exe_patch.load_set(as_json)
                    self.assertEqual(a.sha256, b.sha256)
                    self.assertEqual([(p.va, p.old, p.new, p.what) for p in a.patches],
                                     [(p.va, p.old, p.new, p.what) for p in b.patches])
                    self.assertEqual([(s.start, s.end, s.regex, s.count, s.after) for s in a.scans],
                                     [(s.start, s.end, s.regex, s.count, s.after) for s in b.scans])
                    self.assertEqual(a.sha256, setup.PRISTINE_EXE_SHA256)
            total = sum(len(setup.exe_patch.load_set(self.SETS / f"{n}.toml").patches)
                        for n in setup.TERRAIN_SETS)
            self.assertEqual(total, 65)
            self.assertEqual(sum(len(setup.exe_patch.load_set(self.SETS / f"{n}.toml").patches)
                                 for n in setup.FIX_SETS), 1)

    def test_a_json_set_gets_the_same_validation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bad = pathlib.Path(tmp) / "bad.json"
            bad.write_text(json.dumps(terrain_set("00" * 32, (0x401000, "c1 e5 10", "c1 e5"))))
            with self.assertRaises(setup.exe_patch.PatchError):
                setup.exe_patch.load_set(bad)
            bad.write_text(json.dumps({"target": {"sha256": "00" * 32},
                                       "patch": [{"va": 1, "old": "90", "new": "91"}]}))
            with self.assertRaises(setup.exe_patch.PatchError):
                setup.exe_patch.load_set(bad)

    @unittest.skipUnless(
        os.environ.get("LOM_PRISTINE_EXE") and pathlib.Path(os.environ["LOM_PRISTINE_EXE"]).exists(),
        "set LOM_PRISTINE_EXE to a pristine GS5R3 lomse.exe",
    )
    def test_the_pinned_hashes_are_the_real_binary_and_its_patch(self) -> None:
        image = pathlib.Path(os.environ["LOM_PRISTINE_EXE"]).read_bytes()
        self.assertEqual(hashlib.sha256(image).hexdigest(), setup.PRISTINE_EXE_SHA256)
        with tempfile.TemporaryDirectory() as tmp:
            sets = {}
            for name in setup.TERRAIN_SETS + setup.FIX_SETS:
                path = pathlib.Path(tmp) / f"{name}.json"
                path.write_text(setup.exe_patch.to_json(self.SETS / f"{name}.toml"))
                sets[name] = setup.exe_patch.load_set(path)

            def built(names, edits):
                chosen = [sets[n] for n in names]
                self.assertEqual(len(setup.exe_patch.plan(image, chosen)), edits)
                return hashlib.sha256(setup.exe_patch.apply(image, chosen)).hexdigest()

            self.assertEqual(built(setup.TERRAIN_SETS + setup.FIX_SETS, 66), setup.PATCHED_EXE_SHA256)
            self.assertEqual(built(setup.FIX_SETS, 1), setup.FIXED_EXE_SHA256)
            # What 0.4.0-0.5.0 installed, and what an upgrade must still recognise as ours.
            self.assertEqual((built(setup.TERRAIN_SETS, 65),), setup.EARLIER_PATCHED_EXE_SHA256S)
            # The real sets: a fixed exe holds none of the terrain edits (so the folder is never
            # kept for it), the terrain exe does, and the fixed one rebuilds to the original.
            self.addCleanup(setattr, setup, "HERE", setup.HERE)
            setup.HERE = pathlib.Path(tmp)
            (setup.HERE / "exe_patches").mkdir()
            for name in sets:
                (setup.HERE / "exe_patches" / f"{name}.json").write_text(
                    setup.exe_patch.to_json(self.SETS / f"{name}.toml"))
            fixed = setup.exe_patch.apply(image, [sets[n] for n in setup.FIX_SETS])
            patched = setup.exe_patch.apply(image, [sets[n] for n in setup.TERRAIN_SETS + setup.FIX_SETS])
            self.assertFalse(setup.terrain_edits_present(fixed))
            self.assertTrue(setup.terrain_edits_present(patched))
            self.assertEqual(setup.unfixed(fixed), image)


class Report(unittest.TestCase):
    """--report: one zip of diagnostics, built without installing anything."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        self.game, self.release_dir = root / "game", root / "release"
        self.game.mkdir(); self.release_dir.mkdir()
        (self.game / "lomse.exe").write_bytes(b"pretend exe")
        (self.game / "pic.mpq").write_bytes(b"pretend pic")
        (self.game / "imp.mpq").write_bytes(b"pretend imp")
        (self.game / "gs.mpq").write_bytes(b"pretend gs")
        (self.game / "ddraw.dll").write_bytes(OURS)
        (self.game / "ddraw.ini").write_text("[ddraw]\nrenderer=opengl\n")
        (self.game / "lomhd.log").write_text("pack loaded\n")
        (self.release_dir / "ddraw.dll").write_bytes(OURS)
        (self.release_dir / "release.json").write_text(json.dumps(
            {"version": "test", "ddraw_sha256": hashlib.sha256(OURS).hexdigest()}))
        record = {"release": "test", "ddraw_sha256": hashlib.sha256(OURS).hexdigest(),
                 "had_ddraw": True, "backup_sha256": "00" * 32,
                 "overlay_sha256s": [hashlib.sha256(OURS).hexdigest()], "sprites": True}
        (self.game / setup.RECORD_NAME).write_text(json.dumps(record))
        self.addCleanup(setattr, setup, "HERE", setup.HERE)
        setup.HERE = self.release_dir
        # The GPU/Wine probes shell out to the real OS; stubbed here so the suite stays fast and
        # deterministic. Individual tests below restore the real gpu_name to exercise it directly.
        self._real_gpu_name = setup.gpu_name
        self.addCleanup(setattr, setup, "gpu_name", setup.gpu_name)
        setup.gpu_name = lambda: None
        self.addCleanup(setattr, setup, "detect_wine", setup.detect_wine)
        setup.detect_wine = lambda: None

    def make_report(self, with_save=None, with_dump=False) -> pathlib.Path:
        setup.run_report(self.game, with_save, with_dump)
        zips = list(self.release_dir.glob("lomhd-report-*.zip"))
        self.assertEqual(len(zips), 1, "exactly one report zip, next to setup")
        return zips[0]

    def names_in(self, path: pathlib.Path) -> set:
        with zipfile.ZipFile(path) as z:
            return set(z.namelist())

    def member_bytes(self, path: pathlib.Path, name: str) -> bytes:
        with zipfile.ZipFile(path) as z:
            return z.read(name)

    def member_text(self, path: pathlib.Path, name: str) -> str:
        return self.member_bytes(path, name).decode("utf-8")

    def text_in(self, path: pathlib.Path) -> str:
        return self.member_text(path, "report.txt")

    def test_report_contains_the_expected_files(self) -> None:
        path = self.make_report()
        self.assertEqual(self.names_in(path),
                         {"lomhd.log", "ddraw.ini", setup.RECORD_NAME, "release.json", "report.txt"})
        text = self.text_in(path)
        self.assertIn("Python:", text)
        self.assertIn("magick -version:", text)
        sha = hashlib.sha256(b"pretend exe").hexdigest()
        self.assertIn(f"  lomse.exe: {sha} -- unknown", text.splitlines(),
                      "our fake lomse.exe matches no known hash")
        self.assertIn("this release's overlay ddraw.dll", text)   # ddraw.dll DOES match
        self.assertIn("release: test", text)
        self.assertIn("(none stored)", text)    # no lomhd_last_summary.txt was ever written

    def test_missing_optional_files_are_tolerated(self) -> None:
        for name in ("ddraw.ini", "lomhd.log", setup.RECORD_NAME):
            (self.game / name).unlink()
        path = self.make_report()
        self.assertEqual(self.names_in(path), {"release.json", "report.txt"})
        self.assertIn("(none, or damaged)", self.text_in(path))    # no install record to summarise

    # --- the game-folder listing --------------------------------------------------------------------

    def test_unrecognised_names_in_the_game_folder_are_counted_not_shown(self) -> None:
        """A folder or file the mod does not know is a player's own -- it could be named after them
        (a hand-made backup folder, say) -- so the listing must count it rather than print its name."""
        (self.game / "Alice Smith saves").mkdir()
        (self.game / "screenshot 2026-09-27.png").write_bytes(b"not a real png")
        (self.game / "notes from Alice.txt").write_text("reminder to self")
        path = self.make_report()
        text = self.text_in(path)
        for leak in ("Alice Smith saves", "screenshot 2026-09-27.png", "notes from Alice"):
            self.assertNotIn(leak, text, f"{leak} leaked into the game folder listing")
        self.assertIn("+ 2 other files, 1 other folder (names not shown)", text)

    def test_a_loose_prefix_or_suffix_match_is_not_enough_to_be_known(self) -> None:
        """A prefix/suffix allowlist ("lomhd*", "ddraw.*", "*.mpq") is too broad: each of these four
        starts or ends like a known name without being one, and none may be named in the listing."""
        (self.game / "lomhd_private Alice.txt").write_text("not a file this mod writes")
        (self.game / "lomhdAlice.txt").write_text("not a file this mod writes either")
        (self.game / "ddraw.private Alice").write_text("not ddraw.dll or ddraw.ini")
        (self.game / "my Alice.mpq").write_bytes(b"not one of the game's own archives")
        path = self.make_report()
        text = self.text_in(path)
        for leak in ("lomhd_private Alice.txt", "lomhdAlice.txt", "ddraw.private Alice",
                    "my Alice.mpq"):
            self.assertNotIn(leak, text, f"{leak} leaked into the game folder listing")
        self.assertIn("+ 4 other files (names not shown)", text)

    def test_the_games_own_files_are_still_named_in_the_listing(self) -> None:
        path = self.make_report()
        text = self.text_in(path)
        for name in ("lomse.exe", "pic.mpq", "imp.mpq", "gs.mpq", "ddraw.dll", "ddraw.ini",
                    setup.RECORD_NAME, "lomhd.log"):
            self.assertIn(f"  {name}\t", text, f"{name} should still be named -- it is a known file")

    def test_a_known_folder_is_named_but_never_descended_into(self) -> None:
        saves = self.game / "savegame"
        saves.mkdir()
        (saves / "Alice Smith").write_bytes(b"a save that could be named after the player")
        path = self.make_report()
        text = self.text_in(path)
        self.assertIn("  savegame\\\t<folder>", text)
        self.assertNotIn("Alice Smith", text)

    # --- saves ------------------------------------------------------------------------------------

    def test_saves_are_excluded_by_default_and_opt_in_with_with_save(self) -> None:
        saves = self.game / "savegame"
        saves.mkdir()
        (saves / "Water I").write_bytes(b"save data")      # no extension: a player-named save
        self.assertNotIn("savegame/save", self.names_in(self.make_report()))
        for stale in self.release_dir.glob("lomhd-report-*.zip"):
            stale.unlink()
        self.assertIn("savegame/save", self.names_in(self.make_report(with_save="latest")))
        for stale in self.release_dir.glob("lomhd-report-*.zip"):
            stale.unlink()
        self.assertIn("savegame/save", self.names_in(self.make_report(with_save="Water I")))

    def test_save_arcname_never_carries_the_players_own_filename(self) -> None:
        """The save's own name could be the player's account or character name (`Alice Smith.lom`,
        say) -- the zip must never carry it, in the member name or anywhere else in report.txt."""
        saves = self.game / "savegame"
        saves.mkdir()
        (saves / "Alice Smith.lom").write_bytes(b"a save named after the player")
        path = self.make_report(with_save="Alice Smith.lom")
        self.assertEqual(self.names_in(path) - {"lomhd.log", "ddraw.ini", setup.RECORD_NAME,
                                                "release.json", "report.txt"},
                         {"savegame/save.lom"})
        self.assertNotIn("Alice Smith", self.text_in(path))
        self.assertIn("Included a savegame", self.text_in(path))

    def test_only_lom_files_and_extensionless_names_count_as_saves(self) -> None:
        """docs/loose-files.md: a save is `*.lom` or a player-named file with no extension at all --
        `quickstart` is shipped identically in every install and is not a player's save."""
        saves = self.game / "savegame"
        saves.mkdir()
        (saves / "combat.sav").write_bytes(b"shipped state, not a save")
        (saves / "quickstart").write_bytes(b"shipped new-game state")
        (saves / "Desktop.ini").write_text("[.ShellClassInfo]")
        (saves / "autosave.lom").write_bytes(b"a real save")
        for not_a_save in ("combat.sav", "quickstart", "Desktop.ini"):
            with self.subTest(not_a_save):
                with self.assertRaises(SystemExit):
                    setup.find_save(self.game, not_a_save)
        self.assertEqual(setup.find_save(self.game, "latest").name, "autosave.lom")

    def test_a_named_save_that_does_not_exist_stops_before_writing_a_zip(self) -> None:
        with self.assertRaises(SystemExit):
            setup.run_report(self.game, "no-such-save", False)
        self.assertEqual(list(self.release_dir.glob("lomhd-report-*.zip")), [])

    def test_with_save_refuses_path_traversal_and_absolute_paths(self) -> None:
        saves = self.game / "savegame"
        saves.mkdir()
        (self.game.parent / "outside.lom").write_bytes(b"not a save in this game")
        for bad in ("../outside.lom", "/etc/passwd", "sub/evil.lom", ".."):
            with self.subTest(bad):
                with self.assertRaises(SystemExit):
                    setup.find_save(self.game, bad)

    def test_a_symlinked_save_is_refused_even_as_latest(self) -> None:
        saves = self.game / "savegame"
        saves.mkdir()
        target = self.game.parent / "real.lom"
        target.write_bytes(b"a real file outside the game folder")
        link = saves / "linked.lom"
        try:
            link.symlink_to(target)
        except OSError:
            self.skipTest("symlinks are not available in this environment")
        with self.assertRaises(SystemExit):
            setup.find_save(self.game, "linked.lom")
        with self.assertRaises(SystemExit):
            setup.find_save(self.game, "latest")

    # --- crash/hang files and dumps -----------------------------------------------------------------

    def test_crash_and_hang_files_are_capped_at_the_newest_five(self) -> None:
        now = time.time()
        for i in range(7):
            path = self.game / f"lomhd_crash_2026010{i}_000000.txt"
            path.write_text("crash")
            os.utime(path, (now + i, now + i))   # strictly increasing, all well after 1980
        report = self.make_report()
        crash_members = {n for n in self.names_in(report) if n.startswith("crash/")}
        self.assertEqual(crash_members,
                         {f"crash/2026010{i}_000000.txt" for i in range(2, 7)},
                         "only the newest 5 by mtime are kept, and each keeps its own timestamp")

    def test_a_crash_files_own_filename_never_appears_in_the_zip(self) -> None:
        """A crash reporter's filename could itself carry an account or character name (it does, for
        the exe path inside it -- lomhd_crash_core.c writes `Exe: <full path>`); the zip must use a
        normalised member name instead, never the file the player's machine actually wrote."""
        named = self.game / "lomhd_crash_Alice Smith.txt"     # no recognisable timestamp
        named.write_text("Exe: C:\\Users\\Alice\\...\\lomse.exe\n")
        path = self.make_report()
        self.assertEqual({n for n in self.names_in(path) if n.startswith("crash/")}, {"crash/1.txt"})
        self.assertNotIn("Alice Smith", "\n".join(self.names_in(path)))
        self.assertNotIn("Alice", self.member_text(path, "crash/1.txt"))

    def test_a_symlinked_crash_file_is_refused_and_noted(self) -> None:
        target = self.game.parent / "outside.txt"
        target.write_text("not really a crash log")
        link = self.game / "lomhd_crash_20260101_000000.txt"
        try:
            link.symlink_to(target)
        except OSError:
            self.skipTest("symlinks are not available in this environment")
        path = self.make_report()
        self.assertEqual({n for n in self.names_in(path) if n.startswith("crash/")}, set())
        self.assertIn("refused", self.text_in(path))

    def test_a_left_out_crash_files_own_filename_never_appears_either(self) -> None:
        """The left_out note about a file that never made it in must not smuggle the player's own
        filename back in through the back door -- checked here for a symlink refusal, a size-cap
        refusal, and a dump left out for lack of --with-dump, each named with no recognisable
        timestamp so crash_label has nothing but the player's own name to fall back to."""
        target = self.game.parent / "outside.txt"
        target.write_text("not really a crash log")
        link = self.game / "lomhd_crash_Alice Smith.txt"
        try:
            link.symlink_to(target)
        except OSError:
            self.skipTest("symlinks are not available in this environment")
        (self.game / "lomhd_crash_Bob Jones.dmp").write_bytes(b"x")
        (self.game / "lomhd_hang_Carol Diaz.txt").write_text("hang")
        self.addCleanup(setattr, setup, "DUMP_SIZE_CAP", setup.DUMP_SIZE_CAP)
        setup.DUMP_SIZE_CAP = 0
        path = self.make_report(with_dump=False)
        text = self.text_in(path)
        for name in ("Alice Smith", "Bob Jones", "Carol Diaz"):
            self.assertNotIn(name, text, f"{name} leaked into report.txt")
        self.assertIn("refused", text)
        self.assertIn("MiB cap", text)

    def test_a_symlinked_game_text_file_is_refused(self) -> None:
        target = self.game.parent / "outside.log"
        target.write_text("not the real log")
        (self.game / "lomhd.log").unlink()
        try:
            (self.game / "lomhd.log").symlink_to(target)
        except OSError:
            self.skipTest("symlinks are not available in this environment")
        path = self.make_report()
        self.assertNotIn("lomhd.log", self.names_in(path))

    def test_a_dump_over_the_size_cap_is_left_out_and_reported(self) -> None:
        (self.game / "lomhd_crash_20260101_000000.dmp").write_bytes(b"x")
        (self.game / "lomhd_crash_20260102_000000.dmp").write_bytes(b"x")
        self.addCleanup(setattr, setup, "DUMP_SIZE_CAP", setup.DUMP_SIZE_CAP)
        setup.DUMP_SIZE_CAP = 0        # both files now exceed the cap
        path = self.make_report(with_dump=True)
        self.assertEqual({n for n in self.names_in(path) if n.endswith(".dmp")}, set())
        self.assertIn("Left out of this report:", self.text_in(path))

    def test_dumps_are_left_out_by_default_and_included_unscrubbed_with_with_dump(self) -> None:
        dump = self.game / "lomhd_crash_20260101_000000.dmp"
        dump.write_bytes(b"binary minidump bytes, C:\\Users\\Jake\\ in here somewhere")
        path = self.make_report()
        self.assertEqual({n for n in self.names_in(path) if n.endswith(".dmp")}, set())
        self.assertIn("a binary minidump", self.text_in(path))
        for stale in self.release_dir.glob("lomhd-report-*.zip"):
            stale.unlink()
        path = self.make_report(with_dump=True)
        self.assertIn("crash/20260101_000000.dmp", self.names_in(path))
        self.assertEqual(self.member_bytes(path, "crash/20260101_000000.dmp"), dump.read_bytes(),
                         "a dump is included exactly as written -- it is never scrubbed")
        self.assertIn("Included crash dumps", self.text_in(path))

    # --- GPU probe -----------------------------------------------------------------------------------

    def test_gpu_probe_falls_back_to_powershell_when_wmic_is_missing(self) -> None:
        setup.gpu_name = self._real_gpu_name
        self.addCleanup(setattr, setup.platform, "system", setup.platform.system)
        setup.platform.system = lambda: "Windows"
        self.addCleanup(setattr, setup.subprocess, "run", setup.subprocess.run)
        calls = []

        def fake_run(cmd, **kwargs):
            calls.append(cmd[0])
            if cmd[0] == "wmic":
                raise FileNotFoundError("wmic is gone")
            if cmd[0] == "powershell":
                return subprocess.CompletedProcess(cmd, 0, stdout="NVIDIA GeForce RTX 4090\n", stderr="")
            return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")

        setup.subprocess.run = fake_run
        path = self.make_report()
        self.assertEqual([c for c in calls if c in ("wmic", "powershell")], ["wmic", "powershell"],
                         "a missing wmic must fall through to PowerShell, not give up")
        self.assertIn("GPU: NVIDIA GeForce RTX 4090", self.text_in(path))

    def test_gpu_probe_returns_could_not_be_read_when_every_windows_tool_fails(self) -> None:
        setup.gpu_name = self._real_gpu_name
        self.addCleanup(setattr, setup.platform, "system", setup.platform.system)
        setup.platform.system = lambda: "Windows"
        self.addCleanup(setattr, setup.subprocess, "run", setup.subprocess.run)

        def boom(cmd, **kwargs):
            raise FileNotFoundError("no such tool")

        setup.subprocess.run = boom
        path = self.make_report()          # must not raise
        self.assertIn("GPU: (could not be read)", self.text_in(path))

    def test_gpu_probe_failure_is_tolerated_on_this_host(self) -> None:
        """Not simulated as a particular platform: whatever host actually runs this test, a broken
        subprocess must still produce a report rather than blow up build_report_text."""
        setup.gpu_name = self._real_gpu_name
        self.addCleanup(setattr, setup.subprocess, "run", setup.subprocess.run)

        def boom(*args, **kwargs):
            raise FileNotFoundError("no such tool")

        setup.subprocess.run = boom
        path = self.make_report()          # must not raise
        self.assertIn("GPU: (could not be read)", self.text_in(path))

    # --- scrubbing -------------------------------------------------------------------------------

    def test_scrub_replaces_the_home_folder_and_generic_windows_wine_paths(self) -> None:
        self.addCleanup(setattr, pathlib.Path, "home", pathlib.Path.home)
        pathlib.Path.home = staticmethod(lambda: pathlib.Path("/Users/theplayer"))
        text = ("game folder: /Users/theplayer/Games/LOM\n"
               "Exe: C:\\Users\\Jake\\AppData\\Local\\lomhd\\lomse.exe\n"
               "old profile: C:\\Documents and Settings\\Bob\\lom.cfg\n"
               "wine home: Z:\\home\\alice\\.wine\\drive_c\\...\n"
               "wine users: Z:\\Users\\Carol\\AppData\\...\n"
               "linux: /home/dave/.wine/...\n")
        scrubbed = setup.scrub(text)
        self.assertIn("~/Games/LOM", scrubbed)
        for name in ("Jake", "Bob", "alice", "Carol", "dave"):
            self.assertNotIn(name, scrubbed)
        self.assertIn("C:\\Users\\<user>\\AppData", scrubbed)
        self.assertIn("C:\\Documents and Settings\\<user>\\lom.cfg", scrubbed)
        self.assertIn("Z:\\home\\<user>\\.wine", scrubbed)
        self.assertIn("Z:\\Users\\<user>\\AppData", scrubbed)
        self.assertIn("/home/<user>/.wine", scrubbed)

    def test_scrub_catches_a_home_path_with_no_trailing_slash(self) -> None:
        """A path can simply end where the account name does -- at the end of a line, or right
        before a closing quote, a space, or the punctuation that follows a path in running prose --
        with no separator after the name at all. Every form uses "Alice", a name not set anywhere in
        this process's own environment, so only the generic path pattern -- not the known-account
        word match -- can be what catches it."""
        contexts = {
            "end of line": "path: C:/Users/Alice",
            "before a quote": 'path was "C:\\Users\\Alice", see the log',
            "before a comma": "seen at C:\\Users\\Alice, then it hung",
            "before a space": "under C:\\Users\\Alice and nowhere else",
            "long-path prefix, end of line": "dump path: \\\\?\\C:\\Users\\Alice",
            "posix, end of line": "home: /Users/Alice",
            "posix, before punctuation": "home: /Users/Alice; nothing else",
            "UNC, mid-path": "exe: \\\\server\\Users\\Alice\\Documents\\lomse.exe",
        }
        for label, text in contexts.items():
            with self.subTest(label):
                scrubbed = setup.scrub(text)
                self.assertNotIn("Alice", scrubbed, f"{label}: {scrubbed!r}")
                self.assertIn("<user>", scrubbed, f"{label}: {scrubbed!r}")

    def test_scrub_catches_a_multiword_unknown_account_whole_when_a_slash_follows(self) -> None:
        """A folder name can hold a space ("Alice Smith"). When more path follows, the whole segment
        -- spaces included -- must go, not just its first word: a previous version of this pattern
        stopped at the first space and left "C:\\Users\\<user> Smith\\Games" -- "Smith" leaking right
        next to the placeholder. "Alice Smith" is not set anywhere in this process's own environment,
        so only the generic path pattern can be what catches it."""
        contexts = {
            "backslash, drive": "seen at C:\\Users\\Alice Smith\\Games, reproduces every time",
            "forward slash, drive": "seen at C:/Users/Alice Smith/Games, reproduces every time",
            "UNC": "exe: \\\\server\\Users\\Alice Smith\\Documents\\lomse.exe",
            "Documents and Settings": "old profile: C:\\Documents and Settings\\Alice Smith\\lom.cfg",
            "posix": "home: /Users/Alice Smith/Games/LOM/lomhd.log",
        }
        for label, text in contexts.items():
            with self.subTest(label):
                scrubbed = setup.scrub(text)
                self.assertNotIn("Alice", scrubbed, f"{label}: {scrubbed!r}")
                self.assertNotIn("Smith", scrubbed, f"{label}: {scrubbed!r}")
                self.assertIn("<user>", scrubbed, f"{label}: {scrubbed!r}")

    def test_scrub_catches_a_multiword_unknown_account_with_no_slash_following(self) -> None:
        """The other half of the same fix: with nothing path-like after the name, it must still end at
        the first space rather than swallowing the rest of the sentence."""
        scrubbed = setup.scrub("seen at C:\\Users\\Alice Smith, then it hung")
        self.assertNotIn("Alice", scrubbed)
        self.assertIn("<user> Smith", scrubbed,
                      "with nothing path-like following, only the first word is the segment")
        self.assertIn(", then it hung", scrubbed, "the rest of the sentence must survive untouched")

    def test_scrub_never_touches_a_64_hex_run_even_when_it_contains_the_username(self) -> None:
        fake_sha256 = "1c2ada9f" + "0" * 56
        self.assertEqual(len(fake_sha256), 64)
        with mock.patch.dict(os.environ, {"USER": "ada", "USERNAME": "ada"}):
            text = f"lomse.exe: {fake_sha256} -- unknown"
            self.assertEqual(setup.scrub(text), text,
                             "a username that happens to be hex-shaped must survive inside a hash")

    def test_scrub_replaces_a_short_username_as_a_whole_word_but_not_a_longer_one(self) -> None:
        for name in ("ada", "lom"):
            with self.subTest(name), mock.patch.dict(os.environ, {"USER": name, "USERNAME": name}):
                whole_word = setup.scrub(f"installed by {name.upper()} on this machine")
                self.assertNotIn(name, whole_word.lower())
                self.assertIn("<user>", whole_word)
                # A longer identifier that merely starts with the name must not be corrupted: the
                # word-boundary lookahead requires a non-alnum character right after the match.
                longer_word = f"see {name}se.exe for details"
                self.assertEqual(setup.scrub(longer_word), longer_word)

    def test_a_short_account_name_is_not_scrubbed_standalone_but_still_is_in_a_path(self) -> None:
        """Deliberate, not a gap: under 3 characters is too likely to be noise as a bare word (see
        scrub()'s own docstring), so it survives standalone -- but the path patterns key off the
        path's own shape, not the account's length, so the same name is still caught there."""
        with mock.patch.dict(os.environ, {"USER": "Al", "USERNAME": "Al"}):
            standalone = setup.scrub("installed by AL on this machine")
            self.assertIn("AL", standalone, "a name under 3 characters is left alone standalone")
            in_a_path = setup.scrub("seen at C:\\Users\\Al\\Documents")
            self.assertNotIn("Al", in_a_path)
            self.assertIn("<user>", in_a_path)

    def test_report_never_prints_the_account_name_as_a_field(self) -> None:
        """report.txt has no field that echoes the account name on its own (no "account:" or
        "user:" line) -- the only way it could appear at all is inside scrub()bed prose, which the
        rest of this file's tests already hold to account. A short name is the case that matters here:
        it survives standalone (previous test), so the one thing left to guarantee is that nothing in
        report.txt ever puts it on display as a field in its own right."""
        with mock.patch.dict(os.environ, {"USER": "Al", "USERNAME": "Al"}):
            path = self.make_report()
        for line in self.text_in(path).splitlines():
            self.assertNotRegex(line.lower(), r"^\s*(account|user)\s*:",
                                f"report.txt must not have an account/user field: {line!r}")

    def test_non_ascii_account_name_is_absent_from_every_member(self) -> None:
        """Jos\u00e9 \u00c1lvaro as the account itself (getpass, USER/USERNAME, and the home path), not just
        some unrelated non-ASCII text -- it must be gone from every member, while other non-ASCII
        text (not the identity) survives untouched."""
        identity = "Jos\u00e9 \u00c1lvaro"
        self.addCleanup(setattr, pathlib.Path, "home", pathlib.Path.home)
        pathlib.Path.home = staticmethod(lambda: pathlib.Path(f"/Users/{identity}"))
        unrelated = "unrelated non-ascii text: caf\u00e9 na\u00efve"
        (self.game / "lomhd.log").write_text(
            f"crash reported by {identity}\nhome: /Users/{identity}/Games/LOM\n{unrelated}\n",
            encoding="utf-8")
        (self.game / setup.SUMMARY_NAME).write_text(f"played by {identity}\n", encoding="utf-8")
        with mock.patch.dict(os.environ, {"USER": identity, "USERNAME": identity}), \
             mock.patch("getpass.getuser", return_value=identity):
            path = self.make_report()
        for name in self.names_in(path):
            text = self.member_text(path, name)
            self.assertNotIn(identity, text, f"{name} still contains the account name")
            self.assertNotIn("\u00c1lvaro", text, f"{name} still contains part of the account name")
        self.assertIn(unrelated, self.member_text(path, "lomhd.log"),
                     "unrelated non-ascii text must survive untouched")

    # --- encoding ----------------------------------------------------------------------------------

    def test_decode_text_member_reads_utf16_with_a_bom(self) -> None:
        text = "crash near C:\\Users\\Alice\\lomse.exe"
        for encoding in ("utf-16-le", "utf-16-be"):
            with self.subTest(encoding):
                bom = b"\xff\xfe" if encoding == "utf-16-le" else b"\xfe\xff"
                data = bom + text.encode(encoding)
                self.assertEqual(setup.decode_text_member(data), text)

    def test_decode_text_member_reads_nul_heavy_utf16_without_a_bom(self) -> None:
        text = "crash near C:\\Users\\Alice\\lomse.exe"
        data = text.encode("utf-16-le")          # no BOM, but roughly half NUL bytes
        self.assertEqual(setup.decode_text_member(data), text)

    def test_decode_text_member_falls_back_to_cp1252_for_legacy_windows_text(self) -> None:
        # \xe9 is "e" in cp1252 (Windows-1252) but is not valid UTF-8 on its own.
        data = "caf\u00e9".encode("cp1252")
        with self.assertRaises(UnicodeDecodeError):
            data.decode("utf-8")
        self.assertEqual(setup.decode_text_member(data), "caf\u00e9")

    def test_decode_text_member_gives_up_rather_than_guess(self) -> None:
        # Genuinely arbitrary bytes: not a BOM, not NUL-heavy, and not valid under either codec tried
        # (0x81 and 0x8d are undefined in cp1252).
        self.assertIsNone(setup.decode_text_member(b"\x81\x8d\xff\xfe\xfe\xff\x00\x01\x02"))

    def test_a_text_member_that_cannot_be_decoded_is_left_out_not_copied_in(self) -> None:
        (self.game / "lomhd.log").write_bytes(b"\x81\x8d\xff\x00\x01\x02\x03\x04")
        path = self.make_report()
        self.assertNotIn("lomhd.log", self.names_in(path))
        self.assertIn("lomhd.log (could not be decoded confidently as text -- left out)",
                     self.text_in(path))

    # --- --with-save warning -------------------------------------------------------------------------

    def test_with_save_prints_a_warning_that_a_save_may_carry_in_game_names(self) -> None:
        saves = self.game / "savegame"
        saves.mkdir()
        (saves / "Water I").write_bytes(b"save data")
        warning = "a save may contain your in-game names"
        with mock.patch("builtins.print") as printed:
            self.make_report(with_save="latest")
        messages = [str(call.args[0]) if call.args else "" for call in printed.call_args_list]
        self.assertTrue(any(warning in m for m in messages),
                        f"no warning about in-game names printed; got {messages}")
        printed.reset_mock()
        for stale in self.release_dir.glob("lomhd-report-*.zip"):
            stale.unlink()
        with mock.patch("builtins.print") as printed:
            self.make_report()
        messages = [str(call.args[0]) if call.args else "" for call in printed.call_args_list]
        self.assertFalse(any(warning in m for m in messages),
                         "no savegame was requested, so no warning should be printed")

    # --- guards ------------------------------------------------------------------------------------

    def test_with_save_without_report_is_refused_before_find_game_runs(self) -> None:
        args = argparse.Namespace(game=None, uninstall=False, review=False, port=8765, terrain=False,
                                  sprites=False, no_sprites=False, force_terrain_folder=False,
                                  report=False, with_save="latest", with_dump=False)

        def boom(*a, **k):
            raise AssertionError("find_game must not run before the --with-save guard")

        with mock.patch.object(argparse.ArgumentParser, "parse_args", return_value=args), \
             mock.patch.object(setup, "find_game", boom):
            with self.assertRaises(SystemExit) as failed:
                setup.main()
        self.assertIn("--with-save only makes sense with --report", str(failed.exception))

    def test_with_dump_without_report_is_refused_before_find_game_runs(self) -> None:
        args = argparse.Namespace(game=None, uninstall=False, review=False, port=8765, terrain=False,
                                  sprites=False, no_sprites=False, force_terrain_folder=False,
                                  report=False, with_save=None, with_dump=True)

        def boom(*a, **k):
            raise AssertionError("find_game must not run before the --with-dump guard")

        with mock.patch.object(argparse.ArgumentParser, "parse_args", return_value=args), \
             mock.patch.object(setup, "find_game", boom):
            with self.assertRaises(SystemExit) as failed:
                setup.main()
        self.assertIn("--with-dump only makes sense with --report", str(failed.exception))

    # --- writing the zip -----------------------------------------------------------------------------

    def test_a_failed_member_leaves_no_partial_zip_and_keeps_an_earlier_report(self) -> None:
        first = self.make_report()
        first_bytes = first.read_bytes()
        real_writestr = zipfile.ZipFile.writestr

        def boom_on_report_txt(self_zip, zinfo_or_arcname, data, *a, **k):
            name = getattr(zinfo_or_arcname, "filename", zinfo_or_arcname)
            if name == "report.txt":
                raise RuntimeError("disk full")
            return real_writestr(self_zip, zinfo_or_arcname, data, *a, **k)

        self.addCleanup(setattr, zipfile.ZipFile, "writestr", real_writestr)
        zipfile.ZipFile.writestr = boom_on_report_txt
        with self.assertRaises(RuntimeError):
            setup.run_report(self.game, None, False)
        zipfile.ZipFile.writestr = real_writestr
        zips = sorted(self.release_dir.glob("lomhd-report-*.zip"))
        self.assertEqual(len(zips), 1, "the earlier report must survive a failed later run")
        self.assertEqual(zips[0].read_bytes(), first_bytes)
        self.assertEqual(list(self.release_dir.glob("*.part")), [], "no partial zip left behind")

    def freeze_now(self, *args) -> None:
        real_cls = setup.datetime.datetime
        self.addCleanup(setattr, setup.datetime, "datetime", real_cls)

        class Frozen(real_cls):
            @classmethod
            def now(cls, tz=None):
                return real_cls(*args)

        setup.datetime.datetime = Frozen

    def test_finalize_report_uses_the_next_name_when_the_first_is_taken(self) -> None:
        (self.release_dir / "lomhd-report-20260101-000000.zip").write_bytes(b"existing report")
        self.freeze_now(2026, 1, 1, 0, 0, 0)
        built = self.release_dir / "scratch.part"
        built.write_bytes(b"a freshly built report")
        out = setup.finalize_report(built)
        self.assertEqual(out.name, "lomhd-report-20260101-000000-2.zip")
        self.assertEqual(out.read_bytes(), b"a freshly built report")

    def test_finalize_report_removes_its_own_partial_file_when_the_copy_fails(self) -> None:
        """If this filesystem cannot hard-link (forced here) and the exclusive-create copy fallback
        then fails partway through -- after some bytes have already landed on disk, not before any
        write at all -- the candidate name this run just claimed must not be left behind holding a
        partial report, and a pre-existing report at a different name must be untouched."""
        self.freeze_now(2026, 1, 1, 0, 0, 0)
        (self.release_dir / "lomhd-report-20251231-235959.zip").write_bytes(b"an older, unrelated report")
        built = self.release_dir / "scratch.part"
        built.write_bytes(b"a freshly built report")
        self.addCleanup(setattr, setup.os, "link", setup.os.link)
        setup.os.link = mock.Mock(side_effect=OSError("cross-device link"))

        def partial_write_then_fail(src, dst):
            dst.write(b"x" * 128)     # some of the "report" really did reach disk before the failure
            raise OSError("disk full")

        self.addCleanup(setattr, setup.shutil, "copyfileobj", setup.shutil.copyfileobj)
        setup.shutil.copyfileobj = partial_write_then_fail
        with self.assertRaises(OSError):
            setup.finalize_report(built)
        self.assertEqual(list(self.release_dir.glob("lomhd-report-20260101-000000*.zip")), [],
                         "the candidate this run created must be removed after a failed copy, "
                         "partial bytes and all")
        self.assertEqual((self.release_dir / "lomhd-report-20251231-235959.zip").read_bytes(),
                         b"an older, unrelated report", "an unrelated earlier report must be untouched")

    def test_two_runs_racing_for_the_same_name_both_keep_their_report(self) -> None:
        """Simulates another process having just won the race for this run's timestamp a moment
        before finalize_report tries to claim it -- the real race is between two --report processes,
        which this drives through the public run_report() rather than by calling finalize_report
        directly, so the whole build-then-claim path is exercised, not just the naming loop."""
        self.freeze_now(2026, 1, 1, 0, 0, 0)
        (self.release_dir / "lomhd-report-20260101-000000.zip").write_bytes(b"the other run's report")
        setup.run_report(self.game, None, False)
        zips = sorted(self.release_dir.glob("lomhd-report-*.zip"))
        self.assertEqual([p.name for p in zips],
                         ["lomhd-report-20260101-000000-2.zip", "lomhd-report-20260101-000000.zip"])
        self.assertEqual((self.release_dir / "lomhd-report-20260101-000000.zip").read_bytes(),
                         b"the other run's report", "the pre-existing report must be untouched")
        self.assertEqual(list(self.release_dir.glob("*.part")), [], "no leftover temp file")


class Downloads(unittest.TestCase):
    """fetch(): a file already there is used only if its SHA-256 is the pinned one, checked in full
    on every run -- the downloaded models' integrity check."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dest = pathlib.Path(tmp.name) / "model.bin"
        # Longer than any block a hash might read at a time, with the difference at the end: a
        # hash of the first block alone must not pass for the whole file.
        self.model = b"x" * (1 << 21) + b"the model"
        want = hashlib.sha256(self.model).hexdigest()
        patcher = mock.patch.dict(setup.DOWNLOADS, {"model": ("https://example.invalid/model.bin", want)})
        patcher.start()
        self.addCleanup(patcher.stop)
        self.hashed: list = []
        real = setup.sha256
        self.addCleanup(setattr, setup, "sha256", real)
        setup.sha256 = lambda path: self.hashed.append(path.name) or real(path)
        offline = mock.patch.object(setup.urllib.request, "urlopen", side_effect=OSError("offline"))
        self.urlopen = offline.start()
        self.addCleanup(offline.stop)

    def test_the_pinned_file_is_used_and_hashed_in_full_on_every_run(self) -> None:
        self.dest.write_bytes(self.model)
        for run in (1, 2):
            self.assertEqual(setup.fetch("model", self.dest), self.dest)
            self.assertEqual(self.hashed, ["model.bin"] * run, "hashed every run, not only the first")
        self.urlopen.assert_not_called()

    def test_a_file_that_is_not_the_pinned_one_is_never_used(self) -> None:
        self.dest.write_bytes(self.model[:-1] + b"m")       # same size, last byte other
        with self.assertRaises(SystemExit):
            setup.fetch("model", self.dest)                   # found wrong, so fetched again: offline
        self.urlopen.assert_called_once()
        self.assertEqual(self.dest.read_bytes(), self.model[:-1] + b"m", "and not taken as the model")


class Timing(unittest.TestCase):
    def test_durations_read_as_a_person_would_say_them(self) -> None:
        self.assertEqual([setup.duration(t) for t in (0.4, 59.6, 134, 3600 + 125)],
                         ["0s", "1m 00s", "2m 14s", "1h 02m"])

    def test_each_step_says_what_it_took_and_the_summary_names_them_all(self) -> None:
        said: list = []
        with mock.patch.object(setup, "say", said.append), \
                mock.patch.object(setup.time, "monotonic", side_effect=[0.0, 3.0, 3.0, 137.0]):
            clock = setup.StepClock(5)
            clock.start(1, "Getting the upscaler")
            clock.start(2, "Reading")
            clock.stop()
            clock.stop()                                      # a second stop says nothing more
        self.assertEqual(said, ["1/5  Getting the upscaler", "     1/5 took 3s", "2/5  Reading",
                                "     2/5 took 2m 14s"])
        self.assertEqual(clock.summary(), "Time taken: 1/5 3s, 2/5 2m 14s (2m 17s in all)")

    def test_the_profile_counts_and_times_every_program_run(self) -> None:
        def stub(cmd, *a, **k):
            return None

        with mock.patch.object(setup.subprocess, "run", stub):
            with setup.SpawnProfile() as profile:
                for cmd in (["magick", "a"], ["magick", "b"], [r"C:\\x\\realesrgan-ncnn-vulkan.exe", "-i"],
                            [sys.executable, "/r/tools/upscale.py", "x"]):
                    setup.subprocess.run(cmd)
            self.assertIs(setup.subprocess.run, stub, "put back as it was")
        self.assertEqual(sorted(profile.by_program), ["magick", "python upscale.py", "realesrgan-ncnn-vulkan"])
        self.assertEqual(profile.by_program["magick"][0], 2)
        self.assertEqual(len(profile.lines()), 3)

    def test_a_bad_lomhd_jobs_stops_with_a_message(self) -> None:
        for value, want in (("3", 3), ("0", 1), (" 1 ", 1)):
            with mock.patch.dict(os.environ, {setup.hd_upscale.JOBS_ENV: value}):
                self.assertEqual(setup.hd_upscale.jobs(), want)
        with mock.patch.dict(os.environ, {setup.hd_upscale.JOBS_ENV: "many"}), self.assertRaises(SystemExit):
            setup.hd_upscale.jobs()
        with mock.patch.dict(os.environ, {setup.hd_upscale.JOBS_ENV: ""}):
            self.assertEqual(setup.hd_upscale.jobs(), max(1, min(os.cpu_count() or 1, setup.hd_upscale.MAX_JOBS)))

    def test_windows_never_gets_more_workers_than_its_process_pool_takes(self) -> None:
        with mock.patch.dict(os.environ, {setup.hd_upscale.JOBS_ENV: "64"}):
            with mock.patch.object(setup.hd_upscale.os, "name", "posix"):   # whatever runs the suite
                self.assertEqual(setup.hd_upscale.jobs(), 64, "the control: elsewhere it is taken as given")
            with mock.patch.object(setup.hd_upscale.os, "name", "nt"):
                self.assertEqual(setup.hd_upscale.jobs(), 61)


class MainWritesSummary(unittest.TestCase):
    """lomhd_last_summary.txt: written by a finished run of main(), read back by --report."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        self.game, self.release_dir = root / "game", root / "release"
        self.game.mkdir(); self.release_dir.mkdir()
        (self.game / "lomse.exe").write_bytes(b"pretend exe")
        (self.game / "pic.mpq").write_bytes(b"pretend pic")
        (self.game / "imp.mpq").write_bytes(b"pretend imp")
        (self.game / "ddraw.dll").write_bytes(ORIGINAL)
        (self.release_dir / "ddraw.dll").write_bytes(OURS)
        # Every step of main() except the summary write itself is stubbed out: this test is about
        # the one line at the end of a finished run, not about upscaling or installing.
        stubs = {
            "release": lambda: {"version": "9.9.9-test", "ddraw_sha256": hashlib.sha256(OURS).hexdigest()},
            "check_magick": lambda: None,
            "check_imp": lambda game: None,
            "check_writable": lambda game, terrain=False: None,
            "upscaler": lambda: (pathlib.Path("esrgan"), pathlib.Path("models")),
            "extract_images": lambda game: {"portrait": []},
            "plan_sprites": lambda game, animated: (
                types.SimpleNamespace(static=[], animated=[], skipped=[],
                                      counts={"frames": 0, "repeats": 0, "ineligible": 0, "no_probe": 0}),
                self.release_dir / "sprites", None),
            "upscale_all": lambda found, exe, models: [],
            "upscale_sprites": lambda *a, **k: None,
            "build_pack": lambda *a, **k: (3, [], [], {"packed": 3}, {"packed": 0, "sprites": 0}),
            "install": lambda *a, **k: None,
            "fix_exe": lambda game, terrain_arg="": "lomse.exe left as it is.",
            "HERE": self.release_dir,
        }
        for name, value in stubs.items():
            self.addCleanup(setattr, setup, name, getattr(setup, name))
            setattr(setup, name, value)
        self.addCleanup(setattr, sys, "argv", sys.argv)

    def run_setup(self) -> None:
        sys.argv = ["lomhd_setup.py", "--game", str(self.game)]
        self.assertEqual(setup.main(), 0)

    def test_a_finished_run_writes_the_summary_file(self) -> None:
        self.run_setup()
        summary = self.game / setup.SUMMARY_NAME
        self.assertTrue(summary.is_file())
        text = summary.read_text(encoding="utf-8")
        lines = text.splitlines()
        self.assertTrue(lines[0].startswith("date: "), lines[0])
        self.assertEqual(lines[1], "release: 9.9.9-test")
        self.assertIn("Done: 3 HD images installed", text)
        self.assertIn("To undo: python lomhd_setup.py --uninstall", text)
        self.assertRegex(lines[-1], r"^Time taken: 1/5 \d+s, 2/5 \d+s, 3/5 \d+s, 4/5 \d+s, 5/5 \d+s \(\d+s in all\)$")

    def test_report_reads_back_the_last_setup_summary(self) -> None:
        self.run_setup()
        self.addCleanup(setattr, setup, "gpu_name", setup.gpu_name)
        setup.gpu_name = lambda: None
        self.addCleanup(setattr, setup, "detect_wine", setup.detect_wine)
        setup.detect_wine = lambda: None
        setup.run_report(self.game, None, False)
        zips = list(self.release_dir.glob("lomhd-report-*.zip"))
        self.assertEqual(len(zips), 1)
        with zipfile.ZipFile(zips[0]) as z:
            text = z.read("report.txt").decode("utf-8")
        self.assertIn("Last setup summary:", text)
        self.assertIn("release: 9.9.9-test", text)
        self.assertIn("Done: 3 HD images installed", text)


if __name__ == "__main__":
    unittest.main()
