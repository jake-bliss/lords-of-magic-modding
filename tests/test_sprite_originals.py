"""Tests for the sprite export's stale-export guard."""

import io
import stat
import struct
import subprocess
import sys
import tempfile
import textwrap
import unittest
import zlib
from contextlib import redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools" / "hd-review"))

import sprite_originals  # noqa: E402


def chunk(kind: bytes, payload: bytes) -> bytes:
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))


def one_pixel_png(plte: bytes) -> bytes:
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 3, 0, 0, 0))
            + chunk(b"PLTE", plte) + chunk(b"IDAT", zlib.compress(b"\x00\x02"))
            + chunk(b"IEND", b""))


def raw_imp(entries: list[tuple[int, int, int]]) -> bytes:
    """Just enough IMP for the check: the palette offset at byte 8, entries stored B, G, R, pad."""
    header = bytearray(32)
    struct.pack_into("<I", header, 8, 32)
    padded = entries + [(0, 0, 0)] * (256 - len(entries))
    return bytes(header) + b"".join(bytes((b, g, r, 0)) for r, g, b in padded)


# As the shipped files hold them (R, G, B), with red and green differing so the decodes differ.
ENTRIES = [(0, 255, 0), (255, 0, 0), (200, 40, 16)]
FIXED = b"".join(bytes(e) for e in ENTRIES + [(0, 0, 0)] * 253)
SWAPPED = b"".join(bytes((g, r, b)) for r, g, b in ENTRIES + [(0, 0, 0)] * 253)


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
        sprite_originals.discard_stale(self.out)
        self.assertTrue((self.out / "notes" / "sprite__tree2b.png").exists())

    def symlink_anime4x(self) -> Path:
        elsewhere = Path(tempfile.mkdtemp())
        (elsewhere / "sprite__tree2b.png").write_bytes(b"not ours to delete")
        (self.out / "anime4x").symlink_to(elsewhere, target_is_directory=True)
        return elsewhere

    def test_a_symlinked_review_folder_stops_the_run_before_anything_is_deleted(self) -> None:
        elsewhere = self.symlink_anime4x()     # sorts after folders that would be cleared first
        with self.assertRaises(SystemExit):
            sprite_originals.discard_stale(self.out)
        self.assertTrue((elsewhere / "sprite__tree2b.png").exists())
        self.assertEqual(len(list(self.out.glob("*/sprite__*.png"))), 4)

    def test_a_current_stamp_does_not_excuse_a_symlinked_folder(self) -> None:
        self.stamp(sprite_originals.EXPORT_VERSION + "\n")
        self.symlink_anime4x()
        with self.assertRaises(SystemExit):
            sprite_originals.discard_stale(self.out)


class MainOrderTest(unittest.TestCase):
    """Check the viewer, then discard, then stamp: a refused viewer must change nothing."""

    def setUp(self) -> None:
        self.out = Path(tempfile.mkdtemp())
        (self.out / "original").mkdir()
        (self.out / "original" / "sprite__tree2b.png").write_bytes(b"old")
        self.listfile = self.out / "names.txt"
        self.listfile.write_text("")             # no members: only the setup steps run
        self.real = sprite_originals.viewer_decodes_bgr

    def tearDown(self) -> None:
        sprite_originals.viewer_decodes_bgr = self.real

    def run_main(self, fixed: bool) -> None:
        sprite_originals.viewer_decodes_bgr = lambda *a, **k: fixed
        argv = sys.argv
        sys.argv = ["sprite_originals.py", "imp.mpq", str(self.out), "--listfile", str(self.listfile)]
        try:
            sprite_originals.main()
        finally:
            sys.argv = argv

    def test_an_old_viewer_changes_nothing(self) -> None:
        with self.assertRaises(SystemExit):
            self.run_main(fixed=False)
        self.assertTrue((self.out / "original" / "sprite__tree2b.png").exists())
        self.assertFalse((self.out / "original" / sprite_originals.STAMP).exists())

    def test_a_fixed_viewer_discards_then_stamps(self) -> None:
        self.run_main(fixed=True)
        self.assertFalse((self.out / "original" / "sprite__tree2b.png").exists())
        self.assertEqual((self.out / "original" / sprite_originals.STAMP).read_text().strip(),
                         sprite_originals.EXPORT_VERSION)


class ViewerCheckTest(unittest.TestCase):
    """A stand-in viewer that exports a PNG with a given PLTE, checked against the member's bytes."""

    def viewer(self, plte: bytes, fail_first: int = 0, first: bytes | None = None) -> Path:
        """`first`, when given, is what the first export writes instead (a broken PNG, say)."""
        folder = Path(tempfile.mkdtemp())
        png = folder / "export.png"
        png.write_bytes(one_pixel_png(plte))
        bad = folder / "first.png"
        bad.write_bytes(first or b"")
        counter = folder / "count"
        script = folder / "viewer"
        # argv as the real call passes it: --export-imp-frame ARCHIVE MEMBER FRAME OUTPUT --listfile L
        script.write_text(textwrap.dedent(f"""\
            #!{sys.executable}
            import pathlib, shutil, sys
            assert sys.argv[1] == "--export-imp-frame" and sys.argv[6] == "--listfile"
            counter = pathlib.Path({str(counter)!r})
            n = int(counter.read_text()) if counter.exists() else 0
            counter.write_text(str(n + 1))
            if n < {fail_first}:
                sys.exit(1)
            shutil.copy({str(bad)!r} if n == 0 and {first is not None} else {str(png)!r}, sys.argv[5])
            """))
        script.chmod(script.stat().st_mode | stat.S_IXUSR)
        return script

    def check(self, viewer: Path, members=None) -> bool:
        imp = raw_imp(ENTRIES)
        return sprite_originals.viewer_decodes_bgr(
            viewer, Path("imp.mpq"), ["a.imp", "b.imp"], Path("list.txt"),
            read_member=(members or {"a.imp": imp, "b.imp": imp}).__getitem__)

    def test_the_fixed_decode_passes(self) -> None:
        self.assertTrue(self.check(self.viewer(FIXED)))

    def test_the_old_swapped_decode_is_refused(self) -> None:
        self.assertFalse(self.check(self.viewer(SWAPPED)))

    def test_the_answer_comes_from_the_members_bytes_not_its_colours(self) -> None:
        # Slots stored red then green: a colour heuristic would call the fixed decode old.
        imp = raw_imp([(255, 0, 0), (0, 255, 0), (200, 40, 16)])
        plte = b"".join(bytes(e) for e in [(255, 0, 0), (0, 255, 0), (200, 40, 16)] + [(0, 0, 0)] * 253)
        self.assertTrue(self.check(self.viewer(plte), {"a.imp": imp, "b.imp": imp}))

    def test_a_failed_export_moves_on_to_the_next_sprite(self) -> None:
        self.assertTrue(self.check(self.viewer(FIXED, fail_first=1)))

    def test_an_export_with_broken_deflate_data_moves_on_to_the_next_sprite(self) -> None:
        # Valid chunk CRCs, but the IDAT payload is not a zlib stream.
        broken = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 3, 0, 0, 0))
                  + chunk(b"PLTE", FIXED) + chunk(b"IDAT", b"not deflate") + chunk(b"IEND", b""))
        self.assertTrue(self.check(self.viewer(FIXED, first=broken)))

    def test_neither_decode_refuses_rather_than_guessing(self) -> None:
        with self.assertRaises(SystemExit):
            self.check(self.viewer(b"\x10\x20\x30" * 256))


def fake_png_bytes() -> bytes:
    """A structurally valid, minimal PNG `clear_shadow` can parse (it only reads chunk framing,
    never decodes pixels): IHDR, IDAT, IEND, each followed by 4 placeholder CRC bytes clear_shadow
    never checks."""
    def chunk(kind: bytes, payload: bytes) -> bytes:
        return struct.pack(">I", len(payload)) + kind + payload + b"\0\0\0\0"
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", b"\0" * 13) + chunk(b"IDAT", b"\0" * 4) + chunk(b"IEND", b"")


class MembershipResolutionTest(unittest.TestCase):
    """Fix: membership must be resolved (every spelling of a name tried against the archive)
    before names are deduplicated by basename, and two different members sharing a name must be
    reported as an ambiguous collision rather than one being silently exported under the other's
    name -- the same bug, and the same fix, as `sprite_pack.py`'s own (both now share
    `imp_members.py`)."""

    def setUp(self) -> None:
        self.out = Path(tempfile.mkdtemp())
        (self.out / "original").mkdir()
        self.listfile = self.out / "names.txt"
        for attr in ("viewer_decodes_bgr", "describe"):
            self.addCleanup(setattr, sprite_originals, attr, getattr(sprite_originals, attr))
        self.addCleanup(setattr, sprite_originals.subprocess, "run", sprite_originals.subprocess.run)
        sprite_originals.viewer_decodes_bgr = lambda *a, **k: True

    def run_main(self, describes: dict[str, list], exports: set[str]) -> str:
        def fake_describe(viewer, archive, member, listfile):
            if member not in describes:
                raise SystemExit(f"{member}: not in this archive")
            return describes[member]

        def fake_run(cmd, **kwargs):
            if cmd[0] == "magick":
                if cmd[1] == "identify":
                    return subprocess.CompletedProcess(cmd, 0, stdout="20 20", stderr="")
                return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")
            member = cmd[3]                  # --export-imp-frame ARCHIVE MEMBER FRAME OUTPUT ...
            if member not in exports:
                return subprocess.CompletedProcess(cmd, 1, stdout="", stderr="export failed")
            Path(cmd[5]).write_bytes(fake_png_bytes())
            return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")

        sprite_originals.describe = fake_describe
        sprite_originals.subprocess.run = fake_run
        argv = sys.argv
        sys.argv = ["sprite_originals.py", "imp.mpq", str(self.out), "--listfile", str(self.listfile)]
        buf = io.StringIO()
        try:
            with redirect_stdout(buf):
                sprite_originals.main()
        finally:
            sys.argv = argv
        return buf.getvalue()

    def test_a_present_spelling_is_found_even_when_a_different_spelling_is_not(self) -> None:
        """`aura\\agx06b.imp` and `imp\\agx06b.imp` both come from the recovered listfile; this
        archive only has the second. Deduplicating by basename before checking membership can pick
        the absent spelling and skip the name outright."""
        self.listfile.write_text("aura\\agx06b.imp\nimp\\agx06b.imp\n")
        sequences = [{"label": "STAND", "facings": [[0]]}]
        output = self.run_main({"imp\\agx06b.imp": sequences}, exports={"imp\\agx06b.imp"})
        self.assertTrue((self.out / "original" / "sprite__agx06b.png").exists())
        self.assertIn("1 sprites written, 0 skipped", output)

    def test_two_present_members_sharing_a_name_are_reported_as_a_collision(self) -> None:
        """If BOTH spellings are present, they may be two unrelated sprites that happen to share a
        basename -- exporting either one under the shared name would be a guess."""
        self.listfile.write_text("aura\\agx06b.imp\nimp\\agx06b.imp\n")
        sequences = [{"label": "STAND", "facings": [[0]]}]
        output = self.run_main({"aura\\agx06b.imp": sequences, "imp\\agx06b.imp": sequences}, exports=set())
        self.assertFalse((self.out / "original" / "sprite__agx06b.png").exists())
        self.assertIn("agx06b:", output)
        self.assertIn("ambiguous", output)
        self.assertIn("0 sprites written, 1 skipped", output)

    def test_an_already_exported_name_is_not_re_resolved(self) -> None:
        """Resumability: a name whose PNG already exists must not cost a --describe-imp call at
        all, membership-collision or not. The stamp must already match, or `discard_stale` (which
        `main` runs first) would clear the PNG for an unrelated reason before this is tested."""
        self.listfile.write_text("aura\\agx06b.imp\nimp\\agx06b.imp\n")
        (self.out / "original" / sprite_originals.STAMP).write_text(sprite_originals.EXPORT_VERSION + "\n")
        (self.out / "original" / "sprite__agx06b.png").write_bytes(b"already there")
        output = self.run_main({}, exports=set())          # describe() would raise for every member
        self.assertEqual((self.out / "original" / "sprite__agx06b.png").read_bytes(), b"already there")
        self.assertIn("1 sprites written, 0 skipped", output)


if __name__ == "__main__":
    unittest.main()
