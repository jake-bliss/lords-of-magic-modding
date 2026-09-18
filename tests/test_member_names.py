"""Tests for member-name recovery: the deciding logic, and the probe itself.

The logic tests run against manifests and probe results that describe archives
which do not exist, so they can pose cases the shipped corpus does not -- a
block that two catalogue names both claim, a name that opens the right block
through the wrong hash slot, a pseudo-name that would otherwise confirm itself.

The probe tests build real archives with `lom-mpq create` and skip when StormLib
is unavailable. They exist because the single most dangerous property here was
found in StormLib, not in this repository: a name of the form `File00000022.*`
is resolved by block position and will "confirm" against anything.
"""

import hashlib
import io
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

PROJECT_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PROJECT_DIR / "tools"))

import member_names  # noqa: E402

MPQ_TOOL = PROJECT_DIR / ".build" / "lom-mpq"
BUILD_SCRIPT = PROJECT_DIR / "scripts" / "build-tools.sh"


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


def manifest_text(rows: list[tuple]) -> str:
    header = "\t".join(member_names.MANIFEST_COLUMNS)
    lines = [header]
    for path, block, hash_index, size, sha in rows:
        lines.append(
            "\t".join(
                [path, str(block), str(hash_index), str(size), "0", "0x80010100", "0", sha]
            )
        )
    return "\n".join(lines) + "\n"


def probe_text(rows: list[tuple]) -> str:
    header = "\t".join(member_names.PROBE_COLUMNS)
    lines = [header]
    for row in rows:
        if len(row) == 1:
            lines.append("\t".join([row[0], "absent", "", "", "", ""]))
        else:
            name, block, hash_index, size, sha = row
            lines.append(
                "\t".join([name, "present", str(block), str(hash_index), str(size), sha])
            )
    return "\n".join(lines) + "\n"


DIGEST_A = "a" * 64
DIGEST_B = "b" * 64


class PseudoNameShapeTest(unittest.TestCase):
    """The shape StormLib resolves positionally, as measured on sndfx.mpq.

    Every string here was run through `lom-mpq probe-names` against vanilla
    `sndfx.mpq` on 2026-09-18; the ones marked positional opened block 22 and
    the others did not open at all. If this rule is ever widened or narrowed,
    these are the cases that must move with it.
    """

    POSITIONAL = (
        "File00000022.wav",
        "File00000022.xxx",
        "File00000022.wavZQNOTREAL",
        "file00000022.wav",
        "FILE00000022.WAV",
    )
    NOT_POSITIONAL = (
        "File00000022",
        "File00000022ZQ",
        "File22.wav",
        "File000000022.wav",
        "sub\\File00000022.wav",
        "gs\\dlg\\oobrksui.gs",
        "portrait\\AIpotM.lbm",
    )

    def test_positional_forms_are_recognised(self) -> None:
        for name in self.POSITIONAL:
            with self.subTest(name=name):
                self.assertTrue(member_names.is_pseudo_name(name))

    def test_other_forms_are_real_names(self) -> None:
        for name in self.NOT_POSITIONAL:
            with self.subTest(name=name):
                self.assertFalse(member_names.is_pseudo_name(name))


class PoolCandidatesTest(unittest.TestCase):
    def test_pseudo_names_never_enter_the_pool(self) -> None:
        pool, attribution = member_names.pool_candidates(
            {"donor": ["gs\\a.gs", "File00000007.xxx", "File00000007.wav"]}
        )
        self.assertEqual(pool, ["gs\\a.gs"])
        self.assertNotIn("file00000007.xxx", attribution)

    def test_case_and_separator_variants_collapse_to_one_candidate(self) -> None:
        pool, attribution = member_names.pool_candidates(
            {"one": ["GS\\A.GS"], "two": ["gs/a.gs"]}
        )
        self.assertEqual(pool, ["GS\\A.GS"])
        self.assertEqual(attribution["gs\\a.gs"], {"one", "two"})

    def test_attribution_records_every_contributor(self) -> None:
        _, attribution = member_names.pool_candidates(
            {"one": ["gs\\a.gs"], "two": ["gs\\a.gs", "gs\\b.gs"]}
        )
        self.assertEqual(attribution["gs\\a.gs"], {"one", "two"})
        self.assertEqual(attribution["gs\\b.gs"], {"two"})


class DecideTest(unittest.TestCase):
    def blocks(self):
        return member_names.read_manifest(
            io.StringIO(
                manifest_text(
                    [
                        ("File00000000.xxx", 0, 100, 10, DIGEST_A),
                        ("gs\\known.gs", 1, 200, 20, DIGEST_B),
                    ]
                )
            )
        )

    def decide(self, probe_rows):
        return member_names.decide(
            self.blocks(), member_names.read_probe(io.StringIO(probe_text(probe_rows)))[0]
        )

    def test_agreeing_name_is_recovered(self) -> None:
        decisions = self.decide([("gs\\found.gs", 0, 100, 10, DIGEST_A)])
        self.assertEqual(decisions[0].state, "recovered")
        self.assertEqual(decisions[0].name, "gs\\found.gs")
        self.assertEqual(decisions[0].rejected, [])

    def test_a_name_landing_in_the_wrong_hash_slot_is_rejected(self) -> None:
        decisions = self.decide([("gs\\found.gs", 0, 101, 10, DIGEST_A)])
        self.assertEqual(decisions[0].state, "unnamed")
        self.assertEqual(decisions[0].rejected, [("gs\\found.gs", "hash-index disagrees")])

    def test_a_name_whose_size_disagrees_is_rejected(self) -> None:
        decisions = self.decide([("gs\\found.gs", 0, 100, 11, DIGEST_A)])
        self.assertEqual(decisions[0].state, "unnamed")
        self.assertEqual(decisions[0].rejected, [("gs\\found.gs", "size disagrees")])

    def test_a_name_whose_digest_disagrees_is_rejected(self) -> None:
        decisions = self.decide([("gs\\found.gs", 0, 100, 10, DIGEST_B)])
        self.assertEqual(decisions[0].state, "unnamed")
        self.assertEqual(decisions[0].rejected, [("gs\\found.gs", "digest disagrees")])

    def test_a_pseudo_name_is_rejected_even_when_everything_agrees(self) -> None:
        """The trap. Block 0 opens `File00000000.xxx` and every field matches.

        It is still not a name; it is the block number written back out.
        """
        decisions = self.decide([("File00000000.xxx", 0, 100, 10, DIGEST_A)])
        self.assertEqual(decisions[0].state, "unnamed")
        self.assertEqual(
            decisions[0].rejected, [("File00000000.xxx", "positional pseudo-name")]
        )

    def test_two_agreeing_names_for_one_block_are_reported_as_ambiguous(self) -> None:
        decisions = self.decide(
            [
                ("gs\\one.gs", 0, 100, 10, DIGEST_A),
                ("gs\\two.gs", 0, 100, 10, DIGEST_A),
            ]
        )
        self.assertEqual(decisions[0].state, "recovered-ambiguous")
        self.assertEqual(sorted(decisions[0].confirmed), ["gs\\one.gs", "gs\\two.gs"])

    def test_an_already_named_block_is_not_reported_as_recovered(self) -> None:
        decisions = self.decide([("gs\\known.gs", 1, 200, 20, DIGEST_B)])
        self.assertEqual(decisions[1].state, "already-named")
        self.assertEqual(decisions[1].name, "gs\\known.gs")

    def test_a_hit_on_a_block_the_manifest_never_listed_is_an_error(self) -> None:
        with self.assertRaises(ValueError):
            self.decide([("gs\\found.gs", 9, 100, 10, DIGEST_A)])

    def test_a_block_nothing_claims_stays_unnamed(self) -> None:
        decisions = self.decide([("gs\\absent.gs",)])
        self.assertEqual(decisions[0].state, "unnamed")
        self.assertEqual(decisions[0].name, "")


class ReaderTest(unittest.TestCase):
    def test_manifest_columns_are_checked(self) -> None:
        with self.assertRaises(ValueError):
            member_names.read_manifest(io.StringIO("path\tsize\n"))

    def test_probe_columns_are_checked(self) -> None:
        with self.assertRaises(ValueError):
            member_names.read_probe(io.StringIO("name\tstatus\n"))

    def test_a_repeated_block_index_is_refused(self) -> None:
        text = manifest_text(
            [("File00000000.xxx", 0, 100, 10, DIGEST_A), ("gs\\a.gs", 0, 101, 10, DIGEST_B)]
        )
        with self.assertRaises(ValueError):
            member_names.read_manifest(io.StringIO(text))

    def test_an_unexpected_status_is_refused(self) -> None:
        text = "\t".join(member_names.PROBE_COLUMNS) + "\nx\tmaybe\t\t\t\t\n"
        with self.assertRaises(ValueError):
            member_names.read_probe(io.StringIO(text))


class ControlTest(unittest.TestCase):
    def test_controls_are_drawn_from_the_pool_at_the_given_stride(self) -> None:
        pool = [f"gs\\{index}.gs" for index in range(10)]
        self.assertEqual(
            member_names.make_controls(pool, stride=5),
            ["gs\\4.gsZQNOTREAL", "gs\\9.gsZQNOTREAL"],
        )

    def test_a_stride_of_one_mutates_every_name(self) -> None:
        pool = ["gs\\a.gs", "gs\\b.gs"]
        self.assertEqual(len(member_names.make_controls(pool, stride=1)), 2)

    def test_a_control_that_opened_fails_the_run(self) -> None:
        hits = [member_names.ProbeHit("gs\\a.gsZQ", 0, 1, 1, DIGEST_A)]
        failures = member_names.check_controls(["gs\\a.gsZQ"], [], hits)
        self.assertEqual(len(failures), 1)
        self.assertIn("opened", failures[0])

    def test_a_control_that_was_never_probed_fails_the_run(self) -> None:
        failures = member_names.check_controls(["gs\\a.gsZQ"], [], [])
        self.assertEqual(len(failures), 1)
        self.assertIn("never probed", failures[0])

    def test_a_refused_control_passes(self) -> None:
        self.assertEqual(member_names.check_controls(["gs\\a.gsZQ"], ["gs\\A.gsZQ"], []), [])


class OutputTest(unittest.TestCase):
    def decisions(self):
        blocks = member_names.read_manifest(
            io.StringIO(
                manifest_text(
                    [
                        ("File00000000.xxx", 0, 100, 10, DIGEST_A),
                        ("gs\\known.gs", 1, 200, 20, DIGEST_B),
                        ("File00000002.xxx", 2, 300, 30, DIGEST_A),
                    ]
                )
            )
        )
        hits = member_names.read_probe(
            io.StringIO(probe_text([("gs\\found.gs", 0, 100, 10, DIGEST_A)]))
        )[0]
        return member_names.decide(blocks, hits)

    def test_the_emitted_listfile_carries_only_recovered_names(self) -> None:
        handle = io.StringIO()
        written = member_names.write_listfile(handle, self.decisions())
        self.assertEqual(written, 1)
        self.assertEqual(handle.getvalue(), "gs\\found.gs\n")

    def test_the_resolution_reports_every_block_including_the_refusals(self) -> None:
        handle = io.StringIO()
        member_names.write_resolution(handle, self.decisions())
        lines = handle.getvalue().splitlines()
        self.assertEqual(len(lines), 4)
        self.assertEqual(lines[1].split("\t")[2], "recovered")
        self.assertEqual(lines[2].split("\t")[2], "already-named")
        self.assertEqual(lines[3].split("\t")[2], "unnamed")

    def test_source_contribution_separates_coverage_from_sole_credit(self) -> None:
        _, attribution = member_names.pool_candidates(
            {"donor": ["gs\\found.gs"], "other": ["gs\\found.gs"]}
        )
        shared = member_names.source_contribution(self.decisions(), attribution)
        self.assertEqual(shared["donor"], {"covered": 1, "unique": 0})

        _, solo = member_names.pool_candidates({"donor": ["gs\\found.gs"]})
        alone = member_names.source_contribution(self.decisions(), solo)
        self.assertEqual(alone["donor"], {"covered": 1, "unique": 1})


@unittest.skipUnless(stormlib_available(), "lom-mpq could not be built (StormLib?)")
class ProbeNamesTest(unittest.TestCase):
    """The C++ probe, against archives this test builds.

    No game data is involved and nothing outside the temporary directory is
    read or written.
    """

    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.root = Path(self._temporary.name)
        self.addCleanup(self._temporary.cleanup)

        member = self.root / "member.txt"
        member.write_bytes(b"the quick brown fox\n")
        self.archive = self.root / "probe.mpq"
        result = subprocess.run(
            [
                str(MPQ_TOOL),
                "create",
                str(self.archive),
                "--add",
                f"gs\\real.gs={member}",
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def probe(self, names: list[str]) -> dict[str, tuple[str, ...]]:
        listing = self.root / "names.txt"
        listing.write_text("\n".join(names) + "\n", encoding="utf-8")
        result = subprocess.run(
            [str(MPQ_TOOL), "probe-names", str(self.archive), str(listing)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        rows = [line.split("\t") for line in result.stdout.splitlines()[1:]]
        return {row[0]: tuple(row[1:]) for row in rows}

    def test_a_real_name_opens_and_a_near_miss_does_not(self) -> None:
        answers = self.probe(["gs\\real.gs", "gs\\real.gsZQNOTREAL", "gs\\realX.gs"])
        self.assertEqual(answers["gs\\real.gs"][0], "present")
        self.assertEqual(answers["gs\\real.gsZQNOTREAL"][0], "absent")
        self.assertEqual(answers["gs\\realX.gs"][0], "absent")

    def test_case_does_not_change_the_answer(self) -> None:
        answers = self.probe(["gs\\real.gs", "GS\\REAL.GS"])
        self.assertEqual(answers["gs\\real.gs"][1:], answers["GS\\REAL.GS"][1:])

    def test_a_pseudo_name_resolves_by_position_and_ignores_the_extension(self) -> None:
        """The reason `is_pseudo_name` exists, measured rather than assumed."""
        answers = self.probe(
            [
                "gs\\real.gs",
                "File00000000.xxx",
                "File00000000.wav",
                "File00000000.gsZQNOTREAL",
                "File00000000",
            ]
        )
        real = answers["gs\\real.gs"]
        for name in ("File00000000.xxx", "File00000000.wav", "File00000000.gsZQNOTREAL"):
            with self.subTest(name=name):
                self.assertEqual(answers[name][0], "present")
                self.assertEqual(answers[name][1], real[1])
        self.assertEqual(answers["File00000000"][0], "absent")

    def test_the_probe_reports_the_bytes_it_read(self) -> None:
        """The digest is computed here independently of the C++ implementation.

        A literal would only pin whatever the tool printed the first time. This
        compares StormLib-plus-CommonCrypto against hashlib over the bytes the
        member was built from.
        """
        contents = b"the quick brown fox\n"
        answers = self.probe(["gs\\real.gs"])
        self.assertEqual(answers["gs\\real.gs"][3], str(len(contents)))
        self.assertEqual(
            answers["gs\\real.gs"][4], hashlib.sha256(contents).hexdigest()
        )


@unittest.skipUnless(stormlib_available(), "lom-mpq could not be built (StormLib?)")
class ListfileWiringTest(unittest.TestCase):
    """`--listfile` must add names without disturbing block-index addressing.

    The fixture is built with `create --no-listfile`, which is the only way to
    get a member the archive genuinely cannot name. Without that, every member
    is already named and this whole feature would be untestable -- a passing
    suite would prove nothing.
    """

    MEMBER = "gs\\real.gs"
    CONTENTS = b"content\n"

    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.root = Path(self._temporary.name)
        self.addCleanup(self._temporary.cleanup)
        member = self.root / "member.txt"
        member.write_bytes(self.CONTENTS)
        self.archive = self.root / "wire.mpq"
        result = subprocess.run(
            [
                str(MPQ_TOOL),
                "create",
                str(self.archive),
                "--no-listfile",
                "--add",
                f"{self.MEMBER}={member}",
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def names_file(self, *names: str) -> Path:
        path = self.root / "names.txt"
        path.write_text("".join(name + "\n" for name in names), encoding="utf-8")
        return path

    def manifest(self, *extra: str) -> list[list[str]]:
        result = subprocess.run(
            [str(MPQ_TOOL), "manifest", str(self.archive), *extra],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return [line.split("\t") for line in result.stdout.splitlines()[1:]]

    def test_the_fixture_really_holds_an_unnamed_member(self) -> None:
        rows = self.manifest()
        self.assertEqual(len(rows), 1)
        self.assertTrue(member_names.is_pseudo_name(rows[0][0]))

    def test_a_supplied_name_is_applied(self) -> None:
        rows = self.manifest("--listfile", str(self.names_file(self.MEMBER)))
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0][0], self.MEMBER)

    def test_a_supplied_name_changes_no_block_index_size_or_digest(self) -> None:
        plain = {row[1]: row[2:] for row in self.manifest()}
        named = {
            row[1]: row[2:]
            for row in self.manifest("--listfile", str(self.names_file(self.MEMBER)))
        }
        self.assertEqual(plain, named)
        self.assertEqual(
            next(iter(named.values()))[-1], hashlib.sha256(self.CONTENTS).hexdigest()
        )

    def test_a_name_the_archive_does_not_hold_is_simply_not_used(self) -> None:
        rows = self.manifest(
            "--listfile", str(self.names_file("gs\\nothing-like-this.gs"))
        )
        self.assertEqual(rows, self.manifest())

    def test_a_missing_listfile_is_refused_rather_than_ignored(self) -> None:
        result = subprocess.run(
            [
                str(MPQ_TOOL),
                "manifest",
                str(self.archive),
                "--listfile",
                str(self.root / "absent.txt"),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertNotEqual(result.returncode, 0)

    def test_extraction_uses_the_supplied_name(self) -> None:
        destination = self.root / "out"
        result = subprocess.run(
            [
                str(MPQ_TOOL),
                "extract",
                str(self.archive),
                str(destination),
                "--listfile",
                str(self.names_file(self.MEMBER)),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((destination / "gs" / "real.gs").read_bytes(), self.CONTENTS)


if __name__ == "__main__":
    unittest.main()
