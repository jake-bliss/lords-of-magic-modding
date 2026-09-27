"""The marauder-probe edit to gs\\hotkey.gs (tools/marauder_probe.py).

The corpus tests are the decisive ones: against the installed vanilla gs.mpq the parser must account
for every `addhotkey`, the probe keys must be free, the edit must verify, and the engine's constant
table must agree with the one place the scripts spell the marauder slot as a number. The unit tests
pin the refusals the corpus cannot exercise, using binding shapes copied verbatim from the member.
"""

from __future__ import annotations

import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))

import gs_syntax  # noqa: E402
import marauder_probe as probe  # noqa: E402
import mpq_read  # noqa: E402

GAME = ("drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English")
VANILLA = (pathlib.Path.home() / "Applications/Steambuild 32 64bit DXVK.app/Contents/SharedSupport"
           / "prefix" / GAME)

# Shapes copied from the shipped gs\hotkey.gs.
LITERAL_FORM = b"ASCII_VAL 32{pausecombat}addhotkey"
STRING_FORM = b'ASCII_VAL"="0 get{select_next_army}addhotkey'
DUP_FORM = b"ASCII_VAL 90{getmultiplayerflag not incombat not and{formationview}if}dup ASCII_VAL exch 122 exch addhotkey addhotkey"
DUP_STRING_FORM = b'ASCII_VAL"L"0 get{incombat{90 rotatemap}if}bind dup ASCII_VAL exch"l"0 get exch addhotkey addhotkey'
VK_FORM = b"VK_VAL 112{getmultiplayerflag not{panel_dict begin hotkey_dlg opendialog end}if}addhotkey"


def member(*bindings: bytes) -> bytes:
    """A minimal single-line member holding the flag, the anchor and the given bindings."""
    return b" ".join([b"20 dict begin userdict begin", probe.FLAG_BEFORE, b"end", *bindings,
                      probe.ANCHOR, b"end"])


class BoundHotkeys(unittest.TestCase):
    def test_every_shipped_binding_shape_is_read(self) -> None:
        bound = probe.bound_hotkeys(member(LITERAL_FORM, STRING_FORM, DUP_FORM, DUP_STRING_FORM,
                                           VK_FORM))
        self.assertEqual(bound["ASCII_VAL"], {32, ord("="), 90, 122, ord("L"), ord("l"), 22})
        self.assertEqual(bound["VK_VAL"], {112})

    def test_the_two_namespaces_are_kept_apart(self) -> None:
        """VK_VAL 46 (Delete) and ASCII_VAL 46 (.) are different keys (observed 2026-09-19)."""
        bound = probe.bound_hotkeys(b"VK_VAL 46{a}addhotkey ASCII_VAL 44{b}addhotkey")
        self.assertEqual(bound, {"ASCII_VAL": {44}, "VK_VAL": {46}})

    def test_an_unknown_code_shape_is_refused(self) -> None:
        with self.assertRaises(probe.ProbeError):
            probe.bound_hotkeys(b"ASCII_VAL somekey{a}addhotkey")

    def test_an_addhotkey_with_no_code_site_is_refused(self) -> None:
        """A registration the parser cannot see could hide a collision."""
        with self.assertRaises(probe.ProbeError):
            probe.bound_hotkeys(b"ASCII_VAL 44{b}addhotkey some_code{c}addhotkey")


class ApplyAndVerify(unittest.TestCase):
    def test_apply_flips_the_flag_and_inserts_before_the_anchor(self) -> None:
        shipped = member(LITERAL_FORM)
        edited = probe.apply(shipped)
        self.assertIn(probe.FLAG_AFTER, edited)
        self.assertNotIn(probe.FLAG_BEFORE, edited)
        self.assertIn(probe.PROBE_BLOCK + probe.ANCHOR, edited)
        self.assertEqual(len(edited) - len(shipped),
                         len(probe.FLAG_AFTER) - len(probe.FLAG_BEFORE) + len(probe.PROBE_BLOCK))
        probe.verify(shipped, edited)

    def test_a_probe_key_already_bound_is_refused(self) -> None:
        for key in probe.PROBE_KEYS:
            with self.subTest(key), self.assertRaises(probe.ProbeError):
                probe.apply(member(b'ASCII_VAL"' + key.encode() + b'"0 get{a}addhotkey'))

    def test_a_missing_or_repeated_anchor_or_flag_is_refused(self) -> None:
        shipped = member(LITERAL_FORM)
        for broken in (shipped.replace(probe.ANCHOR, b""),
                       shipped + b" " + probe.ANCHOR,
                       shipped.replace(probe.FLAG_BEFORE, b""),
                       shipped + b" " + probe.FLAG_BEFORE):
            with self.subTest(len(broken)), self.assertRaises(probe.ProbeError):
                probe.apply(broken)

    def test_a_member_with_line_endings_is_refused(self) -> None:
        with self.assertRaises(probe.ProbeError):
            probe.apply(member(LITERAL_FORM).replace(b" ", b"\r", 1))

    def test_applying_twice_is_refused(self) -> None:
        once = probe.apply(member(LITERAL_FORM))
        with self.assertRaises(probe.ProbeError):
            probe.apply(once.replace(probe.FLAG_AFTER, probe.FLAG_BEFORE))

    def test_any_other_byte_change_fails_verification(self) -> None:
        shipped = member(LITERAL_FORM)
        edited = probe.apply(shipped)
        for position in (0, len(edited) // 2, len(edited) - 1):
            mutated = bytearray(edited)
            mutated[position] ^= 0x01
            with self.subTest(position), self.assertRaises(probe.ProbeError):
                probe.verify(shipped, bytes(mutated))


class ProbeBlock(unittest.TestCase):
    def test_the_block_is_single_line_ascii(self) -> None:
        self.assertNotIn(b"\r", probe.PROBE_BLOCK)
        self.assertNotIn(b"\n", probe.PROBE_BLOCK)
        probe.PROBE_BLOCK.decode("ascii")

    def test_each_body_balances_its_braces(self) -> None:
        for key, body in probe.PROBE_KEYS.items():
            tokens = gs_syntax.tokens(body)
            with self.subTest(key):
                self.assertEqual(tokens.count("{"), tokens.count("}"))
                depth = 0
                for token in tokens:
                    depth += (token == "{") - (token == "}")
                    self.assertGreaterEqual(depth, 0)

    def test_each_body_binds_exactly_its_own_key(self) -> None:
        for key, body in probe.PROBE_KEYS.items():
            with self.subTest(key):
                self.assertEqual(probe.bound_hotkeys(body.encode()),
                                 {"ASCII_VAL": {ord(key)}, "VK_VAL": set()})

    def test_no_body_defines_a_name(self) -> None:
        """A hotkey runs under whatever dictionary stack is current; a def would land anywhere."""
        tokens = gs_syntax.tokens(probe.PROBE_BLOCK.decode())
        self.assertNotIn("def", tokens)
        self.assertFalse([t for t in tokens if t.startswith("/") and t != "/decr1"])

    def test_every_body_is_single_player_only(self) -> None:
        for key, body in probe.PROBE_KEYS.items():
            with self.subTest(key):
                self.assertTrue(body.split("{", 1)[1].startswith("getmultiplayerflag not"))

    def test_the_destructive_and_undefined_cheat_keys_are_not_probe_keys(self) -> None:
        """Y is destroyterrainsprite and S calls the undefined superduper (cheat-keys-ladder.md)."""
        self.assertFalse({ord("Y"), ord("y"), ord("S"), ord("s")} & probe.probe_codes())


class EngineConstant(unittest.TestCase):
    def test_a_non_pe_file_is_refused(self) -> None:
        with self.assertRaises(probe.ProbeError):
            probe.engine_constant(b"not an executable", "WANDERING_MONSTER_PLAYER")


@unittest.skipUnless((VANILLA / "gs.mpq").is_file(), "vanilla profile not installed")
class Corpus(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        archive = mpq_read.Archive(VANILLA / "gs.mpq")
        cls.hotkey = archive.read(probe.HOTKEY_MEMBER)
        cls.placedng = archive.read("gs\\placedng.gs")

    def test_the_parser_accounts_for_every_shipped_binding(self) -> None:
        bound = probe.bound_hotkeys(self.hotkey)
        # Keys the run sheet names, confirmed bound: `*` (cheat move points), `.` (game speed),
        # E (end turn), Ctrl+S / Ctrl+L (save / load), Y and S (the keys never to press).
        for code in (ord("*"), ord("."), ord("E"), 19, 12, ord("Y"), ord("S")):
            self.assertIn(code, bound["ASCII_VAL"])

    def test_the_probe_keys_are_free_and_the_edit_verifies(self) -> None:
        self.assertFalse(probe.probe_codes() & probe.bound_hotkeys(self.hotkey)["ASCII_VAL"])
        probe.verify(self.hotkey, probe.apply(self.hotkey))

    def test_the_marauder_slot_agrees_between_engine_and_scripts(self) -> None:
        self.assertEqual(self.placedng.count(probe.PLACEDNG_LITERAL), 1)
        exe = (VANILLA / "lomse.exe").read_bytes()
        self.assertEqual(probe.engine_constant(exe, "WANDERING_MONSTER_PLAYER"), 15)
        # A neighbour in the same table, as a control that the reader is reading the table
        # (MAX_PLAYERS sits beside it; 16 slots is what every 0..15 loop in the scripts assumes).
        self.assertEqual(probe.engine_constant(exe, "MAX_PLAYERS"), 16)


if __name__ == "__main__":
    unittest.main()
