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


def write_upscale(path: pathlib.Path, width: int, height: int, seed: int) -> bytes:
    """A plausible 2x upscale of `write_lbm(..., width, height, seed)`: each pixel as a 2x2 block,
    with one pixel nudged so it is not a bare pixel repeat (which the pack refuses). The content
    check (hd_upscale.damage_score) passes it, as it passes a real upscale."""
    small = [(x * 3 + y * 5 + seed) % 256 for y in range(height) for x in range(width)]
    indices = bytearray(small[(y // 2) * width + x // 2] for y in range(height * 2) for x in range(width * 2))
    indices[-1] = (indices[-1] + 1) % 256
    header = struct.pack(">HHhhBBBBHBBhh", width * 2, height * 2, 0, 0, 8, 0, 1, 0, 0, 1, 1,
                         width * 2, height * 2)
    lbm_png.encode(path, width * 2, height * 2, bytes(indices), PALETTE,
                   [(b"BMHD", header), (b"CMAP", b""), (b"BODY", b"")])
    return bytes(indices)


def rgb_of(indices: bytes) -> bytes:
    return b"".join(bytes(PALETTE[i]) for i in indices)


def masked_sprite(w: int, h: int, key: int, border: int = 2) -> bytes:
    """Indices for a synthetic sprite: `border` columns of the transparent `key` colour on each
    side, opaque pixels (a value that is neither `key` nor the shadow index) in between. Every row
    then has a run of `w - 2 * border` opaque pixels, which is >= MASKED_MIN_OPAQUE_RUN whenever
    the caller picked `w` and `border` to make it so."""
    opaque = 3 if key != 3 else 4
    row = bytes([key] * border + [opaque] * (w - 2 * border) + [key] * border)
    return row * h


class Pack(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = pathlib.Path(self.tmp.name)
        self.small, self.large = root / "small", root / "large"
        self.small.mkdir(); self.large.mkdir()

    def test_a_pair_round_trips_exactly_as_full_colour(self) -> None:
        original = write_lbm(self.small / "aicavp00.lbm", W, H, 1)
        upscale = write_upscale(self.large / "aicavp00.lbm", W, H, 2)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual(skipped, [])
        [(name, small, large, flags, key, _)] = pack.read(data)
        self.assertEqual(name, "aicavp00")
        self.assertEqual((flags, key), (0, 0), "a picture is unmasked, with no colour key")
        self.assertEqual((small[0], small[1], bytes(small[3])), (W, H, original))
        self.assertEqual(small[2], PALETTE, "the original's palette is carried, not re-derived")
        self.assertEqual(large, (W * 2, H * 2, rgb_of(upscale)), "the upscale is stored as its colours")

    def test_images_of_different_widths_share_one_pack(self) -> None:
        """Buildings come in 34 widths; format 1 allowed one."""
        write_lbm(self.small / "portrait.lbm", 70, 67, 1)
        write_upscale(self.large / "portrait.lbm", 70, 67, 2)
        write_lbm(self.small / "llwizt1a.lbm", 143, 12, 3)
        write_upscale(self.large / "llwizt1a.lbm", 143, 12, 4)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual(skipped, [])
        self.assertEqual(sorted((n, s[0]) for n, s, _, _, _, _ in pack.read(data)),
                         [("llwizt1a", 143), ("portrait", 70)])

    def test_originals_and_upscales_may_come_from_several_folders(self) -> None:
        more_small = pathlib.Path(self.tmp.name) / "buildings"; more_small.mkdir()
        more_large = pathlib.Path(self.tmp.name) / "buildings-up"; more_large.mkdir()
        write_lbm(self.small / "a.lbm", W, H, 1); write_upscale(self.large / "a.lbm", W, H, 2)
        write_lbm(more_small / "b.lbm", W, H, 3); write_upscale(more_large / "b.lbm", W, H, 4)
        data, _ = pack.build([self.small, more_small], [self.large, more_large])
        self.assertEqual([n for n, *_ in pack.read(data)], ["a", "b"])

    def test_an_uppercase_extension_still_pairs(self) -> None:
        """8 of the 748 shipped portraits are spelled `.LBM`; a case-sensitive glob dropped them."""
        write_lbm(self.small / "EAINFp00.LBM", W, H, 1)
        write_upscale(self.large / "eainfp00.lbm", W, H, 1)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual(len(pack.read(data)), 1)
        self.assertEqual(skipped, [])

    def test_an_image_without_an_upscale_is_reported_not_dropped_silently(self) -> None:
        write_lbm(self.small / "aipotm.lbm", W, H, 1)
        write_lbm(self.small / "aicavp00.lbm", W, H, 2)
        write_upscale(self.large / "aicavp00.lbm", W, H, 2)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual(len(pack.read(data)), 1)
        self.assertEqual(skipped, ["aipotm: no upscale"])

    def test_the_same_image_upscaled_twice_is_refused(self) -> None:
        """Two upscale folders naming one image: which to draw is a guess, so refuse."""
        other = pathlib.Path(self.tmp.name) / "other"; other.mkdir()
        write_lbm(self.small / "aicavp00.lbm", W, H, 1)
        write_upscale(self.large / "aicavp00.lbm", W, H, 1)
        write_upscale(other / "AICAVP00.lbm", W, H, 3)
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
        write_upscale(self.large / "life.lbm", W, H, 9)
        write_lbm(self.small / "lildwp00.lbm", W, H, 2)
        write_lbm(sources / "LILDWP00.LBM", W, H, 2)
        write_upscale(self.large / "lildwp00.lbm", W, H, 2)
        data, skipped = pack.build(self.small, [self.large], sources)
        self.assertEqual([name for name, *_ in pack.read(data)], ["lildwp00"])
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
        write_upscale(self.large / "real.lbm", W, H, 5)
        data, skipped = pack.build(self.small, [self.large])
        self.assertEqual([n for n, *_ in pack.read(data)], ["real"])
        self.assertEqual(skipped, ["d1_great_axe: the upscale is the original with each pixel repeated, not an upscale"])

    def test_the_index_comes_first_and_the_streams_follow_in_order(self) -> None:
        """The overlay reads the index alone at start and each stream only when it needs it, by
        offsets it computes from the lengths -- so the layout must be exactly this."""
        a = write_lbm(self.small / "a.lbm", W, H, 1); up_a = write_upscale(self.large / "a.lbm", W, H, 2)
        b = write_lbm(self.small / "b.lbm", W, H, 3); up_b = write_upscale(self.large / "b.lbm", W, H, 4)
        data, _ = pack.build(self.small, [self.large])
        self.assertEqual(data[:8], b"LOMHDPK5")
        self.assertEqual(struct.unpack_from("<I", data, 8), (2,))
        pos, lengths = 12, []
        for name in ("a", "b"):
            self.assertEqual(data[pos:pos + 2], bytes([1]) + name.encode())
            self.assertEqual(struct.unpack_from("<HHHH", data, pos + 2), (W, H, W * 2, H * 2))
            self.assertEqual(data[pos + 10:pos + 14], b"\x00\x00\x00\x00", "flags, key, group 0 for a picture")
            lengths.append(struct.unpack_from("<II", data, pos + 14 + 768))
            pos += 2 + 8 + 4 + 768 + 8
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
        """Tied to src/lomhd_match.c: LOMHD_MAX_IMAGES records and LOMHD_MAX_PROBES probes, of which
        a picture takes PICTURE_PROBE_SLOTS -- so a pack of pictures alone meets the image limit
        first; a 640x480 screen at 2x (1280 wide) must be accepted."""
        self.assertEqual(pack.MAX_IMAGES, 131072)
        self.assertEqual(pack.MAX_PROBES, 1 << 20)
        self.assertLessEqual(pack.MAX_IMAGES * pack.PICTURE_PROBE_SLOTS, pack.MAX_PROBES)
        pack.check_reader_limits("screen", 640, 480, 1280, 960)
        with self.assertRaises(SystemExit):
            pack.check_reader_limits("screen", 640, 480, 1281, 960)

    def test_trailing_bytes_are_an_error(self) -> None:
        write_lbm(self.small / "a.lbm", W, H, 1)
        write_upscale(self.large / "a.lbm", W, H, 1)
        data, _ = pack.build(self.small, [self.large])
        with self.assertRaises(ValueError):
            pack.read(data + b"\0")


class MaskedRecords(unittest.TestCase):
    """Format 4's masked (sprite) records: built directly with `encode_record`, since a sprite
    never comes from a name-paired folder of LBMs."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def sprite(self, w=20, h=6, key=5):
        indices = masked_sprite(w, h, key)
        rgba = b"".join(bytes((i % 256, (i * 2) % 256, (i * 3) % 256, 255 if i % 4 else 128))
                       for i in range(w * 2 * h * 2))
        return indices, rgba

    def test_a_masked_record_round_trips_with_straight_rgba(self) -> None:
        w, h, key = 20, 6, 5
        indices, rgba = self.sprite(w, h, key)
        entry, zidx, zhd = pack.encode_record("sprite__orc", w, h, indices, PALETTE, w * 2, h * 2,
                                              rgba, flags=pack.FLAG_MASKED, key=key)
        out = pathlib.Path(self.tmp.name) / "out.pack"
        pack.write_records(out, [(entry, zidx, zhd)])
        [(name, small, large, flags, got_key, _)] = pack.read(out.read_bytes())
        self.assertEqual(name, "sprite__orc")
        self.assertEqual(flags, pack.FLAG_MASKED)
        self.assertEqual(got_key, key)
        self.assertEqual((small[0], small[1], bytes(small[3])), (w, h, indices))
        self.assertEqual(large, (w * 2, h * 2, rgba), "RGBA is carried straight, not premultiplied")
        self.assertEqual(len(large[2]), (w * 2) * (h * 2) * 4, "four bytes a pixel for a masked record")

    def test_masked_header_bytes_place_flags_key_and_group_after_the_sizes(self) -> None:
        w, h, key = 20, 6, 200
        indices, rgba = self.sprite(w, h, key)
        entry, zidx, zhd = pack.encode_record("sprite__x", w, h, indices, PALETTE, w * 2, h * 2,
                                              rgba, flags=pack.FLAG_MASKED, key=key)
        n = len("sprite__x")
        self.assertEqual(entry[0], n)
        self.assertEqual(entry[1:1 + n], b"sprite__x")
        pos = 1 + n
        self.assertEqual(struct.unpack_from("<HHHH", entry, pos), (w, h, w * 2, h * 2))
        self.assertEqual(entry[pos + 8:pos + 12], bytes([pack.FLAG_MASKED, key, 0, 0]))
        palette_start = pos + 12
        self.assertEqual(entry[palette_start:palette_start + 768],
                         bytes(c for colour in PALETTE for c in colour))
        idx_len, hd_len = struct.unpack_from("<II", entry, palette_start + 768)
        self.assertEqual((idx_len, hd_len), (len(zidx), len(zhd)))
        self.assertEqual(len(entry), palette_start + 768 + 8, "nothing follows the two lengths")

    def test_a_mixed_pack_reads_back_both_kinds(self) -> None:
        picture_indices = bytes((x + y) % 256 for y in range(H) for x in range(W))
        picture_rgb = rgb_of(picture_indices)
        picture = pack.encode_record("aicavp00", W, H, picture_indices, PALETTE, W * 2, H * 2,
                                     rgb_of(bytes((x + y) % 256 for y in range(H * 2) for x in range(W * 2))))
        sprite_indices, sprite_rgba = self.sprite()
        sprite = pack.encode_record("sprite__orc", 20, 6, sprite_indices, PALETTE, 40, 12,
                                    sprite_rgba, flags=pack.FLAG_MASKED, key=5)
        out = pathlib.Path(self.tmp.name) / "out.pack"
        pack.write_records(out, [picture, sprite])
        records = {name: (flags, key) for name, _, _, flags, key, _ in pack.read(out.read_bytes())}
        self.assertEqual(records, {"aicavp00": (0, 0), "sprite__orc": (pack.FLAG_MASKED, 5)})

    def test_unknown_flag_bits_are_refused(self) -> None:
        indices, rgba = self.sprite()
        with self.assertRaises(ValueError):
            pack.encode_record("sprite__x", 20, 6, indices, PALETTE, 40, 12, rgba,
                               flags=pack.FLAG_MASKED | 0x04, key=5)

    def test_an_unmasked_record_with_a_nonzero_key_is_refused(self) -> None:
        indices = bytes((x + y) % 256 for y in range(H) for x in range(W))
        rgb = rgb_of(bytes((x + y) % 256 for y in range(H * 2) for x in range(W * 2)))
        with self.assertRaises(ValueError):
            pack.encode_record("a", W, H, indices, PALETTE, W * 2, H * 2, rgb, key=1)

    def test_a_wrong_length_rgba_stream_is_refused(self) -> None:
        indices, rgba = self.sprite()
        with self.assertRaises(ValueError):
            pack.encode_record("sprite__x", 20, 6, indices, PALETTE, 40, 12, rgba[:-4],
                               flags=pack.FLAG_MASKED, key=5)

    # --- the eligibility rule, at its edges -----------------------------------------------------

    def test_a_run_of_exactly_the_minimum_qualifies_a_row(self) -> None:
        key = 0
        indices = bytes([1] + [3] * pack.MASKED_MIN_OPAQUE_RUN)   # shadow, then a run of 16
        self.assertEqual(pack.masked_opaque_rows(len(indices), 1, indices, key), 1)

    def test_a_run_one_short_of_the_minimum_does_not_qualify(self) -> None:
        key = 0
        indices = bytes([1] + [3] * (pack.MASKED_MIN_OPAQUE_RUN - 1))
        self.assertEqual(pack.masked_opaque_rows(len(indices), 1, indices, key), 0)

    def test_both_the_key_and_the_shadow_index_break_a_run(self) -> None:
        """A run of 15 either side of an interruption never reaches the 16-pixel minimum -- proof
        the interruption resets the run instead of the two halves being read as one 31-long run."""
        key, run = 9, pack.MASKED_MIN_OPAQUE_RUN - 1
        key_broken = bytes([3] * run + [key] + [3] * run)
        self.assertEqual(pack.masked_opaque_rows(len(key_broken), 1, key_broken, key), 0)
        shadow_broken = bytes([3] * run + [pack.SHADOW_INDEX] + [3] * run)
        self.assertEqual(pack.masked_opaque_rows(len(shadow_broken), 1, shadow_broken, key), 0,
                         "the shadow index breaks a run exactly like the key does")

    def test_eligibility_needs_three_qualifying_rows_not_two(self) -> None:
        w, h, key = 20, 6, 5
        two_rows = bytearray(masked_sprite(w, h, key))
        # Break the run in every row past the second into thirds, each shorter than a probe.
        for y in range(2, h):
            two_rows[y * w + w // 3] = two_rows[y * w + 2 * w // 3] = key
        self.assertFalse(pack.masked_is_eligible(w, h, bytes(two_rows), key),
                         "two qualifying rows is not enough")
        three_rows = bytearray(masked_sprite(w, h, key))
        for y in range(3, h):
            three_rows[y * w + w // 3] = three_rows[y * w + 2 * w // 3] = key
        self.assertTrue(pack.masked_is_eligible(w, h, bytes(three_rows), key),
                        "three qualifying rows is exactly the rule")

    def test_eligibility_needs_the_minimum_size_too(self) -> None:
        w, h, key = pack.MASKED_MIN_WIDTH, pack.MASKED_MIN_HEIGHT, 5
        indices = masked_sprite(w, h, key, border=0)
        self.assertTrue(pack.masked_is_eligible(w, h, indices, key))
        self.assertFalse(pack.masked_is_eligible(w - 1, h, masked_sprite(w - 1, h, key, border=0), key))
        self.assertFalse(pack.masked_is_eligible(w, h - 1, masked_sprite(w, h - 1, key, border=0), key))

    def test_eligibility_refuses_more_than_the_pixel_cap(self) -> None:
        """A masked image's own w*h must not exceed MASKED_MAX_PIXELS, whatever its opaque rows."""
        side = 256                                     # 256 x 256 = 65,536, exactly the cap
        key = 250
        row = bytes([3] * side)
        at_cap = row * side
        self.assertEqual(side * side, pack.MASKED_MAX_PIXELS)
        self.assertTrue(pack.masked_is_eligible(side, side, at_cap, key))
        over_cap = row * (side + 1)
        self.assertFalse(pack.masked_is_eligible(side, side + 1, over_cap, key))

    # --- the DLL's probe-table capacity ---------------------------------------------------------

    def test_a_sprites_probe_cost_is_its_rows_times_bands_doubled_when_mirrored(self) -> None:
        """This sprite has 6 rows, all of them eligible: `min(6, MASKED_PROBE_ROWS_CAP) == 4` rows x
        3 bands x one colour rule = 12, and 24 when it is MIRROR (every slice also read backwards).
        A picture is always 6."""
        indices, rgba = self.sprite()
        plain = pack.encode_record("sprite__x", 20, 6, indices, PALETTE, 40, 12, rgba,
                                   flags=pack.FLAG_MASKED, key=5)
        mirrored = pack.encode_record("sprite__x", 20, 6, indices, PALETTE, 40, 12, rgba,
                                      flags=pack.FLAG_MASKED | pack.FLAG_MIRROR, key=5)
        picture_indices = bytes((x + y) % 256 for y in range(H) for x in range(W))
        picture_entry, picture_zidx, _ = pack.encode_record(
            "a", W, H, picture_indices, PALETTE, W * 2, H * 2,
            rgb_of(bytes((x + y) % 256 for y in range(H * 2) for x in range(W * 2))))
        self.assertEqual(pack.record_probe_slots(picture_entry, picture_zidx), pack.PICTURE_PROBE_SLOTS)
        self.assertEqual(pack.record_probe_slots(plain[0], plain[1]), 12)
        self.assertEqual(pack.record_probe_slots(mirrored[0], mirrored[1]), 24)

    def test_the_probe_boundary_is_each_sprites_own_cost(self) -> None:
        """Mirrored sprites with exactly three eligible rows cost 18 each (3 rows x 3 bands x 2), not
        a flat 24: MAX_PROBES // 18 of them are accepted and one more is refused."""
        w, h, key = 20, 4, 5
        good_row = bytes([key, key] + [3] * (w - 4) + [key, key])
        empty_row = bytes([key] * w)
        indices = good_row * 3 + empty_row * (h - 3)     # exactly 3 rows with a qualifying run
        self.assertEqual(pack.masked_opaque_rows(w, h, indices, key), 3)
        self.assertTrue(pack.masked_is_eligible(w, h, indices, key))

        rgba = bytes(w * 2 * h * 2 * 4)
        record = pack.encode_record("sprite__x", w, h, indices, PALETTE, w * 2, h * 2, rgba,
                                    flags=pack.FLAG_MASKED | pack.FLAG_MIRROR, key=key)
        self.assertEqual(pack.record_probe_slots(record[0], record[1]), 18)

        fits = pack.MAX_PROBES // 18
        out = pathlib.Path(self.tmp.name) / "fits.pack"
        pack.write_records(out, [record] * fits)
        self.assertEqual(pack.count(out), fits)
        with self.assertRaises(SystemExit):
            pack.write_records(pathlib.Path(self.tmp.name) / "too-many.pack", [record] * (fits + 1))

    # --- format 5: mirrored and grouped sprites -------------------------------------------------

    def test_a_picture_cannot_be_mirrored_or_grouped(self) -> None:
        indices = bytes((x + y) % 256 for y in range(H) for x in range(W))
        rgb = rgb_of(bytes((x + y) % 256 for y in range(H * 2) for x in range(W * 2)))
        with self.assertRaises(ValueError):
            pack.encode_record("a", W, H, indices, PALETTE, W * 2, H * 2, rgb, flags=pack.FLAG_MIRROR)
        with self.assertRaises(ValueError):
            pack.encode_record("a", W, H, indices, PALETTE, W * 2, H * 2, rgb, group=3)

    def test_a_group_must_fit_sixteen_bits(self) -> None:
        indices, rgba = self.sprite()
        pack.encode_record("s", 20, 6, indices, PALETTE, 40, 12, rgba, flags=pack.FLAG_MASKED, key=5,
                           group=pack.MAX_GROUP)
        with self.assertRaises(ValueError):
            pack.encode_record("s", 20, 6, indices, PALETTE, 40, 12, rgba, flags=pack.FLAG_MASKED,
                               key=5, group=pack.MAX_GROUP + 1)

    def test_flags_and_groups_survive_a_round_trip(self) -> None:
        indices, rgba = self.sprite()
        records = [pack.encode_record(f"g{i}", 20, 6, indices, PALETTE, 40, 12, rgba,
                                      flags=pack.FLAG_MASKED | pack.FLAG_MIRROR, key=5, group=7)
                   for i in range(3)]
        out = pathlib.Path(self.tmp.name) / "groups.pack"
        pack.write_records(out, records)
        back = pack.read(out.read_bytes())
        self.assertEqual([(r[0], r[3], r[4], r[5]) for r in back],
                         [(f"g{i}", pack.FLAG_MASKED | pack.FLAG_MIRROR, 5, 7) for i in range(3)])

    def test_a_pack_past_four_gigabytes_is_refused(self) -> None:
        """The DLL's offsets are 32-bit. Checked with the limit lowered: 4 GB of test data is not."""
        indices, rgba = self.sprite()
        record = pack.encode_record("s", 20, 6, indices, PALETTE, 40, 12, rgba, flags=pack.FLAG_MASKED, key=5)
        one = len(pack.MAGIC) + 4 + sum(len(part) for part in record)
        old = pack.MAX_PACK_BYTES
        self.addCleanup(setattr, pack, "MAX_PACK_BYTES", old)
        pack.MAX_PACK_BYTES = one
        pack.write_records(pathlib.Path(self.tmp.name) / "fits.pack", [record])
        pack.MAX_PACK_BYTES = one - 1
        with self.assertRaises(SystemExit):
            pack.write_records(pathlib.Path(self.tmp.name) / "big.pack", [record])

    def test_a_split_group_is_refused(self) -> None:
        """The DLL refuses a group whose records are not consecutive; so does the writer."""
        indices, rgba = self.sprite()
        def rec(name, group):
            return pack.encode_record(name, 20, 6, indices, PALETTE, 40, 12, rgba,
                                      flags=pack.FLAG_MASKED, key=5, group=group)
        ok = pathlib.Path(self.tmp.name) / "ok.pack"
        pack.write_records(ok, [rec("a", 1), rec("b", 1), rec("c", 0), rec("d", 2)])
        with self.assertRaises(SystemExit):
            pack.write_records(pathlib.Path(self.tmp.name) / "split.pack",
                               [rec("a", 1), rec("b", 2), rec("c", 1)])
        with self.assertRaises(SystemExit):
            pack.write_records(pathlib.Path(self.tmp.name) / "split0.pack",
                               [rec("a", 1), rec("b", 0), rec("c", 1)])


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



class ContentCheck(unittest.TestCase):
    """An upscale of the right size that is not a plausible 2x of its original is made again once
    (`rerender`), then left out if it still looks damaged."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        self.small, self.large = root / "small", root / "large"
        self.small.mkdir(); self.large.mkdir()
        write_lbm(self.small / "aagtwr0a.lbm", W, H, 1)
        write_upscale(self.large / "clean.lbm", W, H, 7)
        write_lbm(self.small / "clean.lbm", W, H, 7)
        self.bad = self.large / "aagtwr0a.lbm"
        write_lbm(self.bad, W * 2, H * 2, 1)          # the right size, but not this picture at 2x
        self.calls: list[str] = []

    def records(self, rerender):
        skipped: list[str] = []
        counts: dict = {}
        got = list(pack.unmasked_records(self.small, [self.large], skipped, rerender=rerender, counts=counts))
        return got, skipped, counts

    def test_the_damage_is_real(self) -> None:
        _, _, idx, pal, _ = lbm_png.decode(self.small / "aagtwr0a.lbm")
        _, _, big, _, _ = lbm_png.decode(self.bad)
        score = hd_upscale.damage_score(W, H, rgb_of(idx), rgb_of(big), 3, 3)
        self.assertTrue(hd_upscale.looks_damaged(score), score)

    def test_a_damaged_upscale_made_again_clean_is_packed(self) -> None:
        def remake(path):
            self.calls.append(path.name)
            write_upscale(path, W, H, 1)

        records, skipped, counts = self.records(remake)
        self.assertEqual(len(records), 2)
        self.assertEqual(skipped, [])
        self.assertEqual(self.calls, ["aagtwr0a.lbm"], "only the damaged one, once")
        self.assertEqual((counts["damaged"], counts["remade"]), (1, 1))

    def test_one_still_damaged_is_left_out(self) -> None:
        records, skipped, counts = self.records(lambda path: self.calls.append(path.name))
        self.assertEqual(len(records), 1)
        self.assertEqual(len(skipped), 1)
        self.assertTrue(skipped[0].startswith("aagtwr0a: upscale looked damaged ("), skipped)
        self.assertIn("made twice); the original shows", skipped[0])
        self.assertEqual((counts["damaged"], counts["remade"]), (1, 0))

    def test_without_an_upscaler_it_is_left_out_at_once(self) -> None:
        records, skipped, _ = self.records(None)
        self.assertEqual(len(records), 1)
        self.assertNotIn("made twice", skipped[0])

    def test_an_upscaler_that_fails_the_second_time_leaves_it_out(self) -> None:
        def failing(path):
            path.unlink()
            raise SystemExit("the upscaler failed")

        records, skipped, _ = self.records(failing)
        self.assertEqual(len(records), 1)
        self.assertIn("upscale looked damaged", skipped[0])

    def test_a_malformed_retry_leaves_that_picture_out_not_the_pack(self) -> None:
        def garbage(path):
            path.write_bytes(b"not an image at all")

        records, skipped, counts = self.records(garbage)
        self.assertEqual(len(records), 1, "the other picture is still packed")
        self.assertEqual(len(skipped), 1)
        self.assertIn("upscale looked damaged", skipped[0])
        self.assertIn("could not be made again", skipped[0])
        self.assertEqual((counts["damaged"], counts["remade"], counts["failed"]), (1, 0, 1))

if __name__ == "__main__":
    unittest.main()
