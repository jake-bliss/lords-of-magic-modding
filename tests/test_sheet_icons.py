"""sheet_icons: where the icons on a UI sheet are, which become records, and what each record holds.
Sheets are written by hand as LBMs; the upscaler is not run -- a render is a PNG made here."""

from __future__ import annotations

import pathlib
import struct
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "tools" / "portrait-upscale"))
sys.path.insert(0, str(ROOT / "tools" / "hd-review"))

import hd_portrait_pack as pack  # noqa: E402
import lbm_png  # noqa: E402
import sheet_icons as si  # noqa: E402

KEY = 0
PAL = [(0, 255, 0)] + [(i, (i * 3) % 256, 255 - i) for i in range(1, 256)]


def blank(w: int, h: int) -> bytearray:
    return bytearray([KEY] * (w * h))


def paint(idx: bytearray, w: int, x: int, y: int, cw: int, ch: int, base: int) -> None:
    """A cw x ch icon at (x, y): colours counting across each row, so every row has a probe."""
    for j in range(ch):
        for i in range(cw):
            idx[(y + j) * w + x + i] = 10 + (base + i + j) % 200


class ScriptCutsTest(unittest.TestCase):
    def test_a_page_names_its_sheet_and_its_cuts_follow(self) -> None:
        text = '/intspr1_page"lbm/intspr1.lbm"lbm def \r intspr1_page 341 0 70 67 doodad \r' \
               '{intspr1_page 0 91 24 25 doodad}bind def'
        self.assertEqual(dict(si.script_cuts([text])), {"intspr1": {(341, 0, 70, 67), (0, 91, 24, 25)}})

    def test_cuts_and_the_page_may_be_in_different_scripts(self) -> None:
        cuts = si.script_cuts(['/indicator_page"LBM\\INDICATE.LBM"lbm def', "indicator_page 1 2 30 40 doodad"])
        self.assertEqual(dict(cuts), {"indicate": {(1, 2, 30, 40)}})

    def test_a_page_bound_to_two_files_is_ignored(self) -> None:
        text = '/backdrop_page"lbm/loading.lbm"lbm def /backdrop_page"lbm/start01.lbm"lbm def ' \
               'backdrop_page 0 0 64 64 doodad'
        self.assertEqual(dict(si.script_cuts([text])), {})

    def test_a_page_never_bound_to_a_file_is_ignored(self) -> None:
        self.assertEqual(dict(si.script_cuts(["black_page 0 0 640 480 doodad"])), {})

    def test_any_variable_bound_to_an_lbm_is_a_page(self) -> None:
        """GS5R3 binds staticon as `unitinfo_staticon` and eoturn as `eoturnbuttonpage`."""
        text = '/unitinfo_staticon"LBM/STATICON.lbm"lbm def unitinfo_staticon 424 163 31 18 doodad ' \
               '/eoturnbuttonpage "lbm/eoturn.lbm" lbm def eoturnbuttonpage 89 243 12 27 doodad'
        self.assertEqual(dict(si.script_cuts([text])),
                         {"staticon": {(424, 163, 31, 18)}, "eoturn": {(89, 243, 12, 27)}})

    def test_a_commented_out_cut_is_not_a_cut(self) -> None:
        text = '/intspr1_page"lbm/intspr1.lbm"lbm def\r; /dead intspr1_page 530 151 40 36 doodad def\r' \
               'intspr1_page 0 91 24 25 doodad'
        self.assertEqual(dict(si.script_cuts([text])), {"intspr1": {(0, 91, 24, 25)}})


class ShapesTest(unittest.TestCase):
    def test_separate_shapes_and_diagonal_neighbours(self) -> None:
        w, h = 30, 12
        idx = blank(w, h)
        paint(idx, w, 1, 1, 5, 3, 0)
        idx[4 * w + 6] = 20                               # touches (5, 3) diagonally: same shape
        paint(idx, w, 20, 5, 8, 6, 0)
        self.assertEqual(sorted(si.shapes(w, h, bytes(idx), KEY)), [(1, 1, 6, 4), (20, 5, 8, 6)])


class IconsOfTest(unittest.TestCase):
    def test_script_cuts_first_then_shapes_no_cut_touches(self) -> None:
        w, h = 100, 40
        idx = blank(w, h)
        paint(idx, w, 0, 0, 40, 20, 0)                    # two emblems touching: one shape ...
        paint(idx, w, 60, 10, 20, 12, 0)                  # ... and a free icon
        cuts = {(0, 0, 20, 20), (20, 0, 20, 20), (90, 30, 20, 20)}   # the last runs off the sheet
        icons = si.icons_of("s", w, h, bytes(idx), KEY, cuts)
        self.assertEqual([(i.x, i.y, i.w, i.h, i.source) for i in icons],
                         [(0, 0, 20, 20, "script"), (20, 0, 20, 20, "script"), (60, 10, 20, 12, "shape")])

    def test_a_whole_page_cut_or_a_placeholder_hides_nothing(self) -> None:
        """eoturn's scripts cut `0 0 373 309` (the page itself) and `200 0 1 1`."""
        w, h = 100, 40
        idx = blank(w, h)
        paint(idx, w, 60, 10, 20, 12, 0)
        paint(idx, w, 2, 2, 20, 12, 0)
        icons = si.icons_of("s", w, h, bytes(idx), KEY, {(0, 0, 99, 39), (60, 10, 1, 1), (2, 2, 20, 12)})
        self.assertEqual([(i.x, i.y, i.w, i.h, i.source) for i in icons],
                         [(2, 2, 20, 12, "script"), (60, 10, 20, 12, "shape")])

    def test_a_stack_of_slices_is_not_an_icon(self) -> None:
        """staticon5r3a's bars are cut 30x3 at a time; the stack is never drawn whole."""
        w, h = 60, 30
        idx = blank(w, h)
        paint(idx, w, 10, 5, 30, 9, 0)
        icons = si.icons_of("s", w, h, bytes(idx), KEY, {(10, 5, 30, 3), (10, 8, 30, 3), (10, 11, 30, 3)})
        self.assertEqual(icons, [])

    def test_a_cut_inside_a_larger_one_from_the_same_corner_is_dropped(self) -> None:
        w, h = 60, 30
        idx = blank(w, h)
        paint(idx, w, 5, 5, 27, 18, 0)
        icons = si.icons_of("s", w, h, bytes(idx), KEY, {(5, 5, 26, 17), (5, 5, 27, 18)})
        self.assertEqual([(i.w, i.h) for i in icons], [(27, 18)])

    def test_cuts_neither_of_which_contains_the_other_are_both_kept(self) -> None:
        w, h = 60, 30
        idx = blank(w, h)
        paint(idx, w, 5, 5, 27, 18, 0)
        icons = si.icons_of("s", w, h, bytes(idx), KEY, {(5, 5, 26, 18), (5, 5, 27, 17)})
        self.assertEqual(sorted((i.w, i.h) for i in icons), [(26, 18), (27, 17)])


class PlanAndRecordsTest(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.lbm = self.root / "LBM"
        self.lbm.mkdir()
        self.addCleanup(setattr, si, "SHEETS", si.SHEETS)
        si.SHEETS = ("wide", "copy")

    def write_sheet(self, name: str, w: int, h: int, idx, pal=PAL) -> None:
        bmhd = struct.pack(">HHhhBBBBHBBhh", w, h, 0, 0, 8, 0, 1, 0, 0, 1, 1, w, h)
        lbm_png.encode(self.lbm / f"{name.upper()}.LBM", w, h, bytes(idx), pal,
                       [(b"BMHD", bmhd), (b"CMAP", b""), (b"BODY", b"")])

    def test_small_shapes_drop_out_and_a_repeat_across_sheets_is_packed_once(self) -> None:
        w, h = 80, 30
        idx = blank(w, h)
        paint(idx, w, 3, 2, 20, 10, 0)                    # an icon
        paint(idx, w, 40, 2, 5, 3, 0)                     # a label letter: too small
        paint(idx, w, 50, 15, 24, 12, 7)                  # another icon
        self.write_sheet("wide", w, h, idx)
        copy = blank(40, 20)
        paint(copy, 40, 10, 5, 20, 10, 0)                 # the first icon again, elsewhere
        self.write_sheet("copy", 40, 20, copy)
        skipped: list[str] = []
        planned = si.plan(self.lbm, {}, skipped)
        got = {sheet: [(i.x, i.y, i.w, i.h) for i in icons] for sheet, *_, icons in planned}
        self.assertEqual(got, {"wide": [(3, 2, 20, 10), (50, 15, 24, 12)], "copy": [(10, 5, 20, 10)]})
        self.assertEqual(skipped, [])
        packed = self.pack(planned, {"wide": self.render(80, 30), "copy": self.render(40, 20)})
        self.assertEqual([r[0] for r in packed], ["icon__wide@3,2,20x10", "icon__wide@50,15,24x12"])

    def test_a_repeat_survives_when_its_first_sheet_is_not_packed(self) -> None:
        """Dedupe runs over what is packed: a first sheet with no render must not take the shared
        icon with it (Claude review, 2026-09-23)."""
        for name, w, h, x, y in (("wide", 80, 30, 3, 2), ("copy", 40, 20, 10, 5)):
            idx = blank(w, h)
            paint(idx, w, x, y, 20, 10, 0)
            self.write_sheet(name, w, h, idx)
        planned = si.plan(self.lbm, {}, [])
        packed = self.pack(planned, {"copy": self.render(40, 20)})
        self.assertEqual([r[0] for r in packed], ["icon__copy@10,5,20x10"])

    def test_the_same_indices_under_another_palette_are_another_icon(self) -> None:
        other = [PAL[0]] + [(255 - r, g, b) for r, g, b in PAL[1:]]
        for name, pal in (("wide", PAL), ("copy", other)):
            idx = blank(40, 20)
            paint(idx, 40, 10, 5, 20, 10, 0)
            self.write_sheet(name, 40, 20, idx, pal)
        planned = si.plan(self.lbm, {}, [])
        packed = self.pack(planned, {"wide": self.render(40, 20), "copy": self.render(40, 20)})
        self.assertEqual(len(packed), 2)

    def test_a_sheet_whose_index_0_is_not_the_chroma_key_is_refused(self) -> None:
        """`label`'s most common index is a real colour; keying on it would punch holes."""
        si.SHEETS = ("wide",)
        idx = blank(40, 20)
        paint(idx, 40, 10, 5, 20, 10, 0)
        self.write_sheet("wide", 40, 20, idx, [(27, 43, 43)] + PAL[1:])
        skipped: list[str] = []
        self.assertEqual(si.plan(self.lbm, {}, skipped), [])
        self.assertEqual(skipped, ["wide: index 0 is (27, 43, 43), not the chroma key"])

    def test_the_key_is_index_0_even_where_a_real_colour_is_commoner(self) -> None:
        """`label`: green index 0 only in a margin, a teal fill everywhere else (Claude review).
        Keyed on the commonest index, the fill would be see-through."""
        si.SHEETS = ("wide",)
        w, h = 40, 20
        idx = bytearray([222] * (w * h))
        for x in range(w):
            idx[x] = KEY
        paint(idx, w, 10, 5, 20, 3, 0)
        self.write_sheet("wide", w, h, idx)
        [(sheet, _, _, _, _, key, icons)] = si.plan(self.lbm, {}, [])
        self.assertEqual(key, KEY)
        self.assertEqual([(i.x, i.y, i.w, i.h) for i in icons], [(0, 1, 40, 19)])

    def test_the_upscaler_sees_key_and_index_1_as_grey_and_colours_as_themselves(self) -> None:
        idx = bytes([KEY, pack.SHADOW_INDEX, 77, KEY])
        out = self.root / "prep.png"
        si.prepared_png(out, 2, 2, idx, PAL, KEY)
        pixels = lbm_png.read_png_rgb(out)
        flat = [tuple(p) for row in pixels[2] for p in row] if isinstance(pixels, tuple) else None
        self.assertEqual(flat, [si.PREP_BACKGROUND, si.PREP_BACKGROUND, PAL[77], si.PREP_BACKGROUND])

    def test_the_render_cache_key_follows_everything_the_upscaler_sees(self) -> None:
        idx = bytes(range(10, 210)) * 4
        base = si.render_key(40, 20, idx, PAL, KEY)
        self.assertNotEqual(base, si.render_key(20, 40, idx, PAL, KEY), "same bytes, other size")
        self.assertNotEqual(base, si.render_key(40, 20, idx, [PAL[0], (1, 2, 3)] + PAL[2:], KEY))
        self.assertNotEqual(base, si.render_key(40, 20, idx, PAL, 7))
        self.assertEqual(base, si.render_key(40, 20, idx, PAL, KEY))

    def render(self, w: int, h: int) -> pathlib.Path:
        out = self.root / f"render{w}x{h}.png"
        lbm_png.write_png(out, w * 2, h * 2, [[(x % 256, y % 256, 9) for x in range(w * 2)] for y in range(h * 2)])
        return out

    def pack(self, planned, renders):
        skipped: list[str] = []
        out = self.root / "icons.pack"
        pack.write_records(out, si.records(planned, renders, skipped))
        self.assertEqual(skipped, [])
        return pack.read(out.read_bytes())

    def test_a_missing_sheet_is_reported(self) -> None:
        skipped: list[str] = []
        self.assertEqual(si.plan(self.lbm, {}, skipped), [])
        self.assertEqual(skipped, ["wide: not in this install", "copy: not in this install"])

    def test_a_record_is_the_icon_its_key_and_its_2x_crop_of_the_sheet_render(self) -> None:
        """Off-diagonal position on a non-square sheet, so a swapped x and y cannot pass."""
        w, h = 64, 24
        idx = blank(w, h)
        paint(idx, w, 37, 9, 20, 8, 3)
        self.write_sheet("wide", w, h, idx)
        si.SHEETS = ("wide",)
        planned = si.plan(self.lbm, {}, [])
        render = self.root / "wide.png"
        colour = lambda x, y: (x % 256, y % 256, (x * y) % 256)   # every 2x pixel distinct
        lbm_png.write_png(render, w * 2, h * 2, [[colour(x, y) for x in range(w * 2)] for y in range(h * 2)])
        skipped: list[str] = []
        out = self.root / "icons.pack"
        pack.write_records(out, si.records(planned, {"wide": render}, skipped))
        [(name, (iw, ih, pal, indices), (hw, hh, hd), flags, key, group)] = pack.read(out.read_bytes())
        self.assertEqual(skipped, [])
        self.assertEqual(name, "icon__wide@37,9,20x8")
        self.assertEqual((iw, ih, hw, hh, flags, key, group), (20, 8, 40, 16, pack.FLAG_MASKED, KEY, 0))
        self.assertEqual(indices, si.crop(bytes(idx), w, si.Icon("wide", 37, 9, 20, 8, "shape")))
        self.assertEqual(pal[:4], PAL[:4])
        expect = b"".join(bytes(colour(74 + i, 18 + j)) + b"\xff" for j in range(16) for i in range(40))
        self.assertEqual(hd, expect)

    def test_a_render_of_the_wrong_size_is_refused(self) -> None:
        w, h = 64, 24
        idx = blank(w, h)
        paint(idx, w, 37, 9, 20, 8, 3)
        self.write_sheet("wide", w, h, idx)
        si.SHEETS = ("wide",)
        planned = si.plan(self.lbm, {}, [])
        render = self.root / "wide.png"
        lbm_png.write_png(render, w, h, [[(0, 0, 0)] * w for _ in range(h)])
        skipped: list[str] = []
        self.assertEqual(list(si.records(planned, {"wide": render}, skipped)), [])
        self.assertEqual(skipped, ["wide: render is not 128x48"])

    def test_a_render_with_width_and_height_swapped_is_refused(self) -> None:
        """Same byte count as the right render, wrong geometry (Codex review, 2026-09-23)."""
        w, h = 64, 24
        idx = blank(w, h)
        paint(idx, w, 37, 9, 20, 8, 3)
        self.write_sheet("wide", w, h, idx)
        si.SHEETS = ("wide",)
        planned = si.plan(self.lbm, {}, [])
        render = self.root / "wide.png"
        lbm_png.write_png(render, h * 2, w * 2, [[(0, 0, 0)] * (h * 2) for _ in range(w * 2)])
        skipped: list[str] = []
        self.assertEqual(list(si.records(planned, {"wide": render}, skipped)), [])
        self.assertEqual(skipped, ["wide: render is not 128x48"])


if __name__ == "__main__":
    unittest.main()
