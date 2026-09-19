"""End-to-end tests for the repack pipeline against archives this suite builds.

These archives are deliberately unlike the shipped corpus: a zero-byte member, a
member whose name differs from another only in case, an archive that lost a
member. No game data is involved and nothing outside the temporary directory is
written.
"""

import hashlib
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

PROJECT_DIR = Path(__file__).resolve().parents[1]
MPQ_TOOL = PROJECT_DIR / ".build" / "lom-mpq"
BUILD_SCRIPT = PROJECT_DIR / "scripts" / "build-tools.sh"
REPACK_SCRIPT = PROJECT_DIR / "scripts" / "repack-archive.sh"
SHAPE_TOOL = PROJECT_DIR / "tools" / "mpq_shape.py"

EMPTY_SHA = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"


def stormlib_available() -> bool:
    if MPQ_TOOL.is_file():
        return True
    if shutil.which("brew") is None:
        return False
    return (
        subprocess.run(
            [BUILD_SCRIPT], capture_output=True, check=False, cwd=PROJECT_DIR
        ).returncode
        == 0
    )


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


@unittest.skipUnless(stormlib_available(), "lom-mpq could not be built (StormLib?)")
class RepackPipelineTest(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.root = Path(self._temporary.name)
        self.addCleanup(self._temporary.cleanup)

    def local_file(self, name: str, contents: bytes) -> Path:
        path = self.root / name
        path.write_bytes(contents)
        return path

    def run_tool(self, *arguments: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [str(MPQ_TOOL), *arguments], capture_output=True, text=True, check=False
        )

    def create_archive(self, name: str, members: dict[str, bytes]) -> Path:
        archive = self.root / name
        arguments = ["create", str(archive)]
        for index, (archived_name, contents) in enumerate(members.items()):
            source = self.local_file(f"{name}.member{index}", contents)
            arguments += ["--add", f"{archived_name}={source}"]
        result = self.run_tool(*arguments)
        self.assertEqual(result.returncode, 0, result.stderr)
        return archive

    def manifest(self, archive: Path, name: str) -> Path:
        result = self.run_tool("manifest", str(archive))
        self.assertEqual(result.returncode, 0, result.stderr)
        path = self.root / name
        path.write_text(result.stdout)
        return path

    def shape_check(
        self, source: Path, output: Path, expected: tuple[str, ...] = ()
    ) -> subprocess.CompletedProcess:
        arguments = ["python3", str(SHAPE_TOOL), "--source", str(source), "--output", str(output)]
        for name in expected:
            arguments += ["--expect-changed", name]
        return subprocess.run(arguments, capture_output=True, text=True, check=False)

    # -- the manifest sees what extraction cannot -------------------------------

    def test_manifest_hashes_a_zero_byte_member(self) -> None:
        archive = self.create_archive("zero.mpq", {"zero.bin": b""})
        rows = self.manifest(archive, "zero.tsv").read_text().splitlines()
        member_row = next(row for row in rows if row.startswith("zero.bin\t"))
        columns = member_row.split("\t")

        self.assertEqual(columns[3], "0")
        self.assertEqual(columns[7], EMPTY_SHA)

    def test_two_names_differing_only_in_case_collapse_into_one_member(self) -> None:
        # Observed 2026-09-18: the MPQ name hash is case-insensitive, so an MPQ
        # cannot hold both. The first name survives; the second content wins.
        archive = self.create_archive("case.mpq", {"A.txt": b"alpha", "a.txt": b"BRAVO"})
        rows = self.manifest(archive, "case.tsv").read_text().splitlines()[1:]
        names = [row.split("\t")[0] for row in rows]

        self.assertIn("A.txt", names)
        self.assertNotIn("a.txt", names)
        alpha_row = next(row for row in rows if row.startswith("A.txt\t"))
        self.assertEqual(
            alpha_row.split("\t")[7],
            "f3233097bacb2526cb78cb72d2e1bbeb8e3675abac47990f678a505f3ad6daa7",
        )

    # -- repacking ---------------------------------------------------------------

    def test_repack_replaces_a_member_and_passes_the_shape_check(self) -> None:
        archive = self.create_archive(
            "source.mpq", {"keep.gs": b"keep me", "swap.gs": b"before", "zero.bin": b""}
        )
        source_hash = sha256_file(archive)
        source_manifest = self.manifest(archive, "source.tsv")
        replacement = self.local_file("swap-new.gs", b"after the change")

        output = self.root / "output.mpq"
        result = self.run_tool(
            "repack", str(archive), str(output), "--replace", f"swap.gs={replacement}"
        )
        self.assertEqual(result.returncode, 0, result.stderr)

        output_manifest = self.manifest(output, "output.tsv")
        checked = self.shape_check(source_manifest, output_manifest, ("swap.gs",))

        self.assertEqual(checked.returncode, 0, checked.stdout)
        self.assertIn("shape preserved", checked.stdout)
        self.assertIn("proven unchanged 2", checked.stdout)
        self.assertEqual(sha256_file(archive), source_hash, "the source was modified")

    def test_repacking_twice_produces_byte_identical_archives(self) -> None:
        archive = self.create_archive("source.mpq", {"swap.gs": b"before"})
        replacement = self.local_file("swap-new.gs", b"after")

        hashes = []
        for index in (1, 2):
            output = self.root / f"output{index}.mpq"
            result = self.run_tool(
                "repack", str(archive), str(output), "--replace", f"swap.gs={replacement}"
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            hashes.append(sha256_file(output))

        self.assertEqual(hashes[0], hashes[1])

    def test_replacement_order_does_not_change_the_output(self) -> None:
        archive = self.create_archive("source.mpq", {"a.gs": b"aaa", "b.gs": b"bbb"})
        first = self.local_file("a-new.gs", b"AAA changed")
        second = self.local_file("b-new.gs", b"BBB changed")

        forward = self.root / "forward.mpq"
        backward = self.root / "backward.mpq"
        self.run_tool(
            "repack", str(archive), str(forward),
            "--replace", f"a.gs={first}", "--replace", f"b.gs={second}",
        )
        self.run_tool(
            "repack", str(archive), str(backward),
            "--replace", f"b.gs={second}", "--replace", f"a.gs={first}",
        )

        self.assertEqual(sha256_file(forward), sha256_file(backward))

    def test_repack_preserves_the_storage_flags_of_a_replaced_member(self) -> None:
        archive = self.root / "compressed.mpq"
        member_source = self.local_file("compressed.member", b"original content")
        created = self.run_tool(
            "create", str(archive), "--compress", "--add", f"swap.lbm={member_source}"
        )
        self.assertEqual(created.returncode, 0, created.stderr)
        source_manifest = self.manifest(archive, "source.tsv")
        self.assertIn("0x80000200", source_manifest.read_text())

        replacement = self.local_file("swap-new.lbm", b"a different picture entirely")
        output = self.root / "output.mpq"
        result = self.run_tool(
            "repack", str(archive), str(output), "--replace", f"swap.lbm={replacement}"
        )
        self.assertEqual(result.returncode, 0, result.stderr)

        checked = self.shape_check(
            source_manifest, self.manifest(output, "output.tsv"), ("swap.lbm",)
        )
        self.assertEqual(checked.returncode, 0, checked.stdout)
        output_row = next(
            row
            for row in self.manifest(output, "output.tsv").read_text().splitlines()
            if row.startswith("swap.lbm\t")
        )
        self.assertEqual(output_row.split("\t")[5], "0x80000200")

    def test_repack_accepts_a_local_path_containing_an_equals_sign(self) -> None:
        # The archive name is separated from the local path at the LAST `=`.
        archive = self.create_archive("source.mpq", {"swap.gs": b"before"})
        directory = self.root / "dir=with=equals"
        directory.mkdir()
        replacement = directory / "swap-new.gs"
        replacement.write_bytes(b"after")

        output = self.root / "output.mpq"
        result = self.run_tool(
            "repack", str(archive), str(output), "--replace", f"swap.gs={replacement}"
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        checked = self.shape_check(
            self.manifest(archive, "source.tsv"),
            self.manifest(output, "output.tsv"),
            ("swap.gs",),
        )
        self.assertEqual(checked.returncode, 0, checked.stdout)

    def test_repacking_a_fixed_fixture_produces_a_known_archive(self) -> None:
        # The strongest statement this suite can make about determinism: not
        # "two runs agreed" but "the bytes are these bytes". Written out as
        # literals, measured against StormLib 9.40 on 2026-09-18. If a StormLib
        # upgrade changes the output, this is the test that should say so.
        archive = self.create_archive("source.mpq", {"swap.gs": b"before"})
        self.assertEqual(
            sha256_file(archive),
            "7580b72b1a54d9943facd9b491fbe0a5aa39976d79cf3be14a0aaff0c251a856",
        )

        replacement = self.local_file("swap-new.gs", b"after")
        output = self.root / "output.mpq"
        result = self.run_tool(
            "repack", str(archive), str(output), "--replace", f"swap.gs={replacement}"
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            sha256_file(output),
            "b9f55530aaf1f6657f88caee806dcea14a9c77c2394a477c3077afcafb449a1e",
        )

    # -- refusals ----------------------------------------------------------------

    def test_repack_refuses_a_member_the_source_does_not_have(self) -> None:
        archive = self.create_archive("source.mpq", {"present.gs": b"here"})
        replacement = self.local_file("new.gs", b"new")
        output = self.root / "output.mpq"

        result = self.run_tool(
            "repack", str(archive), str(output), "--replace", f"absent.gs={replacement}"
        )

        self.assertEqual(result.returncode, 1)
        self.assertIn("no member named", result.stderr)
        self.assertFalse(output.exists())
        self.assertFalse((self.root / "output.mpq.partial").exists())

    def test_repack_refuses_to_overwrite_an_existing_output(self) -> None:
        archive = self.create_archive("source.mpq", {"swap.gs": b"before"})
        replacement = self.local_file("new.gs", b"after")
        output = self.local_file("output.mpq", b"do not touch me")

        result = self.run_tool(
            "repack", str(archive), str(output), "--replace", f"swap.gs={replacement}"
        )

        self.assertEqual(result.returncode, 1)
        self.assertIn("already exists", result.stderr)
        self.assertEqual(output.read_bytes(), b"do not touch me")

    def test_repack_refuses_a_missing_replacement_file(self) -> None:
        archive = self.create_archive("source.mpq", {"swap.gs": b"before"})
        output = self.root / "output.mpq"

        result = self.run_tool(
            "repack", str(archive), str(output),
            "--replace", f"swap.gs={self.root / 'nonexistent'}",
        )

        self.assertEqual(result.returncode, 1)
        self.assertIn("not a regular file", result.stderr)
        self.assertFalse(output.exists())

    def test_repack_cleans_up_after_a_failure_during_the_rewrite(self) -> None:
        # The earlier refusals all fire before the staging copy exists. This one
        # fails inside the rewrite, which is the case the cleanup path is for.
        archive = self.create_archive("source.mpq", {"swap.gs": b"before"})
        replacement = self.local_file("unreadable.gs", b"after")
        replacement.chmod(0o000)
        self.addCleanup(replacement.chmod, 0o600)
        output = self.root / "output.mpq"

        result = self.run_tool(
            "repack", str(archive), str(output), "--replace", f"swap.gs={replacement}"
        )

        self.assertEqual(result.returncode, 1)
        self.assertIn("could not add member", result.stderr)
        self.assertFalse(output.exists())
        self.assertFalse(
            (self.root / "output.mpq.partial").exists(),
            "a half-written archive was left on disk",
        )

    def test_shape_check_refuses_an_archive_that_lost_a_member(self) -> None:
        source = self.create_archive("source.mpq", {"a.gs": b"aaa", "b.gs": b"bbb"})
        lossy = self.create_archive("lossy.mpq", {"a.gs": b"aaa"})

        checked = self.shape_check(
            self.manifest(source, "source.tsv"), self.manifest(lossy, "lossy.tsv")
        )

        self.assertEqual(checked.returncode, 1)
        self.assertIn("member_missing b.gs", checked.stdout)
        self.assertIn("REFUSED", checked.stdout)

    def test_shape_check_refuses_a_case_folded_output_name(self) -> None:
        source = self.create_archive("source.mpq", {"Portrait.lbm": b"art"})
        folded = self.create_archive("folded.mpq", {"portrait.lbm": b"art"})

        checked = self.shape_check(
            self.manifest(source, "source.tsv"), self.manifest(folded, "folded.tsv")
        )

        self.assertEqual(checked.returncode, 1)
        self.assertIn("member_case_folded", checked.stdout)

    # -- the driver script --------------------------------------------------------

    def test_driver_refuses_to_write_inside_the_applications_directory(self) -> None:
        fake_home = self.root / "home"
        (fake_home / "Applications").mkdir(parents=True)
        archive = self.create_archive("source.mpq", {"swap.gs": b"before"})
        replacement = self.local_file("new.gs", b"after")
        output = fake_home / "Applications" / "gs.mpq"

        result = subprocess.run(
            [str(REPACK_SCRIPT), str(archive), str(output), f"swap.gs={replacement}"],
            capture_output=True,
            text=True,
            check=False,
            env={"HOME": str(fake_home), "PATH": "/usr/bin:/bin:/usr/sbin:/sbin"},
        )

        self.assertEqual(result.returncode, 1)
        self.assertIn("refusing to write inside", result.stderr)
        self.assertFalse(output.exists())

    def test_driver_refuses_to_install(self) -> None:
        result = subprocess.run(
            [str(REPACK_SCRIPT), "--install", "Lords of Magic Development.app"],
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertEqual(result.returncode, 2)
        self.assertIn("Phase 3", result.stderr)

    def test_driver_reports_repeated_runs_as_byte_identical(self) -> None:
        archive = self.create_archive("source.mpq", {"swap.gs": b"before"})
        replacement = self.local_file("new.gs", b"after")
        output = self.root / "output.mpq"

        result = subprocess.run(
            [
                str(REPACK_SCRIPT), str(archive), str(output),
                f"swap.gs={replacement}", "--determinism-runs", "3",
            ],
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("every run produced byte-identical output", result.stdout)
        self.assertEqual(result.stdout.count("  run "), 4)

    def test_driver_deletes_an_archive_that_fails_its_shape_check(self) -> None:
        archive = self.create_archive("source.mpq", {"swap.gs": b"before"})
        replacement = self.local_file("new.gs", b"before")  # identical content
        output = self.root / "output.mpq"

        result = subprocess.run(
            [str(REPACK_SCRIPT), str(archive), str(output), f"swap.gs={replacement}"],
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertEqual(result.returncode, 1)
        self.assertIn("declared_change_not_applied", result.stdout)
        self.assertFalse(output.exists(), "a refused archive was left on disk")

    # -- a repack that is meant to change nothing ---------------------------------

    def test_driver_accepts_a_declared_no_op_and_says_so(self) -> None:
        """Rung 0 of the engine ladder: rewrite a member to the bytes it already had.

        Without `--expect-unchanged` this is exactly the archive the driver deletes in
        `test_driver_deletes_an_archive_that_fails_its_shape_check`, and the two tests are kept
        beside each other for that reason.
        """
        archive = self.create_archive("source.mpq", {"keep.gs": b"keep", "swap.gs": b"same"})
        replacement = self.local_file("new.gs", b"same")
        output = self.root / "output.mpq"

        result = subprocess.run(
            [
                str(REPACK_SCRIPT), str(archive), str(output),
                "--expect-unchanged", "swap.gs", f"swap.gs={replacement}",
            ],
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("member_rewritten_unchanged", result.stdout)
        self.assertIn("shape preserved", result.stdout)
        self.assertTrue(output.exists())

    def test_driver_deletes_an_archive_whose_declared_no_op_moved(self) -> None:
        archive = self.create_archive("source.mpq", {"swap.gs": b"before"})
        replacement = self.local_file("new.gs", b"after")
        output = self.root / "output.mpq"

        result = subprocess.run(
            [
                str(REPACK_SCRIPT), str(archive), str(output),
                "--expect-unchanged", "swap.gs", f"swap.gs={replacement}",
            ],
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertEqual(result.returncode, 1)
        self.assertIn("declared_no_op_changed_content", result.stdout)
        self.assertFalse(output.exists())

    def test_driver_refuses_an_expect_unchanged_that_names_no_replacement(self) -> None:
        # A typo here would not fail loudly: the member would simply be compared as an undeclared
        # one, which is a weaker check wearing the same exit code.
        archive = self.create_archive("source.mpq", {"swap.gs": b"before"})
        replacement = self.local_file("new.gs", b"after")
        output = self.root / "output.mpq"

        result = subprocess.run(
            [
                str(REPACK_SCRIPT), str(archive), str(output),
                "--expect-unchanged", "swpa.gs", f"swap.gs={replacement}",
            ],
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertEqual(result.returncode, 2)
        self.assertIn("names no replacement", result.stderr)

    # -- adding a member ----------------------------------------------------------

    def test_repack_adds_a_member_the_source_did_not_have(self) -> None:
        archive = self.create_archive("source.mpq", {"keep.gs": b"keep"})
        addition = self.local_file("added.gs", b"brand new")
        output = self.root / "output.mpq"

        result = self.run_tool(
            "repack", str(archive), str(output), "--add", f"new.gs={addition}"
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("0 replaced and 1 added", result.stdout)
        rows = self.manifest(output, "output.tsv").read_text().splitlines()[1:]
        names = [row.split("\t")[0] for row in rows]
        self.assertIn("new.gs", names)
        self.assertIn("keep.gs", names)

    def test_an_added_member_inherits_the_archives_storage_flags(self) -> None:
        archive = self.create_archive("source.mpq", {"keep.gs": b"keep"})
        addition = self.local_file("added.gs", b"brand new")
        output = self.root / "output.mpq"
        self.run_tool("repack", str(archive), str(output), "--add", f"new.gs={addition}")

        rows = self.manifest(output, "output.tsv").read_text().splitlines()[1:]
        flags = {row.split("\t")[0]: row.split("\t")[5] for row in rows}

        self.assertEqual(flags["new.gs"], flags["keep.gs"])

    def test_repack_refuses_to_add_a_member_the_source_already_has(self) -> None:
        archive = self.create_archive("source.mpq", {"keep.gs": b"keep"})
        addition = self.local_file("added.gs", b"brand new")
        output = self.root / "output.mpq"

        result = self.run_tool(
            "repack", str(archive), str(output), "--add", f"keep.gs={addition}"
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("use --replace, not --add", result.stderr)
        self.assertFalse(output.exists())

    def test_repack_refuses_to_add_when_the_archive_stores_members_two_ways(self) -> None:
        # There is no storage class to inherit, and choosing one silently is how an added member
        # ends up stored in a way no member of the archive is.
        archive = self.root / "mixed.mpq"
        stored = self.local_file("stored.bin", b"stored member")
        imploded = self.local_file("imploded.bin", b"imploded member" * 40)
        self.assertEqual(
            self.run_tool(
                "create", str(archive),
                "--store", "--add", f"stored.bin={stored}",
                "--implode", "--add", f"imploded.bin={imploded}",
            ).returncode,
            0,
        )
        addition = self.local_file("third.bin", b"third")
        output = self.root / "output.mpq"

        result = self.run_tool(
            "repack", str(archive), str(output), "--add", f"third.bin={addition}"
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not store all its members the same way", result.stderr)
        self.assertFalse(output.exists())

    def test_an_archives_own_listfile_does_not_make_it_look_mixed(self) -> None:
        """The negative control for the refusal above.

        StormLib writes `(listfile)` uncompressed into an archive whose every real member is
        imploded. Counting it would refuse EVERY addition to EVERY archive carrying one -- which
        is every archive `create` makes by default, and `gs.mpq` -- for a reason that has nothing
        to do with how the content is stored.
        """
        archive = self.create_archive("listed.mpq", {"keep.gs": b"keep" * 40})
        rows = self.manifest(archive, "listed.tsv").read_text().splitlines()[1:]
        flags = {row.split("\t")[0]: row.split("\t")[5] for row in rows}
        self.assertIn("(listfile)", flags)
        self.assertNotEqual(
            flags["(listfile)"], flags["keep.gs"], "the fixture is not mixed at all"
        )

        addition = self.local_file("added.gs", b"brand new")
        output = self.root / "output.mpq"
        result = self.run_tool(
            "repack", str(archive), str(output), "--add", f"new.gs={addition}"
        )

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_driver_reports_an_added_member_by_name_in_the_shape_check(self) -> None:
        archive = self.create_archive("source.mpq", {"keep.gs": b"keep"})
        addition = self.local_file("added.gs", b"brand new")
        output = self.root / "output.mpq"

        result = subprocess.run(
            [str(REPACK_SCRIPT), str(archive), str(output), "--add", f"new.gs={addition}"],
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("member_added_as_declared new.gs", result.stdout)
        self.assertIn("shape preserved", result.stdout)

    def test_driver_refuses_a_repack_with_nothing_to_do(self) -> None:
        archive = self.create_archive("source.mpq", {"keep.gs": b"keep"})
        output = self.root / "output.mpq"

        result = subprocess.run(
            [str(REPACK_SCRIPT), str(archive), str(output)],
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertEqual(result.returncode, 2)
        self.assertIn("at least one replacement or one --add", result.stderr)


if __name__ == "__main__":
    unittest.main()
