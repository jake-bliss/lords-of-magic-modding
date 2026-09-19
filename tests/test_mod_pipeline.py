"""install-dev and restore-dev, run for real against a fabricated `Applications` directory.

These are not unit tests of a Python function; they run the shell scripts, with
`LOM_APPLICATIONS_DIR` and `LOM_ARTIFACTS_DIR` pointed at a temporary tree. The archives are a few
bytes of nonsense rather than real MPQs, because nothing under test opens one: creating a profile,
approving a path, hashing a file and verifying a restore are all archive-agnostic.

**Nothing here reads or writes anything under `~/Applications`.** The most important assertion in
the file is `assert_other_profiles_untouched`, which re-hashes every fabricated profile after every
operation. It is called by every test that writes anything.
"""

import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from tools.mod_build import ENGINE_ACCEPTANCE
from tools.mod_tree import GAME_SUBPATH, PROFILE_APPS

PROJECT_DIR = Path(__file__).resolve().parent.parent
DEV_PROFILE_NAME = "Lords of Magic Development.app"
BASELINE = PROFILE_APPS["vanilla"]
ARCHIVES = ("gs.mpq", "pic.mpq")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def game_is_running() -> bool:
    """True when `lomse.exe` is alive, which these tests cannot run around.

    `scripts/install-dev.sh` and `scripts/restore-dev.sh` refuse while the game is up -- swapping an
    archive under a live process is a class of corruption no checksum afterwards can undo -- so every
    test that drives them fails, with a message about the game rather than about the code.

    That is a correct refusal and a useless test result. Skipping names the real reason: measured
    2026-09-18, opening the Map Editor for an unrelated experiment turned 11 tests red and 3 more
    into errors, and the suite said nothing about which. A red suite that means "a game is open"
    teaches people to disbelieve red.
    """
    try:
        return (
            subprocess.run(
                ["pgrep", "-f", "lomse.exe"],
                capture_output=True,
                check=False,
            ).returncode
            == 0
        )
    except OSError:
        # No pgrep: assume clear rather than skip the suite on a machine that cannot answer.
        return False


@unittest.skipIf(game_is_running(), "lomse.exe is running; install/restore refuse while it is up")
class PipelineTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.base = Path(self._temporary.name)
        self.applications = self.base / "Applications"
        self.artifacts = self.base / "artifacts"
        self.artifacts.mkdir(parents=True)

        for label, app in PROFILE_APPS.items():
            game_dir = self.applications / app / GAME_SUBPATH
            game_dir.mkdir(parents=True)
            for archive in ARCHIVES:
                game_dir.joinpath(archive).write_bytes(f"MPQ\x1a {label} {archive}".encode())
            # The loose map/ directory, which has no backup anywhere in this project.
            game_dir.joinpath("map").mkdir()
            game_dir.joinpath("map", "shipped.scn").write_bytes(b"map bytes")

        self.pristine_state = self.snapshot()

    def tearDown(self) -> None:
        self._temporary.cleanup()

    def snapshot(self) -> dict[str, str]:
        state = {}
        for app in PROFILE_APPS.values():
            for path in sorted((self.applications / app).rglob("*")):
                if path.is_file():
                    state[str(path.relative_to(self.applications))] = digest(path)
        return state

    def assert_other_profiles_untouched(self) -> None:
        """Every fabricated profile is byte-identical to how it started.

        The preserved baseline has no second copy and `map/` has no backup at all, so this is
        asserted after every operation rather than once at the end.
        """
        self.assertEqual(self.snapshot(), self.pristine_state)

    def run_script(self, name: str, *arguments: str) -> subprocess.CompletedProcess:
        environment = dict(os.environ)
        environment["LOM_APPLICATIONS_DIR"] = str(self.applications)
        environment["LOM_ARTIFACTS_DIR"] = str(self.artifacts)
        return subprocess.run(
            [str(PROJECT_DIR / "scripts" / name), *arguments],
            capture_output=True,
            text=True,
            env=environment,
            cwd=PROJECT_DIR,
        )

    @property
    def dev_root(self) -> Path:
        return self.applications / DEV_PROFILE_NAME

    @property
    def metadata(self) -> Path:
        return self.dev_root / ".lom-pipeline"

    def create_profile(self, *extra: str) -> subprocess.CompletedProcess:
        return self.run_script("install-dev.sh", "--create-profile", *extra)

    def fabricate_build(self, mod_id: str, build_id: str, contents: bytes) -> Path:
        build_dir = self.artifacts / "build" / mod_id / build_id
        build_dir.mkdir(parents=True)
        digests = {}
        for archive in ARCHIVES:
            path = build_dir / archive
            path.write_bytes(contents + archive.encode())
            digests[archive] = digest(path)
        (build_dir / "build.json").write_text(
            json.dumps({"build_id": build_id, "output_archive_digests": digests}),
            encoding="utf-8",
        )
        return build_dir


class ProfileCreationTest(PipelineTestCase):
    def test_creation_produces_a_profile_a_manifest_and_a_pristine_copy(self) -> None:
        result = self.create_profile()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_other_profiles_untouched()

        self.assertTrue(self.dev_root.is_dir())
        self.assertTrue((self.metadata / "MANIFEST.sha256").is_file())
        self.assertTrue((self.metadata / "PROFILE.json").is_file())
        self.assertTrue((self.metadata / "INSTALLS.tsv").is_file())

        baseline_game_dir = self.applications / BASELINE / GAME_SUBPATH
        for archive in ARCHIVES:
            expected = digest(baseline_game_dir / archive)
            self.assertEqual(digest(self.dev_root / GAME_SUBPATH / archive), expected)
            self.assertEqual(digest(self.metadata / "pristine" / archive), expected)
            self.assertIn(expected, (self.metadata / "MANIFEST.sha256").read_text())

    def test_the_manifest_is_an_independent_record_of_the_baseline_hashes(self) -> None:
        """Not a record of what was copied: a record of what the baseline held.

        `scripts/lib-game-archives.sh` sets out why. A restore checked against the file it was
        copied from would certify a pristine copy that had itself been overwritten.
        """
        self.create_profile()
        recorded = dict(
            reversed(line.split(maxsplit=1))
            for line in (self.metadata / "MANIFEST.sha256").read_text().splitlines()
            if line
        )
        for archive in ARCHIVES:
            self.assertEqual(
                recorded[f"pristine/{archive}"],
                digest(self.applications / BASELINE / GAME_SUBPATH / archive),
            )

    def test_creating_a_second_time_is_refused(self) -> None:
        self.assertEqual(self.create_profile().returncode, 0)
        marker = self.dev_root / "marker.txt"
        marker.write_text("do not lose me", encoding="utf-8")

        result = self.create_profile()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("already exists", result.stderr)
        self.assertTrue(marker.is_file())
        self.assert_other_profiles_untouched()

    def test_recreate_replaces_it_and_still_leaves_the_others_alone(self) -> None:
        self.create_profile()
        (self.dev_root / "marker.txt").write_text("gone", encoding="utf-8")

        result = self.create_profile("--recreate")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.dev_root / "marker.txt").exists())
        self.assert_other_profiles_untouched()

    def test_a_missing_baseline_is_refused(self) -> None:
        shutil.rmtree(self.applications / BASELINE)
        result = self.create_profile()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("baseline profile not found", result.stderr)


class InstallTest(PipelineTestCase):
    def test_installing_without_a_profile_is_refused(self) -> None:
        self.fabricate_build("example", "abc123", b"new ")
        result = self.run_script("install-dev.sh", "example", "abc123")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no development profile", result.stderr)
        self.assert_other_profiles_untouched()

    def test_installing_a_build_that_does_not_exist_is_refused(self) -> None:
        self.create_profile()
        result = self.run_script("install-dev.sh", "example", "nosuch")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no such build", result.stderr)
        self.assert_other_profiles_untouched()

    def test_a_build_whose_bytes_disagree_with_its_own_build_json_is_refused(self) -> None:
        self.create_profile()
        build_dir = self.fabricate_build("example", "abc123", b"new ")
        (build_dir / "gs.mpq").write_bytes(b"tampered")

        before = digest(self.dev_root / GAME_SUBPATH / "gs.mpq")
        result = self.run_script("install-dev.sh", "example", "abc123")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("BUILD CORRUPT", result.stderr)
        # Preflighted before anything is written, so nothing is half-installed.
        self.assertEqual(digest(self.dev_root / GAME_SUBPATH / "gs.mpq"), before)
        self.assert_other_profiles_untouched()

    def test_a_successful_install_matches_the_recorded_digests_and_logs_itself(self) -> None:
        self.create_profile()
        build_dir = self.fabricate_build("example", "abc123", b"new ")

        result = self.run_script("install-dev.sh", "example", "abc123")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_other_profiles_untouched()

        for archive in ARCHIVES:
            self.assertEqual(
                digest(self.dev_root / GAME_SUBPATH / archive), digest(build_dir / archive)
            )
        log = (self.metadata / "INSTALLS.tsv").read_text()
        self.assertIn("abc123", log)
        self.assertEqual(len(log.strip().splitlines()), 1 + len(ARCHIVES))


class RestoreTest(PipelineTestCase):
    def install_something(self) -> Path:
        self.create_profile()
        build_dir = self.fabricate_build("example", "abc123", b"new ")
        self.assertEqual(
            self.run_script("install-dev.sh", "example", "abc123").returncode, 0
        )
        return build_dir

    def test_restoring_pristine_undoes_an_install(self) -> None:
        self.install_something()
        baseline_game_dir = self.applications / BASELINE / GAME_SUBPATH

        result = self.run_script("restore-dev.sh")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_other_profiles_untouched()
        for archive in ARCHIVES:
            self.assertEqual(
                digest(self.dev_root / GAME_SUBPATH / archive),
                digest(baseline_game_dir / archive),
            )

    def test_restoring_prints_the_hashes_it_produced(self) -> None:
        """Every script in this repository ends by printing what it made."""
        self.install_something()
        result = self.run_script("restore-dev.sh")
        pristine = digest(self.metadata / "pristine" / "gs.mpq")
        self.assertIn(pristine, result.stdout)

    def test_restoring_to_a_build_restores_that_build(self) -> None:
        self.install_something()
        second = self.fabricate_build("example", "def456", b"second ")
        self.run_script("install-dev.sh", "example", "def456")

        result = self.run_script("restore-dev.sh", "--to", "example", "abc123")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_other_profiles_untouched()
        first = self.artifacts / "build" / "example" / "abc123"
        for archive in ARCHIVES:
            self.assertEqual(
                digest(self.dev_root / GAME_SUBPATH / archive), digest(first / archive)
            )
        self.assertNotEqual(
            digest(self.dev_root / GAME_SUBPATH / "gs.mpq"), digest(second / "gs.mpq")
        )

    def test_a_corrupted_pristine_copy_is_refused_before_anything_is_written(self) -> None:
        """The reason the manifest is independent: this is the case it exists to catch."""
        build_dir = self.install_something()
        (self.metadata / "pristine" / "gs.mpq").write_bytes(b"silently overwritten")

        result = self.run_script("restore-dev.sh")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SOURCE CORRUPT", result.stderr)
        # The live archive is still the installed build, not a half-restore.
        self.assertEqual(
            digest(self.dev_root / GAME_SUBPATH / "gs.mpq"), digest(build_dir / "gs.mpq")
        )
        self.assert_other_profiles_untouched()

    def test_restoring_without_a_profile_is_refused(self) -> None:
        result = self.run_script("restore-dev.sh")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no development profile", result.stderr)
        self.assert_other_profiles_untouched()

    def test_restoring_a_profile_this_pipeline_did_not_create_is_refused(self) -> None:
        (self.dev_root / GAME_SUBPATH).mkdir(parents=True)
        result = self.run_script("restore-dev.sh")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no pristine manifest", result.stderr)

    def test_restoring_to_a_build_that_does_not_exist_is_refused(self) -> None:
        self.install_something()
        result = self.run_script("restore-dev.sh", "--to", "example", "nosuch")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no such build", result.stderr)
        self.assert_other_profiles_untouched()


class ProfileTableTest(unittest.TestCase):
    def test_the_bash_and_python_profile_tables_agree(self) -> None:
        """`scripts/lib-mod-pipeline.sh` duplicates `mod_tree.PROFILE_APPS`. Keep them equal."""
        for label, app in PROFILE_APPS.items():
            result = subprocess.run(
                [
                    "bash",
                    "-c",
                    f'source "{PROJECT_DIR}/scripts/lib-mod-pipeline.sh"; profile_app {label}',
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), app)

    def test_an_unknown_profile_label_is_refused_by_bash_too(self) -> None:
        result = subprocess.run(
            [
                "bash",
                "-c",
                f'source "{PROJECT_DIR}/scripts/lib-mod-pipeline.sh"; profile_app development',
            ],
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown profile label", result.stderr)


class EngineAcceptanceCaveatTest(unittest.TestCase):
    """`build.json` carries its own engine-acceptance caveat, and it went stale once already.

    Until 2026-09-18 every build asserted that no rewritten `pic.mpq` had faced the engine and that
    its compression choice was Inferred. Both had been refuted, in this repository's own roadmap,
    and nothing caught it because the claim lives in code and its evidence lives in prose.

    The first version of these tests checked that five phrases -- "ONE member", "REPLACED",
    "size-changing edit", "added member", "encoder" -- appeared somewhere in the caveat, and passed
    happily on a caveat that said the engine HAD accepted all of them. That is the direction that
    does damage: the stale text it was written against merely understated what had been proved,
    while its inverse would ship a build claiming acceptance for a size-changing edit and the
    ByteRun1 encoder, neither of which has ever run. These tests assert the sentence rather than
    its vocabulary, and take the limits from the roadmap at runtime rather than restating them.
    """

    NEGATION = "NOT established:"

    def roadmap(self) -> str:
        return (PROJECT_DIR / "docs" / "roadmap.md").read_text(encoding="utf-8")

    def roadmap_limits_paragraph(self) -> str:
        """The roadmap's own `Not established.` paragraph, which is the measured scope."""
        roadmap = self.roadmap()
        marker = "**Not established.**"
        self.assertIn(
            marker,
            roadmap,
            "the roadmap no longer states the limits of the pic.mpq run; the build caveat has "
            "nothing left to agree with",
        )
        start = roadmap.index(marker)
        return roadmap[start : roadmap.index("\n\n", start)]

    def caveat_negation_clause(self) -> str:
        """The single clause of the caveat that states what has NOT been established."""
        caveat = ENGINE_ACCEPTANCE["pic.mpq"]
        self.assertEqual(
            caveat.count("established"),
            1,
            "the caveat should make exactly one establishment claim, and it is a negative one",
        )
        self.assertIn(self.NEGATION, caveat)
        self.assertNotIn("ALSO established", caveat)
        return caveat[caveat.index(self.NEGATION) :].split(". ")[0]

    def test_the_pic_caveat_agrees_with_the_roadmap_about_acceptance(self) -> None:
        roadmap = self.roadmap()
        accepted = (
            "- [x] Put a rewritten `pic.mpq` in front of the engine. **Observed in gameplay "
            "2026-09-18**"
        )
        self.assertIn(
            accepted,
            roadmap,
            "the roadmap no longer records pic.mpq acceptance on that date; the build caveat has "
            "to follow it rather than lead it",
        )
        caveat = ENGINE_ACCEPTANCE["pic.mpq"]
        self.assertNotIn("Never tested", caveat)
        self.assertIn("Observed in gameplay 2026-09-18", caveat)

    def test_the_pic_caveat_negates_every_limit_the_roadmap_negates(self) -> None:
        """Polarity, not vocabulary: each limit has to sit inside the caveat's negative clause."""
        paragraph = self.roadmap_limits_paragraph()
        for phrase in (
            "An edit that changes a member's *size* has not been put in front of the",
            "neither has an added member",
            "replaced rather than added",
        ):
            with self.subTest(roadmap=phrase):
                self.assertIn(phrase, paragraph)

        clause = self.caveat_negation_clause()
        for limit in ("size-changing edit", "added member", "ByteRun1 encoder"):
            with self.subTest(limit=limit):
                self.assertIn(limit, clause)
        self.assertIn(
            "have none of them been put in front of the engine",
            clause,
            "the limits have to be negated in the caveat, not merely mentioned in it",
        )

    def test_the_positive_half_of_the_pic_caveat_stays_as_narrow_as_the_run(self) -> None:
        caveat = ENGINE_ACCEPTANCE["pic.mpq"]
        claimed = caveat[: caveat.index(self.NEGATION)]
        for narrowing in ("ONE member", "REPLACED", "length-preservingly"):
            with self.subTest(narrowing=narrowing):
                self.assertIn(narrowing, claimed)
        for widening in ("every", "all members", "any member", "archives"):
            with self.subTest(widening=widening):
                self.assertNotIn(widening, claimed)

    def test_the_gs_caveat_still_matches_the_run_it_cites(self) -> None:
        caveat = ENGINE_ACCEPTANCE["gs.mpq"]
        self.assertIn("2026-09-16", caveat)
        self.assertIn("MPQ_FILE_IMPLODE", caveat)
        self.assertIn("once", caveat)
        self.assertIn(
            "attended 2026-09-16 round trip of an `MPQ_FILE_IMPLODE` member of",
            (PROJECT_DIR / "docs" / "build-pipeline.md").read_text(encoding="utf-8"),
        )


if __name__ == "__main__":
    unittest.main()
