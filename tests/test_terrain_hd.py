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
        t, w, h = 4, 8, 8
        idx = bytes((x * 3 + y * 5) % 200 + 10 for y in range(h) for x in range(w))
        write_lbm(self.src / "tilesz01.lbm", w, h, idx)
        write_lbm(self.src / "thite01.lbm", w, h, idx)
        (self.src / "tilesz01.til").write_bytes(til("tilesz01.lbm", t))
        choices = {"terrain__tilesz01": "anime2x", "terrain__thite01": "anime2x"}
        report = th.build(self.src, self.out, self.fake, self.d, self.d / "work", choices)
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
        th.build(self.src, self.out, self.fake, self.d, work, choices)
        second = bytes((x + y) % 50 + 20 for y in range(h) for x in range(w))
        write_lbm(self.src / "tilesz01.lbm", w, h, second)
        th.build(self.src, self.out, self.fake, self.d, work, choices)
        _, _, got, _, _ = lbm_png.decode(self.out / "tilesz01.lbm")
        self.assertEqual(bytes(got), bytes(second[(y // 2) * w + x // 2] for y in range(2 * h) for x in range(2 * w)))

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
