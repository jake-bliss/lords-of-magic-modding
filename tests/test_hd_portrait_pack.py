"""The portrait pack the HD overlay reads: what goes in, what is refused, and what is reported."""

from __future__ import annotations

import pathlib
import struct
import sys
import tempfile
import unittest

TOOLS = pathlib.Path(__file__).resolve().parent.parent / "tools"
sys.path.insert(0, str(TOOLS))
sys.path.insert(0, str(TOOLS / "portrait-upscale"))

import hd_portrait_pack as pack  # noqa: E402
import lbm_png  # noqa: E402

PALETTE = [(i, 255 - i, (i * 7) % 256) for i in range(256)]


def write_lbm(path: pathlib.Path, width: int, height: int, seed: int) -> bytes:
    indices = bytes((x * 3 + y * 5 + seed) % 256 for y in range(height) for x in range(width))
    header = struct.pack(">HHhhBBBBHBBhh", width, height, 0, 0, 8, 0, 1, 0, 0, 1, 1, width, height)
    lbm_png.encode(path, width, height, indices, PALETTE,
                   [(b"BMHD", header), (b"CMAP", b""), (b"BODY", b"")])
    return indices


class Pack(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = pathlib.Path(self.tmp.name)
        self.small, self.large = root / "small", root / "large"
        self.small.mkdir(); self.large.mkdir()

    def test_a_pair_round_trips_exactly(self) -> None:
        original = write_lbm(self.small / "aicavp00.lbm", 7, 5, 1)
        upscale = write_lbm(self.large / "aicavp00.lbm", 14, 10, 2)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual(skipped, [])
        [(name, small, large)] = pack.read(data)
        self.assertEqual(name, "aicavp00.lbm")
        self.assertEqual((small[0], small[1], bytes(small[3])), (7, 5, original))
        self.assertEqual((large[0], large[1], bytes(large[3])), (14, 10, upscale))
        self.assertEqual(small[2], PALETTE, "the palette is carried, not re-derived")

    def test_an_uppercase_extension_still_pairs(self) -> None:
        """8 of the 748 shipped portraits are spelled `.LBM`; a case-sensitive glob dropped them."""
        write_lbm(self.small / "EAINFp00.LBM", 4, 4, 1)
        write_lbm(self.large / "eainfp00.lbm", 8, 8, 1)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual(len(pack.read(data)), 1)
        self.assertEqual(skipped, [])

    def test_a_portrait_without_an_upscale_is_reported_not_dropped_silently(self) -> None:
        write_lbm(self.small / "aipotm.lbm", 4, 4, 1)
        write_lbm(self.small / "aicavp00.lbm", 4, 4, 2)
        write_lbm(self.large / "aicavp00.lbm", 8, 8, 2)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual(len(pack.read(data)), 1)
        self.assertEqual(skipped, ["aipotm.lbm: no upscale"])

    def test_the_same_portrait_upscaled_twice_is_refused(self) -> None:
        """Two upscale directories naming one portrait: which to draw is a guess, so refuse."""
        other = pathlib.Path(self.tmp.name) / "other"; other.mkdir()
        write_lbm(self.small / "aicavp00.lbm", 4, 4, 1)
        write_lbm(self.large / "aicavp00.lbm", 8, 8, 1)
        write_lbm(other / "AICAVP00.lbm", 8, 8, 3)
        with self.assertRaises(SystemExit):
            pack.build(self.small, [self.large, other])

    def test_a_short_palette_is_refused(self) -> None:
        """Padding it would shift every later record; the reader would misparse the whole pack."""
        with self.assertRaises(ValueError):
            pack.encode_image(2, 2, b"\0" * 4, PALETTE[:16])

    def test_an_install_whose_original_differs_from_the_source_is_not_given_that_upscale(self) -> None:
        """The vanilla and GS5R3 installs share a portrait NAME whose picture differs. Pairing by
        name would draw one install's upscale over the other install's picture."""
        sources = pathlib.Path(self.tmp.name) / "sources"; sources.mkdir()
        write_lbm(self.small / "life.lbm", 4, 4, 1)          # what the player's install holds
        write_lbm(sources / "LIFE.lbm", 4, 4, 9)             # what the upscale was made from
        write_lbm(self.large / "life.lbm", 8, 8, 9)
        write_lbm(self.small / "lildwp00.lbm", 4, 4, 2)
        write_lbm(sources / "LILDWP00.LBM", 4, 4, 2)
        write_lbm(self.large / "lildwp00.lbm", 8, 8, 2)
        data, skipped = pack.build(self.small, [self.large], sources)
        self.assertEqual([name for name, _, _ in pack.read(data)], ["lildwp00.lbm"])
        self.assertEqual(skipped, ["life.lbm: installed original differs from the one the upscale was made from"])

    def test_what_the_overlay_would_refuse_fails_at_build_time(self) -> None:
        """The C reader refuses these; the writer must refuse them first, or the game loads a
        pack it calls corrupt and the overlay is silently off."""
        cases = {
            "a different width": [("a.lbm", 4, 4, 8, 8), ("b.lbm", 5, 4, 10, 8)],
            "too few rows for three probes": [("a.lbm", 4, 3, 8, 6)],
            "an upscale larger than the overlay's buffer": [("a.lbm", 4, 4, 513, 8)],
        }
        for label, portraits in cases.items():
            with self.subTest(label), tempfile.TemporaryDirectory() as tmp:
                small, large = pathlib.Path(tmp) / "s", pathlib.Path(tmp) / "l"
                small.mkdir(); large.mkdir()
                for seed, (name, w, h, hw, hh) in enumerate(portraits):
                    write_lbm(small / name, w, h, seed)
                    write_lbm(large / name, hw, hh, seed)
                with self.assertRaises(SystemExit):
                    pack.build(small, [large])

    def test_a_pixel_doubled_upscale_is_left_out_and_reported(self) -> None:
        """395 of the portraits in the pack played on 2026-09-22 were 2x2 repeats: drawn at the
        slot's size they are the original, so the overlay changed nothing for them."""
        indices = write_lbm(self.small / "d1_great_axe.lbm", 4, 4, 1)
        doubled = bytes(indices[(y // 2) * 4 + x // 2] for y in range(8) for x in range(8))
        header = struct.pack(">HHhhBBBBHBBhh", 8, 8, 0, 0, 8, 0, 1, 0, 0, 1, 1, 8, 8)
        lbm_png.encode(self.large / "d1_great_axe.lbm", 8, 8, doubled, PALETTE,
                       [(b"BMHD", header), (b"CMAP", b""), (b"BODY", b"")])
        write_lbm(self.small / "real.lbm", 4, 4, 2)
        write_lbm(self.large / "real.lbm", 8, 8, 5)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual([n for n, _, _ in pack.read(data)], ["real.lbm"])
        self.assertEqual(skipped, ["d1_great_axe.lbm: the upscale is the original with each pixel repeated, not an upscale"])

    def test_trailing_bytes_are_an_error(self) -> None:
        write_lbm(self.small / "a.lbm", 4, 4, 1)
        write_lbm(self.large / "a.lbm", 8, 8, 1)
        data, _ = pack.build(self.small, [self.large])
        with self.assertRaises(ValueError):
            pack.read(data + b"\0")


if __name__ == "__main__":
    unittest.main()
