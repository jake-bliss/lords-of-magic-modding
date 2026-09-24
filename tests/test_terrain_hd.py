"""terrain_hd: tiles are upscaled alone, and no 2x pixel takes an index from another tile.

The model is not run. A stand-in upscaler (nearest-neighbour 2x through ImageMagick) takes its place
in the end-to-end test, so the result is exactly predictable: every 2x block must repeat its source
index. The bleed tests feed hand-built RGB instead, because the defect being guarded is a colour
arriving from the NEIGHBOURING tile, which only a sheet-level upscale produces."""

from __future__ import annotations

import json
import pathlib
import shutil
import stat
import struct
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "tools" / "portrait-upscale"))

import lbm_png  # noqa: E402
import terrain_hd as th  # noqa: E402

PAL = [(i, (i * 7) % 256, 255 - i) for i in range(256)]
PAL[1], PAL[2] = (255, 0, 0), (0, 0, 255)


def write_lbm(path: pathlib.Path, w: int, h: int, idx) -> None:
    bmhd = struct.pack(">HHhhBBBBHBBhh", w, h, 0, 0, 8, 0, 1, 0, 0, 1, 1, w, h)
    lbm_png.encode(path, w, h, bytes(idx), PAL, [(b"BMHD", bmhd), (b"CMAP", b""), (b"BODY", b"")])


def til(lbm: str, size: int) -> bytes:
    # Bare CR line endings, as the game writes them.
    return f"LBM={lbm}\rTILES= 2, 1\rTILESIZE= {size}, {size}\r;comment\rTILE= 0, 19, 19\r".encode()


def two_tiles(t: int) -> bytes:
    """Tile 0 all index 1 (red), tile 1 all index 2 (blue), side by side: 2t x t."""
    return bytes([1] * t + [2] * t) * t


class TilFiles(unittest.TestCase):
    def test_tilesize_doubles_and_nothing_else_moves(self) -> None:
        src = til("a.lbm", 32)
        out = th.doubled_til(src)
        self.assertEqual(out, src.replace(b"TILESIZE= 32, 32", b"TILESIZE= 64, 64"))
        self.assertEqual(out.count(b"\r"), src.count(b"\r"))

    def test_zero_or_two_tilesize_lines_refuse(self) -> None:
        with self.assertRaises(SystemExit):
            th.doubled_til(b"LBM=a.lbm\r")
        with self.assertRaises(SystemExit):
            th.doubled_til(b"TILESIZE= 32, 32\rTILESIZE= 32, 32\r")

    def test_two_tilesets_disagreeing_on_one_atlas_refuse(self) -> None:
        d = pathlib.Path(tempfile.mkdtemp())
        (d / "a.til").write_bytes(til("x.lbm", 32))
        (d / "b.til").write_bytes(til("X.LBM", 16))
        with self.assertRaises(SystemExit):
            th.tile_sizes(d)

    def test_non_square_tiles_refuse(self) -> None:
        d = pathlib.Path(tempfile.mkdtemp())
        (d / "a.til").write_bytes(b"LBM=x.lbm\rTILESIZE= 32, 16\r")
        with self.assertRaises(SystemExit):
            th.read_til(d / "a.til")


class Padding(unittest.TestCase):
    def test_padding_repeats_the_tiles_own_edge_not_the_neighbour(self) -> None:
        t, w = 4, 8
        rows = th.padded_tile(two_tiles(t), w, PAL, 0, 0, t, 3)
        self.assertEqual(len(rows), t + 6)
        self.assertTrue(all(len(r) == t + 6 for r in rows))
        # Tile 0 is red throughout; its right-hand padding sits over tile 1 (blue) on the sheet.
        self.assertTrue(all(px == PAL[1] for r in rows for px in r))

    def test_padding_clamps_each_axis_separately(self) -> None:
        t, w = 3, 3
        idx = bytes(range(10, 19))  # a 3x3 tile with distinct indices
        rows = th.padded_tile(idx, w, PAL, 0, 0, t, 2)
        self.assertEqual(rows[0][0], PAL[10])     # corner padding = corner pixel
        self.assertEqual(rows[0][6], PAL[12])
        self.assertEqual(rows[6][0], PAL[16])
        self.assertEqual(rows[2][4], PAL[12])     # right padding of the top row
        self.assertEqual(rows[3][2], PAL[13])     # the tile itself starts at (2, 2)

    def test_a_side_with_a_neighbour_continues_into_it(self) -> None:
        # Sheet: cell 0 (indices 10..), cell 1 (index 2 throughout). Cell 0's east side is cell 1.
        t, w = 2, 4
        idx = bytes([10, 11, 2, 2, 12, 13, 2, 2])
        rows = th.padded_tile(idx, w, PAL, 0, 0, t, 1, {"e": 1, "n": None, "s": None, "w": None})
        self.assertEqual(rows[1][3], PAL[2])       # east padding comes from cell 1
        self.assertEqual(rows[1][0], PAL[10])      # west repeats cell 0's own edge
        self.assertEqual(rows[0][0], PAL[10])      # corners repeat cell 0's own corner

    def test_neighbour_pixels_are_the_ones_that_would_continue_past_the_edge(self) -> None:
        # Cell 1 is a 2x2 with distinct indices; as cell 0's NORTH neighbour, the row above cell 0
        # is cell 1's BOTTOM row.
        t, w = 2, 4
        idx = bytes([5, 5, 20, 21, 5, 5, 22, 23])
        rows = th.padded_tile(idx, w, PAL, 0, 0, t, 1, {"n": 1})
        self.assertEqual(rows[0][1:3], [PAL[22], PAL[23]])


TIL = (b"LBM=x.lbm\rTILESIZE= 32, 32\r"
       b"TILE=      0, 6,  6,  6,  6,  6,  6,  6,  6,  6,   0\r"
       b"TILE=      1, 6,  6,  6,  1,  6,  6,  6,  6,  6,   9\r"
       b"TILE=      2, 1,  1,  1,  1,  1,  1,  1,  1,  1,   0\r"
       b"TILE=      3, 4,  ~6|9,  *,  6|9,  4,  4,  4,  *,  4,   3\r"
       b"TILE=      4, 6,  6,  6,  6,  6,  6,  6,  6,  6,   7\r")


class TileDefs(unittest.TestCase):
    def setUp(self) -> None:
        self.d = pathlib.Path(tempfile.mkdtemp())
        (self.d / "a.til").write_bytes(TIL)
        self.defs = th.tile_defs(self.d)["x.lbm"]

    def test_the_cell_is_the_first_field_not_the_last(self) -> None:
        # The corpus decides this: tile 392 in tilesb01.til is plain water and cell 392 is blue;
        # its LAST field is 0, a brown cell.
        self.assertEqual(sorted(self.defs), [0, 1, 2, 3, 4])
        self.assertTrue(self.defs[4]["pure"])

    def test_side_types_and_purity(self) -> None:
        self.assertEqual(self.defs[1]["e"], 1)
        self.assertFalse(self.defs[1]["pure"])
        self.assertEqual(self.defs[3]["n"], 6)     # ~6|9 without its own type -> the lowest named
        self.assertEqual(self.defs[3]["w"], 4)     # * -> its own type
        self.assertEqual(self.defs[3]["s"], 4)

    def test_each_side_gets_a_plain_tile_of_that_sides_terrain(self) -> None:
        n = th.neighbours(self.defs, 1)
        self.assertEqual(n["e"], 2)                # the only plain water tile
        self.assertIn(n["n"], (0, 4))              # a plain type-6 tile
        self.assertEqual(th.neighbours(self.defs, 99), dict.fromkeys(th.SIDES))

    def test_a_plain_tile_is_not_its_own_neighbour_when_another_exists(self) -> None:
        self.assertEqual(th.neighbours(self.defs, 0)["n"], 4)
        self.assertEqual(th.neighbours(self.defs, 2)["n"], 2)  # the only plain water tile


class Normalize(unittest.TestCase):
    def test_edges_move_to_the_terrain_average_and_the_interior_does_not(self) -> None:
        w2 = h2 = t2 = 16
        rgb = bytes([100, 100, 100]) * (w2 * h2)
        low = rgb
        defs = {0: {"self": 6, "n": 6, "e": 1, "s": 6, "w": 6, "pure": False}}
        means = {6: (100.0, 100.0, 100.0), 1: (0.0, 0.0, 200.0)}
        out = th.normalize_edges(rgb, low, w2, h2, t2, defs, means, 4)
        px = lambda x, y: tuple(out[(y * w2 + x) * 3:(y * w2 + x) * 3 + 3])  # noqa: E731
        self.assertEqual(px(15, 8), (0, 0, 200))         # east edge -> water average
        self.assertEqual(px(13, 8), (50, 50, 150))       # halfway into the band
        self.assertEqual(px(8, 8), (100, 100, 100))      # interior untouched
        self.assertEqual(px(0, 8), (100, 100, 100))      # west edge already at its average

    def test_detail_survives_the_shift(self) -> None:
        w2 = h2 = t2 = 8
        rgb = bytearray([100, 100, 100]) * (w2 * h2)
        rgb[(4 * w2 + 7) * 3] = 140                       # a bright detail on the east edge
        low = bytes([100, 100, 100]) * (w2 * h2)
        defs = {0: {"self": 6, "n": 6, "e": 1, "s": 6, "w": 6, "pure": False}}
        out = th.normalize_edges(bytes(rgb), low, w2, h2, t2, defs, {6: (100.0,) * 3, 1: (60.0,) * 3}, 4, damp=0)
        self.assertEqual(out[(4 * w2 + 7) * 3], 100)      # 140 + (60 - 100): the detail rides along
        damped = th.normalize_edges(bytes(rgb), low, w2, h2, t2, defs, {6: (100.0,) * 3, 1: (60.0,) * 3}, 4, damp=0.5)
        self.assertEqual(damped[(4 * w2 + 7) * 3], 80)    # 100 + 40 * 0.5 + (60 - 100)
        self.assertEqual(out[(3 * w2 + 7) * 3], 60)

    def test_cells_without_a_definition_are_untouched(self) -> None:
        rgb = bytes(range(48)) * 16
        self.assertEqual(th.normalize_edges(rgb, rgb, 16, 16, 16, {}, {}, 4), rgb)


class Quantize(unittest.TestCase):
    def test_a_neighbours_colour_cannot_cross_the_tile_edge(self) -> None:
        # Sheet-level bleed: the upscale painted tile 0's right column blue. Tile 0 owns only red.
        t, w, h = 4, 8, 4
        rgb = bytearray()
        for y in range(2 * h):
            for x in range(2 * w):
                rgb += bytes(PAL[2] if x >= 2 * t - 2 else PAL[1])
        out, _ = th.quantize(two_tiles(t), w, h, PAL, t, bytes(rgb))
        W = 2 * w
        self.assertTrue(all(out[y * W + x] == 1 for y in range(2 * h) for x in range(2 * t)))
        self.assertEqual(th.foreign_edge_pixels(two_tiles(t), w, h, t, out), 0)

    def test_the_detector_sees_sheet_bleed(self) -> None:
        # The control: the same bleed, quantized with no tile clamp, is counted.
        t, w, h = 4, 8, 4
        W = 2 * w
        bled = bytearray([1] * (W * 2 * h))
        for y in range(2 * h):
            for x in range(2 * t):
                bled[y * W + x] = 1
            bled[y * W + 2 * t - 1] = 2  # tile 0's last 2x column took tile 1's index
        for y in range(2 * h):
            for x in range(2 * t, W):
                bled[y * W + x] = 2
        self.assertEqual(th.foreign_edge_pixels(two_tiles(t), w, h, t, bytes(bled)), 2 * h)

    def test_an_index_further_than_one_source_pixel_is_never_chosen(self) -> None:
        # One tile: index 1 everywhere except a single index-2 pixel in the far corner. RGB says
        # blue everywhere; only 2x pixels within one source pixel of that corner may take index 2.
        t = w = h = 4
        idx = bytearray([1] * 16)
        idx[15] = 2
        rgb = bytes(PAL[2]) * (4 * w * h)
        out, _ = th.quantize(bytes(idx), w, h, PAL, t, rgb)
        W = 2 * w
        for y in range(2 * h):
            for x in range(W):
                near = x // 2 >= 2 and y // 2 >= 2
                self.assertEqual(out[y * W + x], 2 if near else 1, (x, y))

    def test_a_tie_keeps_the_source_index(self) -> None:
        # Indices 5 and 6 are the same colour. The source pixel is 6; the neighbour is 5.
        pal = list(PAL)
        pal[5] = pal[6] = (40, 50, 60)
        t, w, h = 2, 2, 2
        idx = bytes([5, 6, 5, 6])
        rgb = bytes(pal[5]) * (4 * w * h)
        out, _ = th.quantize(idx, w, h, pal, t, rgb)
        W = 2 * w
        self.assertEqual([out[y * W + x] for y in range(4) for x in range(2, 4)], [6] * 8)
        self.assertEqual([out[y * W + x] for y in range(4) for x in range(0, 2)], [5] * 8)


@unittest.skipUnless(shutil.which("magick"), "ImageMagick not installed")
class EndToEnd(unittest.TestCase):
    """build() with a nearest-neighbour stand-in for the model: output must be the exact 2x."""

    def setUp(self) -> None:
        self.d = pathlib.Path(tempfile.mkdtemp())
        self.src, self.out = self.d / "src", self.d / "out"
        self.src.mkdir()
        fake = self.d / "fake-esrgan"
        fake.write_text(
            "#!/bin/sh\n"
            "while [ $# -gt 0 ]; do case $1 in -i) i=$2; shift;; -o) o=$2; shift;; esac; shift; done\n"
            'for f in "$i"/*.png; do magick "$f" -filter point -resize 200% "$o/$(basename "$f")"; done\n'
        )
        fake.chmod(fake.stat().st_mode | stat.S_IEXEC)
        self.fake = fake

    def test_nearest_upscale_round_trips_and_data_maps_are_left_alone(self) -> None:
        # norm=0: the exact round trip is only defined without edge normalization.
        t, w, h = 4, 8, 8
        idx = bytes((x * 3 + y * 5) % 200 + 10 for y in range(h) for x in range(w))
        write_lbm(self.src / "tilesz01.lbm", w, h, idx)
        write_lbm(self.src / "thite01.lbm", w, h, idx)
        (self.src / "tilesz01.til").write_bytes(til("tilesz01.lbm", t))
        choices = {"terrain__tilesz01": "anime2x", "terrain__thite01": "anime2x"}
        report = th.build(self.src, self.out, self.fake, self.d, self.d / "work", choices, norm=0)
        W, H, got, pal, _ = lbm_png.decode(self.out / "tilesz01.lbm")
        self.assertEqual((W, H), (2 * w, 2 * h))
        want = bytes(idx[(y // 2) * w + x // 2] for y in range(H) for x in range(W))
        self.assertEqual(bytes(got), want)
        self.assertEqual([tuple(c) for c in pal], PAL)
        self.assertFalse((self.out / "thite01.lbm").exists())
        self.assertIn(b"TILESIZE= 8, 8", (self.out / "tilesz01.til").read_bytes())
        self.assertTrue(any("top-left keeps source index 100%" in line for line in report), report)

    def test_a_changed_source_is_not_built_from_the_previous_runs_tiles(self) -> None:
        t, w, h = 4, 8, 8
        (self.src / "tilesz01.til").write_bytes(til("tilesz01.lbm", t))
        choices = {"terrain__tilesz01": "anime2x"}
        work = self.d / "work"
        write_lbm(self.src / "tilesz01.lbm", w, h, bytes([10] * 64))
        th.build(self.src, self.out, self.fake, self.d, work, choices, norm=0)
        second = bytes((x + y) % 50 + 20 for y in range(h) for x in range(w))
        write_lbm(self.src / "tilesz01.lbm", w, h, second)
        th.build(self.src, self.out, self.fake, self.d, work, choices, norm=0)
        _, _, got, _, _ = lbm_png.decode(self.out / "tilesz01.lbm")
        self.assertEqual(bytes(got), bytes(second[(y // 2) * w + x // 2] for y in range(2 * h) for x in range(2 * w)))

    def test_normalization_leaves_every_tile_interior_exact(self) -> None:
        t, w, h = 8, 16, 8
        idx = bytes((x * 7 + y * 11) % 200 + 10 for y in range(h) for x in range(w))
        write_lbm(self.src / "tilesz01.lbm", w, h, idx)
        (self.src / "tilesz01.til").write_bytes(
            b"LBM=tilesz01.lbm\rTILES= 2, 1\rTILESIZE= 8, 8\r"
            b"TILE= 0, 6, 6, 6, 1, 6, 6, 6, 6, 6, 0\r"
            b"TILE= 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0\r")
        th.build(self.src, self.out, self.fake, self.d, self.d / "work", {"terrain__tilesz01": "anime2x"}, norm=4)
        W, H, got, _, _ = lbm_png.decode(self.out / "tilesz01.lbm")
        changed_inside = changed_band = 0
        for y in range(H):
            for x in range(W):
                e = min(x % 16, y % 16, 15 - x % 16, 15 - y % 16)
                differs = got[y * W + x] != idx[(y // 2) * w + x // 2]
                if e >= 4:
                    changed_inside += differs
                else:
                    changed_band += differs
        self.assertEqual(changed_inside, 0)
        self.assertGreater(changed_band, 0)  # the control: normalization did something in the band

    def interrupted_run(self, target, name: str, output_of) -> None:
        """Run build() with `target.name` wrapped so that the file it writes is left half-written
        and the run stops there, as a killed run leaves it; then run build() again normally."""
        t, w, h = 4, 8, 8
        idx = bytes((x * 3 + y * 5) % 200 + 10 for y in range(h) for x in range(w))
        write_lbm(self.src / "tilesz01.lbm", w, h, idx)
        (self.src / "tilesz01.til").write_bytes(til("tilesz01.lbm", t))
        real = getattr(target, name)

        def half_then_stop(*args, **kwargs):
            real(*args, **kwargs)
            written = output_of(*args)
            written.write_bytes(written.read_bytes()[:40])
            raise KeyboardInterrupt

        setattr(target, name, half_then_stop)
        try:
            with self.assertRaises(KeyboardInterrupt):
                th.build(self.src, self.out, self.fake, self.d, self.d / "work", {"terrain__tilesz01": "anime4x"}, norm=0)
        finally:
            setattr(target, name, real)
        th.build(self.src, self.out, self.fake, self.d, self.d / "work", {"terrain__tilesz01": "anime4x"}, norm=0)
        _, _, got, _, _ = lbm_png.decode(self.out / "tilesz01.lbm")
        self.assertEqual(bytes(got), bytes(idx[(y // 2) * w + x // 2] for y in range(2 * h) for x in range(2 * w)))

    def test_a_tile_cut_short_by_a_stopped_run_is_not_reused(self) -> None:
        self.interrupted_run(th.lbm_png, "write_png", lambda path, *rest: pathlib.Path(path))

    def test_a_render_cut_short_by_a_stopped_run_is_not_reused(self) -> None:
        """hd_upscale.render skips any output that exists: one cut short must never be there. Only
        its resize step (which writes the output) is interrupted; the stand-in model runs as is."""
        import types
        import hd_upscale

        real_run = hd_upscale.subprocess.run
        resize = types.SimpleNamespace(run=real_run)

        def run(cmd, *args, **kwargs):
            if cmd[0] == "magick" and "-resize" in cmd:
                return resize.run(cmd, *args, **kwargs)
            return real_run(cmd, *args, **kwargs)

        original = hd_upscale.subprocess          # only render's own reference is swapped
        hd_upscale.subprocess = types.SimpleNamespace(run=run)
        try:
            self.interrupted_run(resize, "run", lambda cmd, *rest: pathlib.Path(str(cmd[-1]).removeprefix("PNG:")))
        finally:
            hd_upscale.subprocess = original

    def test_an_atlas_without_a_reviewed_choice_refuses(self) -> None:
        write_lbm(self.src / "tilesz01.lbm", 8, 8, bytes(64))
        with self.assertRaises(SystemExit):
            th.build(self.src, self.out, self.fake, self.d, self.d / "work", {})


class Corpus(unittest.TestCase):
    def test_every_shipped_atlas_has_a_reviewed_full_colour_choice(self) -> None:
        choices = json.loads(th.CHOICES.read_text())["choices"]
        import hd_upscale
        terrain = {k: v for k, v in choices.items() if k.startswith("terrain__")}
        self.assertEqual(len(terrain), 22)
        for key, option in terrain.items():
            if key.removeprefix("terrain__") not in th.NOT_TEXTURES:
                self.assertIn(option, hd_upscale.OPTIONS, key)


if __name__ == "__main__":
    unittest.main()
