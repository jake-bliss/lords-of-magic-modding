"""The image pack the HD overlay reads: what goes in, what is refused, and what is reported."""

from __future__ import annotations

import pathlib
import struct
import sys
import tempfile
import unittest
import zlib

TOOLS = pathlib.Path(__file__).resolve().parent.parent / "tools"
sys.path.insert(0, str(TOOLS))
sys.path.insert(0, str(TOOLS / "portrait-upscale"))

import hd_portrait_pack as pack  # noqa: E402
import hd_upscale  # noqa: E402
import lbm_png  # noqa: E402

PALETTE = [(i, 255 - i, (i * 7) % 256) for i in range(256)]
W, H = 40, 6                       # the smallest shape the overlay accepts is 32 wide, 4 tall


def write_lbm(path: pathlib.Path, width: int, height: int, seed: int) -> bytes:
    indices = bytes((x * 3 + y * 5 + seed) % 256 for y in range(height) for x in range(width))
    header = struct.pack(">HHhhBBBBHBBhh", width, height, 0, 0, 8, 0, 1, 0, 0, 1, 1, width, height)
    lbm_png.encode(path, width, height, indices, PALETTE,
                   [(b"BMHD", header), (b"CMAP", b""), (b"BODY", b"")])
    return indices


def rgb_of(indices: bytes) -> bytes:
    return b"".join(bytes(PALETTE[i]) for i in indices)


class Pack(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = pathlib.Path(self.tmp.name)
        self.small, self.large = root / "small", root / "large"
        self.small.mkdir(); self.large.mkdir()

    def test_a_pair_round_trips_exactly_as_full_colour(self) -> None:
        original = write_lbm(self.small / "aicavp00.lbm", W, H, 1)
        upscale = write_lbm(self.large / "aicavp00.lbm", W * 2, H * 2, 2)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual(skipped, [])
        [(name, small, large)] = pack.read(data)
        self.assertEqual(name, "aicavp00")
        self.assertEqual((small[0], small[1], bytes(small[3])), (W, H, original))
        self.assertEqual(small[2], PALETTE, "the original's palette is carried, not re-derived")
        self.assertEqual(large, (W * 2, H * 2, rgb_of(upscale)), "the upscale is stored as its colours")

    def test_images_of_different_widths_share_one_pack(self) -> None:
        """Buildings come in 34 widths; format 1 allowed one."""
        write_lbm(self.small / "portrait.lbm", 70, 67, 1)
        write_lbm(self.large / "portrait.lbm", 140, 134, 2)
        write_lbm(self.small / "llwizt1a.lbm", 143, 12, 3)
        write_lbm(self.large / "llwizt1a.lbm", 286, 24, 4)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual(skipped, [])
        self.assertEqual(sorted((n, s[0]) for n, s, _ in pack.read(data)), [("llwizt1a", 143), ("portrait", 70)])

    def test_originals_and_upscales_may_come_from_several_folders(self) -> None:
        more_small = pathlib.Path(self.tmp.name) / "buildings"; more_small.mkdir()
        more_large = pathlib.Path(self.tmp.name) / "buildings-up"; more_large.mkdir()
        write_lbm(self.small / "a.lbm", W, H, 1); write_lbm(self.large / "a.lbm", W * 2, H * 2, 2)
        write_lbm(more_small / "b.lbm", W, H, 3); write_lbm(more_large / "b.lbm", W * 2, H * 2, 4)
        data, _ = pack.build([self.small, more_small], [self.large, more_large])
        self.assertEqual([n for n, _, _ in pack.read(data)], ["a", "b"])

    def test_an_uppercase_extension_still_pairs(self) -> None:
        """8 of the 748 shipped portraits are spelled `.LBM`; a case-sensitive glob dropped them."""
        write_lbm(self.small / "EAINFp00.LBM", W, H, 1)
        write_lbm(self.large / "eainfp00.lbm", W * 2, H * 2, 1)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual(len(pack.read(data)), 1)
        self.assertEqual(skipped, [])

    def test_an_image_without_an_upscale_is_reported_not_dropped_silently(self) -> None:
        write_lbm(self.small / "aipotm.lbm", W, H, 1)
        write_lbm(self.small / "aicavp00.lbm", W, H, 2)
        write_lbm(self.large / "aicavp00.lbm", W * 2, H * 2, 2)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual(len(pack.read(data)), 1)
        self.assertEqual(skipped, ["aipotm: no upscale"])

    def test_the_same_image_upscaled_twice_is_refused(self) -> None:
        """Two upscale folders naming one image: which to draw is a guess, so refuse."""
        other = pathlib.Path(self.tmp.name) / "other"; other.mkdir()
        write_lbm(self.small / "aicavp00.lbm", W, H, 1)
        write_lbm(self.large / "aicavp00.lbm", W * 2, H * 2, 1)
        write_lbm(other / "AICAVP00.lbm", W * 2, H * 2, 3)
        with self.assertRaises(SystemExit):
            pack.build(self.small, [self.large, other])

    def test_a_short_palette_is_refused(self) -> None:
        """Padding it would shift every later record; the reader would misparse the whole pack."""
        with self.assertRaises(ValueError):
            pack.encode_record("a", 2, 2, b"\0" * 4, PALETTE[:16], 4, 4, b"\0" * 48)

    def test_an_install_whose_original_differs_from_the_source_is_not_given_that_upscale(self) -> None:
        """The vanilla and GS5R3 installs share a portrait NAME whose picture differs. Pairing by
        name would draw one install's upscale over the other install's picture."""
        sources = pathlib.Path(self.tmp.name) / "sources"; sources.mkdir()
        write_lbm(self.small / "life.lbm", W, H, 1)           # what the player's install holds
        write_lbm(sources / "LIFE.lbm", W, H, 9)              # what the upscale was made from
        write_lbm(self.large / "life.lbm", W * 2, H * 2, 9)
        write_lbm(self.small / "lildwp00.lbm", W, H, 2)
        write_lbm(sources / "LILDWP00.LBM", W, H, 2)
        write_lbm(self.large / "lildwp00.lbm", W * 2, H * 2, 2)
        data, skipped = pack.build(self.small, [self.large], sources)
        self.assertEqual([name for name, _, _ in pack.read(data)], ["lildwp00"])
        self.assertEqual(skipped, ["life: installed original differs from the one the upscale was made from"])

    def test_what_the_overlay_would_refuse_fails_at_build_time(self) -> None:
        """The C reader refuses these; the writer must refuse them first, or the game loads a
        pack it calls corrupt and the overlay is silently off."""
        cases = {
            "narrower than the probe slice": [("a.lbm", 31, 6, 62, 12)],
            "too few rows for three probes": [("a.lbm", W, 3, W * 2, 6)],
            "an upscale larger than the overlay's limit": [("a.lbm", W, H, 1281, 12)],
        }
        for label, images in cases.items():
            with self.subTest(label), tempfile.TemporaryDirectory() as tmp:
                small, large = pathlib.Path(tmp) / "s", pathlib.Path(tmp) / "l"
                small.mkdir(); large.mkdir()
                for seed, (name, w, h, hw, hh) in enumerate(images):
                    write_lbm(small / name, w, h, seed)
                    write_lbm(large / name, hw, hh, seed)
                with self.assertRaises(SystemExit):
                    pack.build(small, [large])

    def test_a_pixel_doubled_upscale_is_left_out_and_reported(self) -> None:
        """395 of the portraits in the pack played on 2026-09-22 were 2x2 repeats: drawn at the
        slot's size they are the original, so the overlay changed nothing for them."""
        indices = write_lbm(self.small / "d1_great_axe.lbm", W, H, 1)
        doubled = bytes(indices[(y // 2) * W + x // 2] for y in range(H * 2) for x in range(W * 2))
        header = struct.pack(">HHhhBBBBHBBhh", W * 2, H * 2, 0, 0, 8, 0, 1, 0, 0, 1, 1, W * 2, H * 2)
        lbm_png.encode(self.large / "d1_great_axe.lbm", W * 2, H * 2, doubled, PALETTE,
                       [(b"BMHD", header), (b"CMAP", b""), (b"BODY", b"")])
        write_lbm(self.small / "real.lbm", W, H, 2)
        write_lbm(self.large / "real.lbm", W * 2, H * 2, 5)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual([n for n, _, _ in pack.read(data)], ["real"])
        self.assertEqual(skipped, ["d1_great_axe: the upscale is the original with each pixel repeated, not an upscale"])

    def test_the_index_comes_first_and_the_streams_follow_in_order(self) -> None:
        """The overlay reads the index alone at start and each stream only when it needs it, by
        offsets it computes from the lengths -- so the layout must be exactly this."""
        a = write_lbm(self.small / "a.lbm", W, H, 1); up_a = write_lbm(self.large / "a.lbm", W * 2, H * 2, 2)
        b = write_lbm(self.small / "b.lbm", W, H, 3); up_b = write_lbm(self.large / "b.lbm", W * 2, H * 2, 4)
        data, _ = pack.build(self.small, [self.large])
        self.assertEqual(data[:8], b"LOMHDPK3")
        self.assertEqual(struct.unpack_from("<I", data, 8), (2,))
        pos, lengths = 12, []
        for name in ("a", "b"):
            self.assertEqual(data[pos:pos + 2], bytes([1]) + name.encode())
            self.assertEqual(struct.unpack_from("<HHHH", data, pos + 2), (W, H, W * 2, H * 2))
            lengths.append(struct.unpack_from("<II", data, pos + 10 + 768))
            pos += 2 + 8 + 768 + 8
        for (idx_len, hd_len), idx, up in ((lengths[0], a, up_a), (lengths[1], b, up_b)):
            self.assertEqual(zlib.decompress(data[pos:pos + idx_len]), idx); pos += idx_len
            self.assertEqual(zlib.decompress(data[pos:pos + hd_len]), rgb_of(up)); pos += hd_len
        self.assertEqual(pos, len(data))
        self.assertEqual(pack.count(self._write(data)), 2)

    def _write(self, data: bytes) -> pathlib.Path:
        path = pathlib.Path(self.tmp.name) / "out.pack"
        path.write_bytes(data)
        return path

    def test_an_empty_pack_is_refused(self) -> None:
        """The overlay refuses a count of zero as corrupt; write nothing rather than that."""
        write_lbm(self.small / "aipotm.lbm", W, H, 1)
        with self.assertRaises(SystemExit):
            pack.build(self.small, [self.large])

    def test_the_limits_are_the_overlays(self) -> None:
        """Tied to src/lomhd_match.c: count x 2 rules x 3 probes must fit half of 65,536 slots, and a
        640x480 screen at 2x (1280 wide) must be accepted."""
        self.assertLessEqual(pack.MAX_IMAGES * 6, 65536 // 2)
        self.assertGreater((pack.MAX_IMAGES + 1) * 6, 65536 // 2)
        pack.check_reader_limits("screen", 640, 480, 1280, 960)
        with self.assertRaises(SystemExit):
            pack.check_reader_limits("screen", 640, 480, 1281, 960)

    def test_trailing_bytes_are_an_error(self) -> None:
        write_lbm(self.small / "a.lbm", W, H, 1)
        write_lbm(self.large / "a.lbm", W * 2, H * 2, 1)
        data, _ = pack.build(self.small, [self.large])
        with self.assertRaises(ValueError):
            pack.read(data + b"\0")


class Choices(unittest.TestCase):
    """What the setup does with an image the review never saw (another install's extra art)."""

    def test_character_portraits_default_to_the_approved_pipeline(self) -> None:
        for stem in ("lildwp00", "orwizp12", "blankp01"):
            self.assertEqual(hd_upscale.default_choice("portrait", stem), hd_upscale.APPROVED, stem)

    def test_everything_else_defaults_to_the_most_picked_option(self) -> None:
        for group, stem in (("portrait", "aiamul"), ("portrait", "d1_great_axe"), ("building", "aagtwr0a")):
            self.assertEqual(hd_upscale.default_choice(group, stem), "ultrasharp-tta", stem)

    def test_the_shipped_choices_cover_only_known_options(self) -> None:
        import json
        path = TOOLS.parent / "release" / "hd-overlay" / "upscale-choices.json"
        choices = json.loads(path.read_text())["choices"]
        allowed = set(hd_upscale.OPTIONS) | {hd_upscale.APPROVED, "original"}
        self.assertEqual({v for v in choices.values()} - allowed, set())
        self.assertGreater(len(choices), 900)


if __name__ == "__main__":
    unittest.main()
