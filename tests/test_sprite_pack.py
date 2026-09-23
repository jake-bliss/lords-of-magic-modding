"""The sprite pack builder's selection logic: which static sprites get packed, and why the rest
don't. Every game-dependent piece (the asset viewer, ImageMagick) is a fake or a stub -- no archive,
listfile or render pipeline is needed to run this file."""

from __future__ import annotations

import pathlib
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "tools" / "hd-review"))

import hd_portrait_pack as pack  # noqa: E402
import sprite_pack  # noqa: E402


def chunk(kind: bytes, payload: bytes) -> bytes:
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))


def indexed_png(w: int, h: int, indices: bytes, plte: bytes, trns: bytes | None = None) -> bytes:
    """A minimal colour-type-3 PNG, built by hand -- same shape `--export-imp-frame` writes."""
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        raw.extend(indices[y * w:(y + 1) * w])
    chunks = [chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 3, 0, 0, 0)), chunk(b"PLTE", plte)]
    if trns is not None:
        chunks.append(chunk(b"tRNS", trns))
    chunks.append(chunk(b"IDAT", zlib.compress(bytes(raw))))
    chunks.append(chunk(b"IEND", b""))
    return b"\x89PNG\r\n\x1a\n" + b"".join(chunks)


def masked_sprite(w: int, h: int, key: int, opaque: int = 3, border: int = 2) -> bytes:
    """`border` columns of `key` on each side, colours counting up from `opaque` in between (the
    overlay makes no probe of a run of one colour), `h` identical rows -- a run of `w - 2 * border`
    on every row."""
    inside = [10 + (opaque * 16 + x) % 200 for x in range(w - 2 * border)]
    row = bytes([key] * border + [v if v != key else v + 1 for v in inside] + [key] * border)
    return row * h


PLTE = bytes(v for i in range(256) for v in (i, 255 - i, (i * 7) % 256))


def describe(frame_count: int) -> str:
    """Enough of --describe-imp's own tab-separated output for `frame_count` to count correctly:
    one `sequence`/`facing` line and `frame_count` `frame` lines."""
    lines = ["record\tindex\towner\tlabels-or-flags\tmetadata-or-size\tfirst\tcount\tplacement",
             f"sequence\t0\t-\tSTAND\t\tfacing:0;frame:0\tfacing:1;frame:{frame_count}\t-",
             f"facing\t0\tsequence:0\t-\t0x0000\tframe:0\tframe:{frame_count}\t-"]
    lines += [f"frame\t{i}\tsequence:0;facing:0;offset:{i}\t0x00;direct\t8x8\t-\t0\torigin:0:0"
             for i in range(frame_count)]
    return "\n".join(lines) + "\n"


class FakeViewer:
    """Stands in for the asset viewer's subprocess calls: `--describe-imp` and
    `--export-imp-frame` only, keyed by member name. Anything else is a test bug."""

    def __init__(self, describes: dict[str, str], exports: dict[str, bytes]):
        self.describes = describes
        self.exports = exports
        self.export_calls: list[str] = []

    def run(self, cmd, **kwargs):
        if cmd[1] == "--describe-imp":
            member = cmd[3]
            if member not in self.describes:
                return subprocess.CompletedProcess(cmd, 1, stdout="", stderr="no such member")
            return subprocess.CompletedProcess(cmd, 0, stdout=self.describes[member], stderr="")
        if cmd[1] == "--export-imp-frame":
            member = cmd[3]
            self.export_calls.append(member)
            if member not in self.exports:
                return subprocess.CompletedProcess(cmd, 1, stdout="", stderr="export failed")
            pathlib.Path(cmd[5]).write_bytes(self.exports[member])
            return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")
        raise AssertionError(f"unexpected viewer command: {cmd}")


class TransparentKeyTest(unittest.TestCase):
    def png(self, trns: bytes | None) -> "sprite_pack.IndexedPng":
        from png_index_patch import read_indexed_png
        data = indexed_png(4, 4, bytes(16), PLTE, trns)
        return read_indexed_png(data)

    def test_the_one_zero_alpha_index_is_the_key(self) -> None:
        trns = bytes([255, 255, 0, 255])
        self.assertEqual(sprite_pack.transparent_key(self.png(trns)), 2)

    def test_no_trns_is_not_a_key(self) -> None:
        self.assertIsNone(sprite_pack.transparent_key(self.png(None)))

    def test_two_zero_alpha_indices_refuse_a_single_key(self) -> None:
        trns = bytes([255, 0, 255, 0])
        self.assertIsNone(sprite_pack.transparent_key(self.png(trns)))


class PadPaletteTest(unittest.TestCase):
    def test_a_short_plte_is_padded_with_black(self) -> None:
        padded = sprite_pack.pad_palette(bytes([10, 20, 30, 40, 50, 60]))
        self.assertEqual(len(padded), 256)
        self.assertEqual(padded[0], (10, 20, 30))
        self.assertEqual(padded[1], (40, 50, 60))
        self.assertEqual(padded[2], (0, 0, 0))

    def test_a_full_plte_is_unchanged(self) -> None:
        self.assertEqual(sprite_pack.pad_palette(PLTE), [tuple(PLTE[i:i + 3]) for i in range(0, 768, 3)])


class CandidateMembersTest(unittest.TestCase):
    def test_case_variant_spellings_of_one_path_collapse_to_one_candidate(self) -> None:
        """`unit\\Tree.imp` and `unit\\TREE.imp` name the same archive entry -- one candidate, not
        two -- but which spelling survives is not asserted."""
        with tempfile.TemporaryDirectory() as tmp:
            listfile = pathlib.Path(tmp) / "list.txt"
            listfile.write_text("unit\\Tree.imp\nunit\\TREE.imp\nunit\\Goblin.imp\nunit\\not-a-sprite.pbm\n")
            grouped = sprite_pack.candidate_members(listfile)
            self.assertEqual(set(grouped), {"tree", "goblin"})
            self.assertEqual(len(grouped["tree"]), 1, "one spelling of unit\\tree.imp, not both")

    def test_different_folders_sharing_a_basename_are_kept_as_separate_candidates(self) -> None:
        """`aura\\agx06b.imp` and `imp\\agx06b.imp` are different paths that only share a
        basename -- both must be tried against a specific archive, not collapsed here."""
        with tempfile.TemporaryDirectory() as tmp:
            listfile = pathlib.Path(tmp) / "list.txt"
            listfile.write_text("aura\\agx06b.imp\nimp\\agx06b.imp\n")
            grouped = sprite_pack.candidate_members(listfile)
            self.assertEqual(grouped, {"agx06b": ["aura\\agx06b.imp", "imp\\agx06b.imp"]})


class FrameCountTest(unittest.TestCase):
    def test_counts_frame_records_not_sequences_or_facings(self) -> None:
        viewer = FakeViewer({"a.imp": describe(1), "b.imp": describe(3)}, {})
        real, sprite_pack.subprocess.run = sprite_pack.subprocess.run, viewer.run
        try:
            self.assertEqual(sprite_pack.frame_count(pathlib.Path("viewer"), pathlib.Path("a.mpq"),
                                                     "a.imp", pathlib.Path("l.txt")), 1)
            self.assertEqual(sprite_pack.frame_count(pathlib.Path("viewer"), pathlib.Path("a.mpq"),
                                                     "b.imp", pathlib.Path("l.txt")), 3)
        finally:
            sprite_pack.subprocess.run = real

    def test_a_member_missing_from_the_archive_is_none(self) -> None:
        viewer = FakeViewer({}, {})
        real, sprite_pack.subprocess.run = sprite_pack.subprocess.run, viewer.run
        try:
            self.assertIsNone(sprite_pack.frame_count(pathlib.Path("viewer"), pathlib.Path("a.mpq"),
                                                       "ghost.imp", pathlib.Path("l.txt")))
        finally:
            sprite_pack.subprocess.run = real


class BuildSpriteRecordsTest(unittest.TestCase):
    """The full selection pipeline, with the asset viewer, the palette-decode check and the
    magick-backed HD loader all faked, so only the selection rule itself is under test."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name)
        self.archive = self.root / "imp.mpq"
        self.archive.write_bytes(b"")
        self.listfile = self.root / "list.txt"
        self.renders = self.root / "renders"
        self.renders.mkdir()

        # `identify_size` and `load_rgba` (not `load_hd_rgba` itself) are what stand in for
        # ImageMagick, so `load_hd_rgba`'s own real logic -- refusing a render whose size is not
        # exactly (2w, 2h) -- runs for real in every test. A "render" is a text file holding its
        # own claimed "WxH"; `write_render` below writes it that size for real.
        for attr, value in (("viewer_decodes_bgr", lambda *a, **k: True),
                            ("identify_size", self.fake_identify_size),
                            ("load_rgba", self.fake_load_rgba)):
            self.addCleanup(setattr, sprite_pack, attr, getattr(sprite_pack, attr))
            setattr(sprite_pack, attr, value)
        self.addCleanup(setattr, sprite_pack, "WORK", sprite_pack.WORK)
        sprite_pack.WORK = self.root / "work"

    @staticmethod
    def fake_identify_size(render: pathlib.Path) -> tuple[int, int]:
        w, h = render.read_text().split("x")
        return int(w), int(h)

    @staticmethod
    def fake_load_rgba(render: pathlib.Path) -> bytes:
        w, h = BuildSpriteRecordsTest.fake_identify_size(render)
        return bytes(w * h * 4)

    def write_render(self, choice: str, record_name: str, w: int, h: int) -> None:
        """A stand-in render claiming to be `w`x`h`, for `fake_identify_size`/`fake_load_rgba`."""
        folder = self.renders / choice
        folder.mkdir(exist_ok=True)
        (folder / f"{record_name}.png").write_text(f"{w}x{h}")

    def install_viewer(self, describes: dict[str, str], exports: dict[str, bytes]) -> FakeViewer:
        viewer = FakeViewer(describes, exports)
        self.addCleanup(setattr, sprite_pack.subprocess, "run", sprite_pack.subprocess.run)
        sprite_pack.subprocess.run = viewer.run
        return viewer

    def masked_png(self, w: int, h: int, key: int = 5, opaque: int = 3) -> bytes:
        indices = masked_sprite(w, h, key, opaque=opaque)
        trns = bytearray(b"\xff" * 256)
        trns[key] = 0
        return indexed_png(w, h, indices, PLTE, bytes(trns))

    def eligible_sprite_png(self, key: int = 5) -> bytes:
        return self.masked_png(20, 6, key)

    def write_listfile(self, *members: str) -> None:
        self.listfile.write_text("\n".join(f"unit\\{m}" for m in members) + "\n")

    def run_build(self, choices: dict) -> tuple[list, int, list[str]]:
        return sprite_pack.build_sprite_records(self.archive, pathlib.Path("viewer"), self.listfile,
                                                self.renders, choices)

    def test_a_static_eligible_sprite_with_a_pick_and_a_render_is_packed(self) -> None:
        self.write_listfile("tree.imp")
        self.install_viewer({"unit\\tree.imp": describe(1)}, {"unit\\tree.imp": self.eligible_sprite_png()})
        self.write_render("anime2x", "sprite__tree", 40, 12)
        records, considered, skipped = self.run_build({"sprite__tree": "anime2x"})
        self.assertEqual(considered, 1)
        self.assertEqual(skipped, [])
        [(entry, zidx, zhd)] = records
        [(name, small, large, flags, key, _)] = pack.read(
            self._packed_bytes(records))
        self.assertEqual(name, "sprite__tree")
        self.assertEqual(flags, pack.FLAG_MASKED)
        self.assertEqual(key, 5)
        self.assertEqual((small[0], small[1]), (20, 6))
        self.assertEqual(large[:2], (40, 12))

    def _packed_bytes(self, records) -> bytes:
        out = self.root / "out.pack"
        pack.write_records(out, records)
        return out.read_bytes()

    def test_a_multi_frame_sprite_is_not_static(self) -> None:
        """Not a static record, and not reported as left out either: animated sprites are
        anim_frames.py's (sprite_pack.py --animated)."""
        self.write_listfile("goblin.imp")
        self.install_viewer({"unit\\goblin.imp": describe(4)}, {})
        records, considered, skipped = self.run_build({})
        self.assertEqual(records, [])
        self.assertEqual(considered, 1)
        self.assertEqual(skipped, [])

    def test_a_member_not_in_this_archive_is_reported_not_silently_dropped(self) -> None:
        self.write_listfile("ghost.imp")
        self.install_viewer({}, {})
        records, considered, skipped = self.run_build({})
        self.assertEqual(records, [])
        self.assertEqual(skipped, ["ghost: not in this archive"])

    def test_a_sprite_too_small_for_the_matcher_is_skipped(self) -> None:
        self.write_listfile("dot.imp")
        indices = masked_sprite(10, 6, key=5)               # narrower than MASKED_MIN_WIDTH
        trns = bytearray(b"\xff" * 256); trns[5] = 0
        png = indexed_png(10, 6, indices, PLTE, bytes(trns))
        self.install_viewer({"unit\\dot.imp": describe(1)}, {"unit\\dot.imp": png})
        records, considered, skipped = self.run_build({"sprite__dot": "anime2x"})
        self.assertEqual(records, [])
        self.assertEqual(len(skipped), 1)
        self.assertIn("dot:", skipped[0])
        self.assertIn("matcher", skipped[0])

    def test_a_sprite_with_no_upscale_pick_is_skipped(self) -> None:
        self.write_listfile("tree.imp")
        self.install_viewer({"unit\\tree.imp": describe(1)}, {"unit\\tree.imp": self.eligible_sprite_png()})
        records, considered, skipped = self.run_build({})       # no pick at all
        self.assertEqual(records, [])
        self.assertEqual(skipped, ["tree: no usable upscale pick (None)"])

    def test_a_pick_of_approved_does_not_apply_to_sprites(self) -> None:
        self.write_listfile("tree.imp")
        self.install_viewer({"unit\\tree.imp": describe(1)}, {"unit\\tree.imp": self.eligible_sprite_png()})
        records, considered, skipped = self.run_build({"sprite__tree": "approved"})
        self.assertEqual(records, [])
        self.assertEqual(skipped, ["tree: no usable upscale pick ('approved')"])

    def test_a_missing_render_file_is_skipped(self) -> None:
        self.write_listfile("tree.imp")
        self.install_viewer({"unit\\tree.imp": describe(1)}, {"unit\\tree.imp": self.eligible_sprite_png()})
        records, considered, skipped = self.run_build({"sprite__tree": "anime2x"})   # anime2x/ never created
        self.assertEqual(records, [])
        self.assertEqual(len(skipped), 1)
        self.assertIn("render is missing", skipped[0])

    def test_a_frame_with_no_single_transparent_index_is_skipped(self) -> None:
        self.write_listfile("solid.imp")
        indices = masked_sprite(20, 6, key=5)
        png = indexed_png(20, 6, indices, PLTE, None)          # no tRNS at all
        self.install_viewer({"unit\\solid.imp": describe(1)}, {"unit\\solid.imp": png})
        records, considered, skipped = self.run_build({"sprite__solid": "anime2x"})
        self.assertEqual(records, [])
        self.assertEqual(skipped, ["solid: not exactly one fully-transparent palette index"])

    def test_a_record_name_over_the_dll_limit_is_skipped_before_any_export(self) -> None:
        long_name = "a" * 32                                # "sprite__" + 32 = 40, one over 39
        self.write_listfile(f"{long_name}.imp")
        viewer = self.install_viewer({}, {})
        records, considered, skipped = self.run_build({})
        self.assertEqual(records, [])
        self.assertEqual(len(skipped), 1)
        self.assertIn("longer than the DLL's 39-character limit", skipped[0])
        self.assertEqual(viewer.export_calls, [], "an oversized name must not even be exported")

    def test_a_frame_export_failure_is_skipped(self) -> None:
        self.write_listfile("broken.imp")
        self.install_viewer({"unit\\broken.imp": describe(1)}, {})   # describe ok, export fails
        records, considered, skipped = self.run_build({"sprite__broken": "anime2x"})
        self.assertEqual(records, [])
        self.assertEqual(len(skipped), 1)
        self.assertIn("broken: could not export frame 0", skipped[0])

    def test_an_export_already_on_disk_is_reused_not_re_exported(self) -> None:
        self.write_listfile("tree.imp")
        sprite_pack.WORK.mkdir(parents=True, exist_ok=True)
        fingerprint = sprite_pack.archive_fingerprint(self.archive)
        member_hash = sprite_pack.member_fingerprint("unit\\tree.imp")
        (sprite_pack.WORK / f"{fingerprint}__{member_hash}__tree.png").write_bytes(self.eligible_sprite_png())
        viewer = self.install_viewer({"unit\\tree.imp": describe(1)}, {})   # no export entry needed
        self.write_render("anime2x", "sprite__tree", 40, 12)
        records, considered, skipped = self.run_build({"sprite__tree": "anime2x"})
        self.assertEqual(len(records), 1)
        self.assertEqual(viewer.export_calls, [], "a cached frame export must not be re-exported")

    # --- the upscale side must fit the DLL's own limit, not just the eligibility rule -----------

    def test_a_sprite_upscale_at_exactly_the_max_side_is_packed(self) -> None:
        """640 wide, 16 tall: eligible (w*h well under the pixel cap), and 2*640 = 1280 is exactly
        MAX_UPSCALE_SIDE -- the DLL's own boundary, not the eligibility rule's."""
        self.write_listfile("wide.imp")
        png = self.masked_png(640, 16)
        self.install_viewer({"unit\\wide.imp": describe(1)}, {"unit\\wide.imp": png})
        self.write_render("anime2x", "sprite__wide", 1280, 32)
        records, considered, skipped = self.run_build({"sprite__wide": "anime2x"})
        self.assertEqual(skipped, [])
        self.assertEqual(len(records), 1)

    def test_a_sprite_upscale_one_pixel_over_the_max_side_is_skipped_not_packed(self) -> None:
        """641 wide passes the same eligibility rule as 640 does (w*h is still tiny), but its
        upscale is 1282 wide -- over MAX_UPSCALE_SIDE -- which the DLL refuses the WHOLE pack for,
        not just this one record, if it is ever allowed through."""
        self.write_listfile("toowide.imp")
        png = self.masked_png(641, 16)
        self.install_viewer({"unit\\toowide.imp": describe(1)}, {"unit\\toowide.imp": png})
        self.write_render("anime2x", "sprite__toowide", 1282, 32)
        records, considered, skipped = self.run_build({"sprite__toowide": "anime2x"})
        self.assertEqual(records, [])
        self.assertEqual(len(skipped), 1)
        self.assertIn("exceeds", skipped[0])

    def test_a_render_of_the_wrong_size_is_refused_not_resized(self) -> None:
        """A render that is not exactly (2w, 2h) was made from a different original -- a stale
        render, or the classic case of two different members sharing a record name -- and must be
        refused, not silently resized into shipping the wrong picture."""
        self.write_listfile("tree.imp")
        self.install_viewer({"unit\\tree.imp": describe(1)}, {"unit\\tree.imp": self.eligible_sprite_png()})
        self.write_render("anime2x", "sprite__tree", 41, 13)          # not exactly (40, 12)
        records, considered, skipped = self.run_build({"sprite__tree": "anime2x"})
        self.assertEqual(records, [])
        self.assertEqual(len(skipped), 1)
        self.assertIn("render is 41x13, expected 40x12", skipped[0])
        self.assertIn("different original", skipped[0])

    # --- the frame-export cache must not mix pixels from two different archives ------------------

    def test_two_different_archives_do_not_share_a_cached_frame_export(self) -> None:
        self.write_listfile("tree.imp")
        archive_a = self.root / "a.mpq"; archive_a.write_bytes(b"archive-a-bytes")
        archive_b = self.root / "b.mpq"; archive_b.write_bytes(b"a-different-archive-entirely")
        self.write_render("anime2x", "sprite__tree", 40, 12)

        self.install_viewer({"unit\\tree.imp": describe(1)},
                            {"unit\\tree.imp": self.masked_png(20, 6, opaque=3)})
        records_a, _, skipped_a = sprite_pack.build_sprite_records(
            archive_a, pathlib.Path("viewer"), self.listfile, self.renders, {"sprite__tree": "anime2x"})
        self.assertEqual(skipped_a, [])

        self.install_viewer({"unit\\tree.imp": describe(1)},
                            {"unit\\tree.imp": self.masked_png(20, 6, opaque=9)})
        records_b, _, skipped_b = sprite_pack.build_sprite_records(
            archive_b, pathlib.Path("viewer"), self.listfile, self.renders, {"sprite__tree": "anime2x"})
        self.assertEqual(skipped_b, [])

        out_a, out_b = self.root / "a.pack", self.root / "b.pack"
        pack.write_records(out_a, records_a)
        pack.write_records(out_b, records_b)
        [(_, small_a, *_)] = pack.read(out_a.read_bytes())
        [(_, small_b, *_)] = pack.read(out_b.read_bytes())
        self.assertNotEqual(bytes(small_a[3]), bytes(small_b[3]),
                            "each archive's own pixels, not whichever was cached first")

    # --- membership must be resolved before names are deduplicated by basename -------------------

    def test_a_present_spelling_is_found_even_when_a_different_spelling_of_the_name_is_not(self) -> None:
        """`aura\\agx06b.imp` and `imp\\agx06b.imp` both come from the recovered listfile; this
        archive only has the second. Deduplicating by basename before checking membership can pick
        the absent spelling and report the whole name as missing."""
        self.listfile.write_text("aura\\agx06b.imp\nimp\\agx06b.imp\n")
        self.install_viewer({"imp\\agx06b.imp": describe(1)},
                            {"imp\\agx06b.imp": self.eligible_sprite_png()})
        self.write_render("anime2x", "sprite__agx06b", 40, 12)
        records, considered, skipped = self.run_build({"sprite__agx06b": "anime2x"})
        self.assertEqual(skipped, [])
        self.assertEqual(len(records), 1)

    def test_two_different_present_members_sharing_a_name_are_reported_as_a_collision(self) -> None:
        """If BOTH spellings are present in this archive, they may be two unrelated sprites that
        happen to share a basename -- picking one silently would be a guess, not a resolution."""
        self.listfile.write_text("aura\\agx06b.imp\nimp\\agx06b.imp\n")
        self.install_viewer({"aura\\agx06b.imp": describe(1), "imp\\agx06b.imp": describe(1)}, {})
        records, considered, skipped = self.run_build({"sprite__agx06b": "anime2x"})
        self.assertEqual(records, [])
        self.assertEqual(len(skipped), 1)
        self.assertIn("agx06b:", skipped[0])
        self.assertIn("ambiguous", skipped[0])

    def test_the_frame_cache_is_also_keyed_by_which_member_was_resolved(self) -> None:
        """One archive, two different members sharing a record name: building once with a listfile
        naming only the aura\\ spelling and once naming only the imp\\ spelling must not let the
        second build reuse the first build's cached export just because the archive and the
        resulting record NAME are the same -- the member path actually resolved must be part of
        the cache key too."""
        self.write_render("anime2x", "sprite__agx06b", 40, 12)

        self.listfile.write_text("aura\\agx06b.imp\n")
        self.install_viewer({"aura\\agx06b.imp": describe(1)},
                            {"aura\\agx06b.imp": self.masked_png(20, 6, opaque=3)})
        records_aura, _, skipped_aura = self.run_build({"sprite__agx06b": "anime2x"})
        self.assertEqual(skipped_aura, [])

        self.listfile.write_text("imp\\agx06b.imp\n")
        self.install_viewer({"imp\\agx06b.imp": describe(1)},
                            {"imp\\agx06b.imp": self.masked_png(20, 6, opaque=9)})
        records_imp, _, skipped_imp = self.run_build({"sprite__agx06b": "anime2x"})
        self.assertEqual(skipped_imp, [])

        out_aura, out_imp = self.root / "aura.pack", self.root / "imp.pack"
        pack.write_records(out_aura, records_aura)
        pack.write_records(out_imp, records_imp)
        [(_, small_aura, *_)] = pack.read(out_aura.read_bytes())
        [(_, small_imp, *_)] = pack.read(out_imp.read_bytes())
        self.assertNotEqual(bytes(small_aura[3]), bytes(small_imp[3]),
                            "the imp\\ build must not reuse the aura\\ build's cached export")


if __name__ == "__main__":
    unittest.main()
