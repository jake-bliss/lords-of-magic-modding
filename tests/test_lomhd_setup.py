"""The HD overlay setup's install and uninstall: the player's ddraw.dll must always come back.

These drive `install`/`uninstall` directly against a temporary game folder. The extraction and
upscaling steps are covered by test_mpq_read.py and the end-to-end run in docs/hd-overlay.md."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import shutil
import struct
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
        self.palette = [(i, i, i) for i in range(256)]
        self.lbm_png, self.struct = lbm_png, struct
        self.seen: dict[str, bytes] = {}

        def render(option, inputs, dest, esrgan, models):
            for key, png in inputs.items():
                self.seen[key] = png.read_bytes()
            return len(inputs)

        def run(cmd, check=False):         # magick PPM -> PNG, stubbed as a copy
            pathlib.Path(cmd[2].removeprefix("PNG:")).write_bytes(pathlib.Path(cmd[1]).read_bytes())

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

    def add(self, member: str, *frames) -> None:
        self.members[member.lower()] = self.imp_file(list(frames))
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
        self.hd_sprites.write_png_rgba(upscaled / "aagtwr0a.png", 80, 12, bytes(range(256)) * 15)
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
                "terrain-stride-1024": terrain_set(sha, (0x401016, "c1 fb 07", "c1 fb 06"))}
        for name, body in sets.items():
            (self.release_dir / "exe_patches" / f"{name}.json").write_text(json.dumps(body))
        self.patched = setup.exe_patch.apply(
            PRISTINE_EXE, [setup.exe_patch.load_set(self.release_dir / "exe_patches" / f"{n}.json")
                           for n in setup.TERRAIN_SETS])
        (self.built / "tilesa01.lbm").write_bytes(b"pretend 2x atlas")
        (self.built / "tilesa01.til").write_bytes(b"TILESIZE= 64, 64")
        self.record = {"version": "test", "ddraw_sha256": hashlib.sha256(OURS).hexdigest()}
        for name, value in (("HERE", self.release_dir), ("say", lambda text: None),
                            ("PRISTINE_EXE_SHA256", sha),
                            ("PATCHED_EXE_SHA256", hashlib.sha256(self.patched).hexdigest())):
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
    """The release ships the terrain sets as JSON (Python 3.9 has no tomllib)."""

    SETS = ROOT / "tools" / "exe_patches"

    def test_json_sets_match_the_toml_sets_edit_for_edit(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            for name in setup.TERRAIN_SETS:
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
            sets = []
            for name in setup.TERRAIN_SETS:
                path = pathlib.Path(tmp) / f"{name}.json"
                path.write_text(setup.exe_patch.to_json(self.SETS / f"{name}.toml"))
                sets.append(setup.exe_patch.load_set(path))
            self.assertEqual(len(setup.exe_patch.plan(image, sets)), 65)
            self.assertEqual(hashlib.sha256(setup.exe_patch.apply(image, sets)).hexdigest(),
                             setup.PATCHED_EXE_SHA256)


if __name__ == "__main__":
    unittest.main()
