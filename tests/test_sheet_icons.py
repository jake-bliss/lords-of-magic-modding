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

KEY = 250
PAL = [(i, (i * 3) % 256, 255 - i) for i in range(256)]


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

    def test_a_cut_inside_a_larger_one_from_the_same_corner_is_dropped(self) -> None:
        w, h = 60, 30
        idx = blank(w, h)
        paint(idx, w, 5, 5, 27, 18, 0)
        icons = si.icons_of("s", w, h, bytes(idx), KEY, {(5, 5, 26, 17), (5, 5, 27, 18)})
        self.assertEqual([(i.w, i.h) for i in icons], [(27, 18)])


class PlanAndRecordsTest(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.lbm = self.root / "LBM"
        self.lbm.mkdir()
        self.addCleanup(setattr, si, "SHEETS", si.SHEETS)
        si.SHEETS = ("wide", "copy")

    def write_sheet(self, name: str, w: int, h: int, idx) -> None:
        bmhd = struct.pack(">HHhhBBBBHBBhh", w, h, 0, 0, 8, 0, 1, 0, 0, 1, 1, w, h)
        lbm_png.encode(self.lbm / f"{name.upper()}.LBM", w, h, bytes(idx), PAL,
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
        self.assertEqual(got, {"wide": [(3, 2, 20, 10), (50, 15, 24, 12)], "copy": []})
        self.assertEqual(skipped, [])

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


if __name__ == "__main__":
    unittest.main()
