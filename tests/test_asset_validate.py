"""Image content validation, against images assembled chunk by chunk in memory.

No game content is committed or read here. Every fixture is built by `image()` below from the
pieces a real member has, so each refusal can be provoked one at a time by changing exactly one
field -- which is the only way to know a check fires for the reason it claims.

The numbers the fixtures use were measured from the shipped corpus, not chosen: 8 planes, masking
0, compression 1, a 768-byte CMAP, rows padded to an even byte count. See
`tools/asset_validate.py` for the survey.
"""

import struct
import unittest

from tools.asset_validate import (
    BMHD_LENGTH,
    ERROR,
    WARNING,
    decode_body,
    is_image_member,
    validate_image,
)

PALETTE_256 = bytes(range(256)) * 3

#: Two rows of four distinct, in-palette pixels: the default image body.
TWO_ROWS = [bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])]


def chunk(chunk_id: bytes, payload: bytes) -> bytes:
    """One IFF chunk, padded to an even size the way the format requires."""
    pad = b"\x00" if len(payload) % 2 else b""
    return chunk_id + struct.pack(">I", len(payload)) + payload + pad


def bmhd(
    width: int = 4,
    height: int = 2,
    *,
    planes: int = 8,
    masking: int = 0,
    compression: int = 1,
    transparent: int = 0,
) -> bytes:
    return chunk(
        b"BMHD",
        struct.pack(">HH", width, height)
        + struct.pack(">HH", 0, 0)
        + bytes([planes, masking, compression, 0])
        + struct.pack(">H", transparent)
        + bytes([1, 1])
        + struct.pack(">HH", width, height),
    )


def byte_run1(rows: list[bytes]) -> bytes:
    """Encode each row as one literal packet, which is what a row of distinct pixels becomes."""
    return b"".join(bytes([len(row) - 1]) + row for row in rows)


def image(
    *,
    header: bytes | None = None,
    palette: bytes | None = PALETTE_256,
    body: bytes | None = None,
    form_id: bytes = b"PBM ",
    form_size_delta: int = 0,
    trailing: bytes = b"",
) -> bytes:
    if header is None:
        header = bmhd()
    if body is None:
        body = byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])])
    payload = form_id + header
    if palette is not None:
        payload += chunk(b"CMAP", palette)
    payload += chunk(b"BODY", body)
    return b"FORM" + struct.pack(">I", len(payload) + form_size_delta) + payload + trailing


class ImageFixtureTest(unittest.TestCase):
    def assert_finding(self, data: bytes, severity: str, check: str) -> str:
        findings, _ = validate_image(data)
        matching = [
            finding
            for finding in findings
            if finding.severity == severity and finding.check == check
        ]
        self.assertTrue(
            matching, f"expected a {severity} on {check}; got {[str(f) for f in findings]}"
        )
        return matching[0].message

    def assert_clean(self, data: bytes) -> None:
        findings, summary = validate_image(data)
        self.assertEqual([str(finding) for finding in findings], [])
        self.assertIsNotNone(summary)


class WellFormedImageTest(ImageFixtureTest):
    def test_a_well_formed_image_produces_no_finding_at_all(self) -> None:
        self.assert_clean(image())

    def test_the_summary_reports_what_was_checked(self) -> None:
        _, summary = validate_image(image())
        self.assertEqual((summary.width, summary.height), (4, 2))
        self.assertEqual(summary.palette_entries, 256)
        self.assertEqual(summary.pixels_checked, 8)

    def test_an_odd_width_image_pads_its_rows_to_an_even_byte_count(self) -> None:
        """88 of the 1,045 base images are odd-width; every one decodes only under this rule."""
        self.assert_clean(
            image(
                header=bmhd(width=3, height=2),
                body=byte_run1([bytes([1, 2, 3, 0]), bytes([4, 5, 6, 0])]),
            )
        )

    def test_an_odd_width_image_refuses_rows_of_the_unpadded_width(self) -> None:
        """The counter-case: three-byte rows leave the second row starved, which must be seen."""
        self.assert_finding(
            image(
                header=bmhd(width=3, height=2),
                body=byte_run1([bytes([1, 2, 3]), bytes([4, 5, 6])]),
            ),
            ERROR,
            "body",
        )

    def test_an_uncompressed_body_is_read_as_padded_rows(self) -> None:
        self.assert_clean(
            image(
                header=bmhd(width=3, height=2, compression=0),
                body=bytes([1, 2, 3, 0, 4, 5, 6, 0]),
            )
        )

    def test_a_single_trailing_zero_on_an_even_sized_body_is_accepted_silently(self) -> None:
        """65 base members carry exactly this pad byte inside the declared BODY size.

        The BODY must come out EVEN-sized for the pad explanation to apply, so the fixture uses
        three rows: 3 x 5 = 15 packet bytes, plus the pad byte = 16. An earlier version of this
        test used two rows, which made the declared BODY 11 bytes -- odd -- so it was asserting
        that the loose behaviour was correct rather than that the measured rule was implemented.
        """
        rows = [bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8]), bytes([1, 2, 3, 4])]
        body = byte_run1(rows) + b"\x00"
        self.assertEqual(len(body) % 2, 0, "the fixture must be even-sized or it proves nothing")
        self.assert_clean(image(header=bmhd(width=4, height=3), body=body))

    def test_a_single_trailing_zero_on_an_odd_sized_body_is_reported(self) -> None:
        """The counter-case. Without it, loosening the rule to any parity passes the suite."""
        body = byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])]) + b"\x00"
        self.assertEqual(len(body) % 2, 1, "the fixture must be odd-sized or it proves nothing")
        findings, summary = validate_image(image(body=body))
        self.assertEqual([(f.severity, f.check) for f in findings], [(WARNING, "body")])
        self.assertIsNotNone(summary, "an unexplained pad byte is not fatal")

    def test_the_no_op_packet_writes_nothing(self) -> None:
        rows = byte_run1([bytes([1, 2, 3, 4])]), byte_run1([bytes([5, 6, 7, 8])])
        body = b"\x80" + rows[0] + b"\x80" + rows[1]
        self.assert_clean(image(body=body))

    def test_a_repeat_packet_fills_the_row(self) -> None:
        self.assert_clean(image(body=(b"\xfd\x07" * 2)))


class StructureRefusalTest(ImageFixtureTest):
    def test_a_file_that_is_not_an_iff_form_is_refused(self) -> None:
        message = self.assert_finding(b"\x89PNG\r\n\x1a\n" + b"\x00" * 32, ERROR, "iff-structure")
        self.assertIn("FORM", message)

    def test_a_form_that_is_not_a_pbm_is_refused(self) -> None:
        self.assert_finding(image(form_id=b"ILBM"), ERROR, "iff-structure")

    def test_a_file_too_short_to_hold_a_form_header_is_refused(self) -> None:
        self.assert_finding(b"FORM\x00\x00\x00\x04", ERROR, "iff-structure")

    def test_a_form_larger_than_the_file_is_refused(self) -> None:
        message = self.assert_finding(image(form_size_delta=1), ERROR, "iff-structure")
        self.assertIn("bytes", message)

    def test_bytes_after_the_end_of_the_form_are_reported(self) -> None:
        """All 1,045 base FORMs end exactly at the end of the file, so this is never normal."""
        self.assert_finding(image(trailing=b"\x00\x00"), WARNING, "iff-structure")

    def test_a_chunk_declaring_more_than_the_form_holds_is_refused(self) -> None:
        body = byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])])
        payload = b"PBM " + bmhd() + chunk(b"CMAP", PALETTE_256)
        payload += b"BODY" + struct.pack(">I", len(body) + 2) + body
        data = b"FORM" + struct.pack(">I", len(payload)) + payload
        message = self.assert_finding(data, ERROR, "iff-structure")
        self.assertIn("past the end of the FORM", message)

    def test_a_chunk_walk_that_stops_short_of_the_form_end_is_refused(self) -> None:
        """Three bytes left over cannot start a chunk header, so the walk ends before the FORM."""
        message = self.assert_finding(_with_extra_form_bytes(3), ERROR, "iff-structure")
        self.assertIn("belong to no chunk", message)


def _with_extra_form_bytes(extra: int) -> bytes:
    """An image whose FORM claims `extra` bytes more than its chunks fill, inside the file."""
    payload = (
        b"PBM "
        + bmhd()
        + chunk(b"CMAP", PALETTE_256)
        + chunk(b"BODY", byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])]))
    )
    return b"FORM" + struct.pack(">I", len(payload) + extra) + payload + b"\x00" * extra


class HeaderRefusalTest(ImageFixtureTest):
    def test_a_missing_bmhd_is_refused(self) -> None:
        payload = b"PBM " + chunk(b"CMAP", PALETTE_256) + chunk(b"BODY", b"\x00")
        data = b"FORM" + struct.pack(">I", len(payload)) + payload
        self.assert_finding(data, ERROR, "bmhd")

    def test_a_short_bmhd_is_refused(self) -> None:
        data = image(header=chunk(b"BMHD", b"\x00" * 18))
        message = self.assert_finding(data, ERROR, "bmhd")
        self.assertIn("20", message)

    def test_a_zero_width_is_refused(self) -> None:
        message = self.assert_finding(image(header=bmhd(width=0, height=2)), ERROR, "bmhd")
        self.assertIn("0x2", message)

    def test_a_zero_height_is_refused(self) -> None:
        self.assert_finding(image(header=bmhd(width=4, height=0)), ERROR, "bmhd")

    def test_a_plane_count_this_pipeline_cannot_read_is_refused(self) -> None:
        message = self.assert_finding(image(header=bmhd(planes=4)), ERROR, "bmhd")
        self.assertIn("4 plane", message)

    def test_eight_planes_is_accepted(self) -> None:
        self.assert_clean(image(header=bmhd(planes=8)))

    def test_a_compression_with_no_decoder_is_refused(self) -> None:
        message = self.assert_finding(image(header=bmhd(compression=2)), ERROR, "bmhd")
        self.assertIn("compression 2", message)

    def test_a_masking_mode_the_decoder_ignores_is_a_warning(self) -> None:
        self.assert_finding(image(header=bmhd(masking=1)), WARNING, "bmhd")

    def test_masking_two_is_not_warned_about(self) -> None:
        self.assert_clean(image(header=bmhd(masking=2)))


class PaletteRefusalTest(ImageFixtureTest):
    def test_a_missing_cmap_is_refused(self) -> None:
        message = self.assert_finding(image(palette=None), ERROR, "cmap")
        self.assertIn("CMAP", message)

    def test_a_cmap_that_is_not_whole_rgb_triples_is_refused(self) -> None:
        message = self.assert_finding(image(palette=PALETTE_256 + b"\x00"), ERROR, "cmap")
        self.assertIn("triples", message)

    def test_an_empty_cmap_is_refused(self) -> None:
        self.assert_finding(image(palette=b""), ERROR, "cmap")

    def test_a_cmap_smaller_than_the_planes_can_address_is_a_warning(self) -> None:
        message = self.assert_finding(
            image(palette=bytes(9 * 3), body=byte_run1(TWO_ROWS)),
            WARNING,
            "cmap",
        )
        self.assertIn("9 colour", message)

    def test_a_full_256_entry_cmap_is_not_warned_about(self) -> None:
        self.assert_clean(image(palette=PALETTE_256))

    def test_a_pixel_index_past_the_end_of_the_palette_is_refused(self) -> None:
        """The palette check itself: 8 colours, and a pixel asking for the ninth."""
        message = self.assert_finding(
            image(palette=bytes(8 * 3), body=byte_run1(TWO_ROWS)),
            ERROR,
            "palette-index",
        )
        self.assertIn("column 3 row 1", message)
        self.assertIn("index 8", message)

    def test_the_last_index_the_palette_holds_is_accepted(self) -> None:
        """The other direction: index 7 of an 8-colour palette must not be reported."""
        findings, _ = validate_image(
            image(palette=bytes(8 * 3), body=byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 7])]))
        )
        self.assertEqual([f for f in findings if f.check == "palette-index"], [])

    def test_every_out_of_range_index_is_counted_not_just_the_first(self) -> None:
        message = self.assert_finding(
            image(
                palette=bytes(4 * 3),
                body=byte_run1([bytes([0, 1, 9, 4]), bytes([9, 0, 1, 2])]),
            ),
            ERROR,
            "palette-index",
        )
        self.assertIn("3 pixel(s)", message)
        self.assertIn("9 (2 pixel(s))", message)

    def test_the_row_pad_column_is_not_treated_as_a_pixel(self) -> None:
        """An odd-width row's pad byte is decoded but never drawn, so it cannot be a defect."""
        findings, summary = validate_image(
            image(
                header=bmhd(width=3, height=2),
                palette=bytes(8 * 3),
                body=byte_run1([bytes([1, 2, 3, 200]), bytes([4, 5, 6, 200])]),
            )
        )
        self.assertEqual([f for f in findings if f.check == "palette-index"], [])
        self.assertEqual(summary.pixels_checked, 6)

    def test_a_transparent_index_outside_the_palette_is_refused_when_masking_uses_it(self) -> None:
        message = self.assert_finding(
            image(header=bmhd(masking=2, transparent=8), palette=bytes(8 * 3)),
            ERROR,
            "bmhd",
        )
        self.assertIn("transparent index 8", message)

    def test_a_transparent_index_inside_the_palette_is_accepted(self) -> None:
        findings, _ = validate_image(
            image(header=bmhd(masking=2, transparent=7), palette=bytes(8 * 3))
        )
        self.assertEqual([f for f in findings if "transparent" in f.message], [])

    def test_an_unused_transparent_index_outside_the_palette_is_only_a_warning(self) -> None:
        """Masking 0 means the decoder never reads the field; 528 base members set it to 255."""
        message = self.assert_finding(
            image(header=bmhd(masking=0, transparent=9), palette=bytes(8 * 3)),
            WARNING,
            "bmhd",
        )
        self.assertIn("does not use it", message)


class BodyRefusalTest(ImageFixtureTest):
    def test_a_missing_body_is_refused(self) -> None:
        payload = b"PBM " + bmhd() + chunk(b"CMAP", PALETTE_256)
        data = b"FORM" + struct.pack(">I", len(payload)) + payload
        self.assert_finding(data, ERROR, "body")

    def test_a_body_that_ends_before_the_image_is_full_is_refused(self) -> None:
        message = self.assert_finding(
            image(body=byte_run1([bytes([1, 2, 3, 4])])), ERROR, "body"
        )
        self.assertIn("row 1 of 2", message)

    def test_a_body_that_ends_mid_row_is_refused(self) -> None:
        message = self.assert_finding(
            image(body=byte_run1([bytes([1, 2, 3, 4])]) + b"\x01\x05\x06"), ERROR, "body"
        )
        self.assertIn("row 1 of 2", message)
        self.assertIn("2 of 4 byte(s)", message)

    def test_a_literal_packet_claiming_bytes_the_body_does_not_hold_is_refused(self) -> None:
        message = self.assert_finding(image(body=b"\x03\x01\x02"), ERROR, "body")
        self.assertIn("runs past the end", message)

    def test_a_literal_packet_claiming_one_byte_past_the_body_is_refused(self) -> None:
        """The boundary: a packet whose last byte is the first byte the BODY does not have."""
        message = self.assert_finding(image(body=b"\x03\x01\x02\x03"), ERROR, "body")
        self.assertIn("runs past the end", message)

    def test_a_repeat_packet_with_no_value_byte_is_refused(self) -> None:
        message = self.assert_finding(image(body=b"\xfd"), ERROR, "body")
        self.assertIn("no value byte", message)

    def test_an_uncompressed_body_shorter_than_the_image_is_refused(self) -> None:
        message = self.assert_finding(
            image(header=bmhd(compression=0), body=bytes(7)), ERROR, "body"
        )
        self.assertIn("need 8", message)

    def test_a_packet_crossing_a_scanline_is_a_warning_because_a_shipped_member_does_it(
        self,
    ) -> None:
        """GS5R3's PORTRAIT/decr5p00.lbm crosses 30 times; refusing would reject shipped art."""
        data = image(body=b"\x05\x01\x02\x03\x04\x05\x06" + byte_run1([bytes([5, 6, 7, 8])]))
        message = self.assert_finding(data, WARNING, "body")
        self.assertIn("cross a scanline boundary", message)
        self.assertIn("2 pixel(s) are discarded", message)
        findings, summary = validate_image(data)
        self.assertEqual([f for f in findings if f.severity == ERROR], [])
        self.assertIsNotNone(summary)

    def test_more_than_one_trailing_body_byte_is_reported(self) -> None:
        message = self.assert_finding(
            image(body=byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])]) + b"\x00\x00"),
            WARNING,
            "body",
        )
        self.assertIn("2 byte(s)", message)

    def test_a_single_nonzero_trailing_body_byte_is_reported(self) -> None:
        """One byte is only the even-size pad when it is zero; anything else is unread data."""
        self.assert_finding(
            image(body=byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])]) + b"\x01"),
            WARNING,
            "body",
        )


class BoundaryTest(ImageFixtureTest):
    """The cases one byte either side of a bound, which a one-direction sweep never reaches."""

    def test_a_form_pbm_with_no_chunks_is_refused_for_its_header_not_its_magic(self) -> None:
        """Exactly 12 bytes: the magic is intact, so the complaint must be the missing BMHD."""
        data = b"FORM" + struct.pack(">I", 4) + b"PBM "
        self.assertEqual(len(data), 12)
        self.assert_finding(data, ERROR, "bmhd")

    def test_a_form_whose_fourth_magic_byte_differs_is_refused(self) -> None:
        self.assert_finding(b"FORX" + struct.pack(">I", 4) + b"PBM ", ERROR, "iff-structure")

    def test_a_form_type_whose_fourth_byte_is_not_a_space_is_refused(self) -> None:
        """`PBMX` shares three bytes with `PBM ` and is not the type this reader handles."""
        self.assert_finding(image(form_id=b"PBMX"), ERROR, "iff-structure")

    def test_a_single_byte_after_the_form_is_reported(self) -> None:
        self.assert_finding(image(trailing=b"\x00"), WARNING, "iff-structure")

    def test_a_chunk_overrunning_the_form_by_one_byte_is_refused(self) -> None:
        body = byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])])
        payload = b"PBM " + bmhd() + chunk(b"CMAP", PALETTE_256)
        payload += b"BODY" + struct.pack(">I", len(body) + 1) + body
        data = b"FORM" + struct.pack(">I", len(payload)) + payload
        message = self.assert_finding(data, ERROR, "iff-structure")
        self.assertIn("runs 1 byte(s) past", message)

    def test_seven_bytes_too_few_to_start_a_chunk_are_refused_not_ignored(self) -> None:
        """Seven bytes cannot hold a chunk header, and must not be read as though they could."""
        message = self.assert_finding(_with_extra_form_bytes(7), ERROR, "iff-structure")
        self.assertIn("7 byte(s) belong to no chunk", message)

    def test_a_zero_length_chunk_is_walked_rather_than_left_over(self) -> None:
        """Eight bytes at the end are a real empty chunk; base members carry TINY and DPPS."""
        payload = (
            b"PBM "
            + bmhd()
            + chunk(b"CMAP", PALETTE_256)
            + chunk(b"BODY", byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])]))
            + chunk(b"TINY", b"")
        )
        self.assert_clean(b"FORM" + struct.pack(">I", len(payload)) + payload)

    def test_a_bmhd_one_byte_short_of_the_header_is_refused(self) -> None:
        """A valid header with its last byte cut off: the length is the only thing wrong."""
        truncated = bmhd()[8 : 8 + BMHD_LENGTH - 1]
        message = self.assert_finding(image(header=chunk(b"BMHD", truncated)), ERROR, "bmhd")
        self.assertIn(f"is {BMHD_LENGTH - 1} bytes", message)

    def test_more_planes_than_this_pipeline_reads_is_refused_too(self) -> None:
        """Not only fewer: a 16-plane image is equally undecodable here."""
        self.assert_finding(image(header=bmhd(planes=16)), ERROR, "bmhd")

    def test_a_cmap_that_is_an_even_but_not_a_triple_size_is_refused(self) -> None:
        message = self.assert_finding(image(palette=PALETTE_256 + b"\x00\x00"), ERROR, "cmap")
        self.assertIn("2 byte(s) over", message)

    def test_a_cmap_that_is_a_multiple_of_four_but_not_of_three_is_refused(self) -> None:
        self.assert_finding(image(palette=b"\x00" * 4), ERROR, "cmap")

    def test_a_128_byte_literal_packet_fills_a_128_byte_row(self) -> None:
        """Control byte 127 is the largest literal; 128 is the no-op, and they are not the same."""
        row = bytes(range(128))
        self.assert_clean(
            image(header=bmhd(width=128, height=1), body=b"\x7f" + row)
        )

    def test_a_repeat_packet_crossing_a_scanline_reports_what_it_discarded(self) -> None:
        """The literal case is not enough: the repeat branch clamps through separate code."""
        message = self.assert_finding(
            image(header=bmhd(width=4, height=1), body=b"\xfa\x07"), WARNING, "body"
        )
        self.assertIn("3 pixel(s) are discarded", message)

    def test_a_packet_crossing_a_scanline_by_exactly_one_byte_is_reported(self) -> None:
        data = image(
            header=bmhd(width=4, height=1), body=b"\x04\x01\x02\x03\x04\x05"
        )
        message = self.assert_finding(data, WARNING, "body")
        self.assertIn("1 pixel(s) are discarded", message)


class DecodeBodyTest(unittest.TestCase):
    def test_clamping_reports_what_it_discarded(self) -> None:
        pixels, consumed, crossings = decode_body(b"\x05\x01\x02\x03\x04\x05\x06", 4, 1, 1)
        self.assertEqual(pixels, bytes([1, 2, 3, 4]))
        self.assertEqual(consumed, 7)
        self.assertEqual((crossings[0].row, crossings[0].count, crossings[0].discarded), (0, 6, 2))

    def test_a_packet_that_exactly_fills_the_row_is_not_a_crossing(self) -> None:
        _, _, crossings = decode_body(b"\x03\x01\x02\x03\x04", 4, 1, 1)
        self.assertEqual(crossings, [])


class MemberNameTest(unittest.TestCase):
    def test_both_spellings_the_archive_uses_are_recognised(self) -> None:
        """Vanilla pic.mpq holds 1,033 `.lbm` and 11 `.LBM` members."""
        self.assertTrue(is_image_member("LBM\\ACTIONS.lbm"))
        self.assertTrue(is_image_member("LBM\\ACTIONS.LBM"))

    def test_a_member_that_is_not_an_image_is_not_claimed(self) -> None:
        self.assertFalse(is_image_member("til\\jeff01.til"))
        self.assertFalse(is_image_member("units\\orinf.gs"))
        self.assertFalse(is_image_member("File00001070.xxx"))


if __name__ == "__main__":
    unittest.main()


class ReviewFindingsTest(unittest.TestCase):
    """Regressions for the defects a Claude/Codex cross-model review found on 2026-09-18.

    Each of these passed before the fix, which is the only reason they are worth having.
    """

    def test_a_body_that_cannot_possibly_expand_that_far_is_refused_before_allocating(self) -> None:
        # The BMHD is attacker- and accident-controlled. Before the fix this allocated
        # width*height bytes from it -- 4.3 GB for 65535x65535 -- and a MemoryError escaped
        # validate_image entirely, aborting the build gate with a traceback naming no member.
        data = image(header=bmhd(width=65535, height=65535), body=b"\x00\x01")
        findings, summary = validate_image(data)
        self.assertIsNone(summary, "the member must not decode")
        self.assertIn(
            "64x", "  ".join(f.message for f in findings), "the refusal cites the format's bound"
        )
        self.assertEqual([f.severity for f in findings], [ERROR])

    def test_the_last_of_a_duplicated_chunk_wins_matching_the_rust_decoder(self) -> None:
        # spikes/asset-viewer/src/pbm.rs assigns `body = Some(..)` on every match, so the LAST
        # BODY is what it renders. With setdefault this validator read the FIRST, so a member
        # could pass here and still fail to decode -- the validator checking different bytes
        # than the decoder is the one thing it may never do.
        #
        # The clean BODY's indices must all be INSIDE the palette, or both copies raise
        # palette-index and the test cannot tell first-wins from last-wins.
        clean = byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 0])])
        poisoned = byte_run1([bytes([200, 201, 202, 203]), bytes([204, 205, 206, 207])])
        palette = bytes(8 * 3)
        payload = b"PBM " + bmhd() + _chunk(b"CMAP", palette) + _chunk(b"BODY", clean)
        payload += _chunk(b"BODY", poisoned)
        data = b"FORM" + struct.pack(">I", len(payload)) + payload
        findings, _ = validate_image(data)
        checks = [f.check for f in findings]
        self.assertIn("palette-index", checks, "the LAST BODY's indices must be the ones checked")
        self.assertIn("iff-structure", checks, "the duplicate is reported in its own right")

        # The control: with ONLY the clean BODY there is no palette-index finding at all, which is
        # what makes the assertion above evidence about which copy was read.
        control = b"PBM " + bmhd() + _chunk(b"CMAP", palette) + _chunk(b"BODY", clean)
        control = b"FORM" + struct.pack(">I", len(control)) + control
        control_findings, _ = validate_image(control)
        self.assertNotIn("palette-index", [f.check for f in control_findings])

    def test_the_expansion_bound_is_exactly_64x_at_the_boundary(self) -> None:
        # A bound of 63x or 65x passes a test that only uses a wildly oversized header, so the
        # boundary is pinned from both sides with a body whose 64x limit is an exact pixel count.
        # Both sides fail to decode -- 16 zero bytes are not a real ByteRun1 stream -- so the
        # discriminator is WHICH refusal fires, not whether one does. At the limit the bound must
        # let the decode be attempted and the decoder must be the one to complain; past it, the
        # bound must refuse before any allocation happens.
        body = bytes(16)  # 16 bytes -> at most 1024 output bytes at the format's 64x ceiling
        at_limit = validate_image(image(header=bmhd(width=32, height=32), body=body))[0]
        past_limit = validate_image(image(header=bmhd(width=32, height=33), body=body))[0]
        self.assertNotIn(
            "64x", "  ".join(f.message for f in at_limit),
            "32x32 = 1024 pixels from 16 bytes is exactly 64x, so the bound must not fire",
        )
        self.assertIn(
            "64x", "  ".join(f.message for f in past_limit),
            "32x33 = 1056 pixels from 16 bytes exceeds 64x, so the bound must fire",
        )
        # Pins the bound from ABOVE as well. 40x26 = 1040 is exactly 65x of 16 bytes, so a bound
        # loosened by one multiple would let it through while the format cannot produce it.
        just_over = validate_image(image(header=bmhd(width=40, height=26), body=body))[0]
        self.assertIn(
            "64x", "  ".join(f.message for f in just_over),
            "1040 pixels from 16 bytes is 65x, which ByteRun1 cannot produce",
        )

    def test_pixels_the_palette_check_refuted_are_not_counted_as_confirmed(self) -> None:
        # The coverage block is this pipeline's honesty mechanism. A counter that reports refuted
        # pixels as "confirmed to address a colour" inverts exactly what it exists to say.
        palette = bytes(4 * 3)  # 4 colours, so indices 4..7 are out of range
        body = byte_run1([bytes([0, 1, 2, 3]), bytes([4, 5, 6, 7])])
        data = image(palette=palette, body=body)
        findings, summary = validate_image(data)
        self.assertIn("palette-index", [f.check for f in findings])
        self.assertIsNotNone(summary)
        self.assertEqual(
            summary.pixels_checked, 4,
            "8 pixels, 4 of them refuted; only the 4 in-range ones were confirmed",
        )

    def test_a_fully_in_palette_image_counts_every_pixel(self) -> None:
        """The counter-case, so the subtraction cannot be a constant that happens to fit."""
        _, summary = validate_image(image(body=byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])])))
        self.assertIsNotNone(summary)
        self.assertEqual(summary.pixels_checked, 8)

    def test_repeated_crng_chunks_are_normal_and_draw_nothing(self) -> None:
        """CRNG is a list. Measured: up to 16 per member, and >1 in 3,079 of 3,463 corpus images.

        An earlier version of the duplicate check warned on ANY repeated chunk and fired on 916 of
        vanilla's 1,044 members -- a validator rejecting shipped content, which is the exact failure
        this module's docstring opens by warning about.
        """
        payload = b"PBM " + bmhd() + _chunk(b"CMAP", PALETTE_256)
        for _ in range(16):
            payload += _chunk(b"CRNG", bytes(8))
        payload += _chunk(b"BODY", byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])]))
        data = b"FORM" + struct.pack(">I", len(payload)) + payload
        findings, summary = validate_image(data)
        self.assertEqual([str(f) for f in findings], [])
        self.assertIsNotNone(summary)

    def test_a_repeated_singleton_chunk_is_still_reported(self) -> None:
        """The counter-case: exempting CRNG must not exempt everything."""
        payload = b"PBM " + bmhd() + _chunk(b"CMAP", PALETTE_256) + _chunk(b"DPPS", bytes(4))
        payload += _chunk(b"DPPS", bytes(4))
        payload += _chunk(b"BODY", byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])]))
        data = b"FORM" + struct.pack(">I", len(payload)) + payload
        findings, _ = validate_image(data)
        self.assertEqual([(f.severity, f.check) for f in findings], [(WARNING, "iff-structure")])

    def test_a_chunk_walk_that_overruns_does_not_report_a_negative_count(self) -> None:
        # The overrun and stop-short branches shared one message, which printed
        # "-1 byte(s) belong to no chunk" -- a count that cannot exist.
        payload = b"PBM " + bmhd() + _chunk(b"CMAP", PALETTE_256)
        body = byte_run1([bytes([1, 2, 3, 4]), bytes([5, 6, 7, 8])]) + b"\x00"
        payload += b"BODY" + struct.pack(">I", len(body)) + body  # odd size, pad outside the FORM
        data = b"FORM" + struct.pack(">I", len(payload)) + payload + b"\x00"
        findings, _ = validate_image(data)
        message = "  ".join(f.message for f in findings)
        self.assertNotIn("-1 byte", message)
        self.assertNotIn("-", message.split("byte(s)")[0][-3:], "no negative count anywhere")


def _chunk(identifier: bytes, body: bytes) -> bytes:
    return identifier + struct.pack(">I", len(body)) + body + (b"\x00" if len(body) & 1 else b"")
