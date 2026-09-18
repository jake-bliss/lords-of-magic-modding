import io
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

from tools.mpq_shape import Member, compare, main, read_manifest

# Literal digests, so an assertion can never be satisfied by the same expression
# that built the fixture.
ALPHA_SHA = "8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8"
BRAVO_SHA = "f3233097bacb2526cb78cb72d2e1bbeb8e3675abac47990f678a505f3ad6daa7"
EMPTY_SHA = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"

MANIFEST_HEADER = (
    "path\tblock_index\thash_index\tsize\tcompressed_size\tflags\tlocale\tsha256\n"
)


def member(
    path: str,
    sha256: str,
    *,
    size: int = 5,
    block_index: int = 0,
    hash_index: int = 0,
    compressed_size: int = 13,
    flags: str = "0x80000100",
    locale: int = 0,
) -> Member:
    return Member(
        path=path,
        block_index=block_index,
        hash_index=hash_index,
        size=size,
        compressed_size=compressed_size,
        flags=flags,
        locale=locale,
        sha256=sha256,
    )


def kinds(report) -> list[str]:
    return [finding.kind for finding in report.findings]


def failure_kinds(report) -> list[str]:
    return [finding.kind for finding in report.failures]


class ShapeCheckTest(unittest.TestCase):
    def test_identical_archives_prove_every_member_unchanged(self) -> None:
        members = [member("START.GS", ALPHA_SHA), member("gs\\a.gs", BRAVO_SHA)]
        report = compare(members, list(members))

        self.assertTrue(report.ok)
        self.assertEqual(report.findings, [])
        self.assertEqual(report.unchanged_members, 2)
        self.assertEqual(report.source_entries, 2)
        self.assertEqual(report.output_entries, 2)

    def test_declared_replacement_is_reported_and_not_counted_unchanged(self) -> None:
        report = compare(
            [member("START.GS", ALPHA_SHA), member("keep.gs", BRAVO_SHA)],
            [member("START.GS", BRAVO_SHA), member("keep.gs", BRAVO_SHA)],
            {"START.GS"},
        )

        self.assertTrue(report.ok)
        self.assertEqual(kinds(report), ["member_changed"])
        self.assertEqual(report.unchanged_members, 1)
        # Written out rather than sliced from the fixture digests, so the
        # assertion cannot move when the reporting width does.
        self.assertEqual(
            report.findings[0].detail,
            "8ed3f6ad685b959e 5B 0x80000100 -> f3233097bacb2526 5B 0x80000100",
        )

    def test_declared_replacement_may_not_change_how_a_member_is_stored(self) -> None:
        report = compare(
            [member("START.GS", ALPHA_SHA, flags="0x80000100")],
            [member("START.GS", BRAVO_SHA, flags="0x80000000")],
            {"START.GS"},
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["declared_change_altered_storage"])

    def test_declared_replacement_may_not_change_a_member_locale(self) -> None:
        report = compare(
            [member("START.GS", ALPHA_SHA, locale=0)],
            [member("START.GS", BRAVO_SHA, locale=1033)],
            {"START.GS"},
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["declared_change_altered_storage"])

    def test_declared_replacement_that_changed_nothing_is_refused(self) -> None:
        report = compare(
            [member("START.GS", ALPHA_SHA)],
            [member("START.GS", ALPHA_SHA)],
            {"START.GS"},
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["declared_change_not_applied"])

    def test_declaring_a_member_the_source_lacks_is_refused(self) -> None:
        report = compare(
            [member("START.GS", ALPHA_SHA)],
            [member("START.GS", ALPHA_SHA)],
            {"absent.gs"},
        )

        self.assertFalse(report.ok)
        self.assertIn("declared_change_not_in_source", failure_kinds(report))

    def test_undeclared_content_change_is_refused(self) -> None:
        report = compare(
            [member("START.GS", ALPHA_SHA)],
            [member("START.GS", BRAVO_SHA)],
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["undeclared_content_change"])

    def test_dropped_member_is_refused(self) -> None:
        report = compare(
            [member("START.GS", ALPHA_SHA), member("gone.gs", BRAVO_SHA)],
            [member("START.GS", ALPHA_SHA)],
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["member_missing"])
        self.assertEqual(report.failures[0].path, "gone.gs")

    def test_added_member_is_refused(self) -> None:
        report = compare(
            [member("START.GS", ALPHA_SHA)],
            [member("START.GS", ALPHA_SHA), member("(attributes)", BRAVO_SHA)],
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["member_added"])
        self.assertEqual(report.failures[0].path, "(attributes)")

    def test_case_folded_name_is_refused_as_one_named_finding(self) -> None:
        report = compare(
            [member("Portrait\\AIpotM.lbm", ALPHA_SHA)],
            [member("portrait\\aipotm.lbm", ALPHA_SHA)],
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["member_case_folded"])
        self.assertEqual(report.failures[0].path, "Portrait\\AIpotM.lbm")
        self.assertIn("portrait\\aipotm.lbm", report.failures[0].detail)

    def test_case_folded_name_is_refused_when_the_output_name_is_upper(self) -> None:
        # The mirror of the test above. With only the lower-cased direction the
        # fixture accidentally satisfies a case-sensitive lookup, and a real
        # defect in the matching survives.
        report = compare(
            [member("portrait\\aipotm.lbm", ALPHA_SHA)],
            [member("Portrait\\AIpotM.lbm", ALPHA_SHA)],
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["member_case_folded"])
        self.assertEqual(report.failures[0].path, "portrait\\aipotm.lbm")

    def test_losing_one_of_two_entries_sharing_a_name_is_refused(self) -> None:
        # PIC5R3's real shape: two distinct members, one name.
        report = compare(
            [
                member("portrait\\AIpotM.lbm", ALPHA_SHA, block_index=1108),
                member("portrait\\AIpotM.lbm", BRAVO_SHA, block_index=1144),
            ],
            [member("portrait\\AIpotM.lbm", ALPHA_SHA, block_index=1108)],
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["member_count_changed"])
        self.assertIn("2 entries in the source, 1 in the output", report.failures[0].detail)

    def test_both_entries_sharing_a_name_survive_a_repack(self) -> None:
        source = [
            member("portrait\\AIpotM.lbm", ALPHA_SHA, block_index=1108),
            member("portrait\\AIpotM.lbm", BRAVO_SHA, block_index=1144),
        ]
        # Block indices move when an archive is rewritten; content must not.
        output = [
            member("portrait\\AIpotM.lbm", BRAVO_SHA, block_index=3),
            member("portrait\\AIpotM.lbm", ALPHA_SHA, block_index=9),
        ]
        report = compare(source, output)

        self.assertTrue(report.ok)
        self.assertEqual(report.unchanged_members, 2)

    def test_one_of_two_entries_sharing_a_name_changing_is_refused(self) -> None:
        report = compare(
            [
                member("portrait\\AIpotM.lbm", ALPHA_SHA, block_index=1108),
                member("portrait\\AIpotM.lbm", BRAVO_SHA, block_index=1144),
            ],
            [
                member("portrait\\AIpotM.lbm", ALPHA_SHA, block_index=3),
                member("portrait\\AIpotM.lbm", ALPHA_SHA, block_index=9),
            ],
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["undeclared_content_change"])

    def test_zero_byte_member_is_proven_unchanged_rather_than_ignored(self) -> None:
        members = [member("zero.bin", EMPTY_SHA, size=0, compressed_size=0)]
        report = compare(members, list(members))

        self.assertTrue(report.ok)
        self.assertEqual(report.unchanged_members, 1)

    def test_empty_output_archive_is_refused(self) -> None:
        report = compare([member("START.GS", ALPHA_SHA)], [])

        self.assertFalse(report.ok)
        self.assertIn("output_archive_empty", failure_kinds(report))
        self.assertEqual(report.output_entries, 0)

    def test_metadata_only_change_is_refused(self) -> None:
        report = compare(
            [member("START.GS", ALPHA_SHA, flags="0x80000100")],
            [member("START.GS", ALPHA_SHA, flags="0x80000200")],
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["undeclared_metadata_change"])

    def test_locale_change_is_refused(self) -> None:
        report = compare(
            [member("START.GS", ALPHA_SHA, locale=0)],
            [member("START.GS", ALPHA_SHA, locale=1033)],
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["undeclared_metadata_change"])

    def test_compressed_size_alone_may_change(self) -> None:
        report = compare(
            [member("START.GS", ALPHA_SHA, compressed_size=2740)],
            [member("START.GS", ALPHA_SHA, compressed_size=2752)],
        )

        self.assertTrue(report.ok)
        self.assertEqual(report.unchanged_members, 1)

    def test_rewritten_internal_listfile_is_allowed_but_reported(self) -> None:
        report = compare(
            [member("(listfile)", ALPHA_SHA, size=48469, flags="0x80010100")],
            [member("(listfile)", BRAVO_SHA, size=48457, flags="0x80010100")],
        )

        self.assertTrue(report.ok)
        self.assertEqual(kinds(report), ["internal_listfile_rewritten"])
        self.assertEqual(report.unchanged_members, 0)

    def test_internal_listfile_losing_its_flags_is_refused(self) -> None:
        report = compare(
            [member("(listfile)", ALPHA_SHA, flags="0x80010100")],
            [member("(listfile)", BRAVO_SHA, flags="0x80000200")],
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["internal_member_metadata_change"])

    def test_only_the_listfile_is_exempt_from_the_content_check(self) -> None:
        # `(attributes)` carries timestamps and is NOT exempt: if an archive
        # grows one, or its contents move, that is reported like any other
        # member. Only the listfile, whose names are checked directly, is waived.
        report = compare(
            [member("(attributes)", ALPHA_SHA)],
            [member("(attributes)", BRAVO_SHA)],
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["undeclared_content_change"])

    def test_a_dropped_member_is_refused_even_when_the_listfile_is_rewritten(
        self,
    ) -> None:
        # The listfile exemption must not become an umbrella over real loss.
        report = compare(
            [
                member("(listfile)", ALPHA_SHA, flags="0x80010100"),
                member("gone.gs", BRAVO_SHA),
            ],
            [member("(listfile)", BRAVO_SHA, flags="0x80010100")],
        )

        self.assertFalse(report.ok)
        self.assertEqual(failure_kinds(report), ["member_missing"])


class ManifestReadingTest(unittest.TestCase):
    def test_reads_a_manifest_row(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.tsv"
            path.write_text(
                MANIFEST_HEADER
                + f"START.GS\t1699\t758\t5738\t2740\t0x80000100\t0\t{ALPHA_SHA}\n"
            )
            members = read_manifest(path)

        self.assertEqual(len(members), 1)
        self.assertEqual(members[0].path, "START.GS")
        self.assertEqual(members[0].block_index, 1699)
        self.assertEqual(members[0].hash_index, 758)
        self.assertEqual(members[0].size, 5738)
        self.assertEqual(members[0].compressed_size, 2740)
        self.assertEqual(members[0].flags, "0x80000100")
        self.assertEqual(members[0].locale, 0)
        self.assertEqual(members[0].sha256, ALPHA_SHA)

    def test_reads_a_header_only_manifest_as_no_members(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.tsv"
            path.write_text(MANIFEST_HEADER)
            self.assertEqual(read_manifest(path), [])

    def test_rejects_a_manifest_with_different_columns(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.tsv"
            # The `list` verb's columns, which carry no content hash.
            path.write_text("path\tsize\tcompressed_size\tflags\tlocale\n")
            with self.assertRaises(ValueError):
                read_manifest(path)


class CommandLineTest(unittest.TestCase):
    def _write(self, directory: Path, name: str, rows: str) -> Path:
        path = directory / name
        path.write_text(MANIFEST_HEADER + rows)
        return path

    def test_exit_code_is_zero_when_the_shape_is_preserved(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            row = f"START.GS\t0\t0\t5\t13\t0x80000100\t0\t{ALPHA_SHA}\n"
            source = self._write(root, "source.tsv", row)
            output = self._write(root, "output.tsv", row)
            captured = io.StringIO()
            with redirect_stdout(captured):
                status = main(["--source", str(source), "--output", str(output)])

        self.assertEqual(status, 0)
        self.assertIn("shape preserved", captured.getvalue())

    def test_exit_code_is_nonzero_when_a_member_is_lost(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = self._write(
                root,
                "source.tsv",
                f"START.GS\t0\t0\t5\t13\t0x80000100\t0\t{ALPHA_SHA}\n"
                f"gone.gs\t1\t1\t5\t13\t0x80000100\t0\t{BRAVO_SHA}\n",
            )
            output = self._write(
                root,
                "output.tsv",
                f"START.GS\t0\t0\t5\t13\t0x80000100\t0\t{ALPHA_SHA}\n",
            )
            captured = io.StringIO()
            with redirect_stdout(captured):
                status = main(["--source", str(source), "--output", str(output)])

        self.assertEqual(status, 1)
        self.assertIn("REFUSED", captured.getvalue())
        self.assertIn("member_missing", captured.getvalue())


if __name__ == "__main__":
    unittest.main()
