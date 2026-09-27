"""The rung-2 save edit (tools/marauder_save_bind.py and examples/save_bind_user.rs).

Every behavioural test runs against real saves from the installed vanilla profile, copied into a
temporary directory first; nothing here writes near a game folder. The corpus is also what the
edit's preconditions are asserted against: every shipped save binds user record *i* to player *i*
(so the target names its own index and no record names 15), and record 1's mode word equals record
0's, which the user switch reads.
"""

from __future__ import annotations

import pathlib
import shutil
import struct
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))

import marauder_save_bind as bind_tool  # noqa: E402

GAME = ("drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English")
SAVES = (pathlib.Path.home() / "Applications/Steambuild 32 64bit DXVK.app/Contents/SharedSupport"
         / "prefix" / GAME / "savegame")


def corpus() -> list[pathlib.Path]:
    return sorted(p for p in SAVES.iterdir() if p.is_file()) if SAVES.is_dir() else []


class CheckOutput(unittest.TestCase):
    def test_seven_records_are_refused(self) -> None:
        text = "\n".join(f"record {i}  offset 0x{i:x}  player {i}" for i in range(7))
        with self.assertRaises(bind_tool.BindError):
            bind_tool.parse_check_output(text)

    def test_a_malformed_line_is_refused(self) -> None:
        with self.assertRaises(bind_tool.BindError):
            bind_tool.parse_check_output("record 0 at 0x10 is player 0")


@unittest.skipUnless(corpus(), "vanilla profile saves not installed")
class Corpus(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)

    def copy(self, save: pathlib.Path) -> pathlib.Path:
        target = self.tmp / save.name
        shutil.copyfile(save, target)
        return target

    def test_every_shipped_save_binds_user_i_to_player_i(self) -> None:
        for save in corpus():
            with self.subTest(save.name):
                bindings = bind_tool.check(save)
                self.assertEqual([b.player for b in bindings], list(range(8)))
                data = save.read_bytes()
                offsets = [b.offset for b in bindings]
                self.assertEqual(data[offsets[0] - 8: offsets[0] - 1], b"LS_USER")
                self.assertEqual([b - a for a, b in zip(offsets, offsets[1:])], [784] * 7)

    def test_bind_changes_only_the_word_and_keeps_a_byte_backup(self) -> None:
        for save in corpus():
            with self.subTest(save.name):
                source = self.copy(save)
                original = source.read_bytes()
                out = self.tmp / f"{save.name}.bound"
                backups = self.tmp / "backups"
                bind_tool.bind(source, out, backups, applications_dir=self.tmp / "apps")
                self.assertEqual(source.read_bytes(), original)
                self.assertEqual((backups / (source.name + ".orig")).read_bytes(), original)
                edited = out.read_bytes()
                offset = bind_tool.check(source)[bind_tool.DEFAULT_RECORD].offset
                self.assertEqual(len(edited), len(original))
                moved = [i for i, (a, b) in enumerate(zip(original, edited)) if a != b]
                self.assertTrue(moved)
                self.assertTrue(all(offset <= i < offset + 4 for i in moved), moved)
                self.assertEqual(struct.unpack_from("<I", edited, offset)[0], 15)
                self.assertEqual([b.player for b in bind_tool.check(out)],
                                 [0, 15, 2, 3, 4, 5, 6, 7])

    def test_the_switch_precondition_holds_across_the_corpus(self) -> None:
        """Record 1's mode word (+0x2E8, read by the user switch) equals record 0's."""
        for save in corpus():
            with self.subTest(save.name):
                data = save.read_bytes()
                offsets = [b.offset for b in bind_tool.check(save)]
                modes = [struct.unpack_from("<i", data, o + bind_tool.MODE_OFFSET)[0]
                         for o in offsets]
                self.assertEqual(modes[1], modes[0], modes)

    def test_a_record_whose_mode_differs_from_record_0_is_refused(self) -> None:
        source = self.copy(corpus()[0])
        data = bytearray(source.read_bytes())
        offset = bind_tool.check(source)[1].offset + bind_tool.MODE_OFFSET
        data[offset] ^= 0x01
        source.write_bytes(bytes(data))
        with self.assertRaises(bind_tool.BindError):
            bind_tool.bind(source, self.tmp / "out", self.tmp / "b",
                           applications_dir=self.tmp / "apps")
        self.assertFalse((self.tmp / "out").exists())

    def test_record_0_can_still_be_chosen_explicitly(self) -> None:
        source = self.copy(corpus()[0])
        out = self.tmp / "zero"
        bind_tool.bind(source, out, self.tmp / "b", applications_dir=self.tmp / "apps", record=0)
        self.assertEqual([b.player for b in bind_tool.check(out)], [15, 1, 2, 3, 4, 5, 6, 7])

    def test_a_record_out_of_range_is_refused(self) -> None:
        source = self.copy(corpus()[0])
        for record in (-1, 8):
            with self.subTest(record), self.assertRaises(bind_tool.BindError):
                bind_tool.bind(source, self.tmp / "out", self.tmp / "b",
                               applications_dir=self.tmp / "apps", record=record)

    def test_an_already_bound_save_is_refused(self) -> None:
        source = self.copy(corpus()[0])
        once = self.tmp / "once"
        bind_tool.bind(source, once, self.tmp / "b", applications_dir=self.tmp / "apps")
        with self.assertRaises(bind_tool.BindError):
            bind_tool.bind(once, self.tmp / "twice", self.tmp / "b",
                           applications_dir=self.tmp / "apps")
        self.assertFalse((self.tmp / "twice").exists())

    def test_existing_output_and_in_place_are_refused(self) -> None:
        source = self.copy(corpus()[0])
        taken = self.tmp / "taken"
        taken.write_bytes(b"x")
        for out in (taken, source):
            with self.subTest(out.name), self.assertRaises(bind_tool.BindError):
                bind_tool.bind(source, out, self.tmp / "b", applications_dir=self.tmp / "apps")
        self.assertEqual(taken.read_bytes(), b"x")

    def test_a_backup_dir_outside_the_dev_profile_under_applications_is_refused(self) -> None:
        source = self.copy(corpus()[0])
        apps = self.tmp / "apps"
        (apps / "Lords of Magic 3.02.app").mkdir(parents=True)
        with self.assertRaises(bind_tool.BindError):
            bind_tool.bind(source, self.tmp / "out", apps / "Lords of Magic 3.02.app" / "bk",
                           applications_dir=apps, running=lambda: False)
        self.assertFalse((apps / "Lords of Magic 3.02.app" / "bk").exists())
        self.assertFalse((self.tmp / "out").exists())

    def test_inside_applications_only_the_dev_profile_and_only_with_the_game_closed(self) -> None:
        source = self.copy(corpus()[0])
        apps = self.tmp / "apps"
        other = apps / "Lords of Magic 3.02.app" / "savegame"
        dev = apps / "Lords of Magic Development.app" / "savegame"
        other.mkdir(parents=True)
        dev.mkdir(parents=True)
        with self.assertRaises(bind_tool.BindError):
            bind_tool.bind(source, other / "x", self.tmp / "b", applications_dir=apps,
                           running=lambda: False)
        with self.assertRaises(bind_tool.BindError):
            bind_tool.bind(source, dev / "x", self.tmp / "b", applications_dir=apps,
                           running=lambda: True)
        bind_tool.bind(source, dev / "x", self.tmp / "b", applications_dir=apps,
                       running=lambda: False)
        self.assertFalse((other / "x").exists())
        self.assertTrue((dev / "x").exists())

    def test_a_damaged_save_is_refused(self) -> None:
        source = self.copy(corpus()[0])
        data = source.read_bytes()
        offset = bind_tool.check(source)[0].offset
        # Cut 100 bytes out of LS_USER: no longer eight records of 784.
        source.write_bytes(data[:offset + 10] + data[offset + 110:])
        with self.assertRaises(bind_tool.BindError):
            bind_tool.bind(source, self.tmp / "out", self.tmp / "b",
                           applications_dir=self.tmp / "apps")
        self.assertFalse((self.tmp / "out").exists())

    def test_a_different_existing_backup_is_never_overwritten(self) -> None:
        source = self.copy(corpus()[0])
        backups = self.tmp / "b"
        backups.mkdir()
        stale = backups / (source.name + ".orig")
        stale.write_bytes(b"an older save")
        with self.assertRaises(bind_tool.BindError):
            bind_tool.bind(source, self.tmp / "out", backups, applications_dir=self.tmp / "apps")
        self.assertEqual(stale.read_bytes(), b"an older save")
        self.assertFalse((self.tmp / "out").exists())


if __name__ == "__main__":
    unittest.main()
