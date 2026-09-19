"""install-dev and restore-dev, run for real against a fabricated `Applications` directory.

These are not unit tests of a Python function; they run the shell scripts, with
`LOM_APPLICATIONS_DIR` and `LOM_ARTIFACTS_DIR` pointed at a temporary tree. The archives are a few
bytes of nonsense rather than real MPQs, because nothing under test opens one: creating a profile,
approving a path, hashing a file and verifying a restore are all archive-agnostic.

**Nothing here reads or writes anything under `~/Applications`.** The most important assertion in
the file is `assert_other_profiles_untouched`, which re-hashes every fabricated profile after every
operation. It is called by every test that writes anything.
"""

import ast
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from dataclasses import replace
from pathlib import Path

from tools.engine_acceptance import (
    ACCEPTANCE,
    ArchiveAcceptance,
    Disposition,
    EditKind,
    EngineRun,
    build_metadata,
    roadmap_paragraph,
)
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
    """What the engine has accepted is data, and these tests assert the data.

    The history matters more than the tests do. The caveat `build.json` carries was a hand-written
    sentence; it went stale, claiming no rewritten `pic.mpq` had faced the engine after one had.
    The repair was a test that grepped that sentence, and review then broke it three times running:
    `NOT established` became `ALSO established`; then `have none of them` became `have every one of
    them`; then a new sentence was appended after the clause being asserted -- "In fact, the engine
    accepted a size-changing edit, an added member, and the full ByteRun1 encoder" -- and every
    assertion stayed green. Three bypasses, one cause: a finite set of assertions about prose
    cannot constrain the open set of sentences prose can be.

    So the sentence is no longer written. `tools/engine_acceptance.py` holds the facts, every
    sentence is rendered from them, and these tests assert the facts and the rendering. A false
    claim is now unrepresentable rather than un-greppable: the limits are *derived* from what the
    run was, so saying the engine accepted a size-changing edit means saying the run was
    size-changing, which changes the rendered roadmap paragraph, which stops matching
    `docs/roadmap.md`, which fails here.
    """

    def roadmap(self) -> str:
        return (PROJECT_DIR / "docs" / "roadmap.md").read_text(encoding="utf-8")

    @staticmethod
    def flatten(text: str) -> str:
        """Compare wording, not line breaks: the doc is wrapped and the renderer is not."""
        return " ".join(text.split())

    def test_the_pic_run_is_recorded_as_the_narrow_thing_it_was(self) -> None:
        run = ACCEPTANCE["pic.mpq"].run
        self.assertIsNotNone(run)
        self.assertEqual(run.date, "2026-09-18")
        self.assertEqual(run.members, 1)
        self.assertIs(run.disposition, Disposition.REPLACED)
        self.assertIs(run.edit_kind, EditKind.LENGTH_PRESERVING)
        self.assertEqual(run.mechanism, "tools/pbm_patch.py")

    def test_the_limits_are_derived_from_the_run_rather_than_typed_beside_it(self) -> None:
        """The property that makes the false claim unrepresentable, asserted directly.

        A run that was length-preserving *implies* that a size-changing edit is untested, and a
        run that replaced a member implies that an added one is. Nobody can delete those limits
        while leaving the run describing what it describes.
        """
        run = ACCEPTANCE["pic.mpq"].run
        self.assertIn("an edit that changes a member's size", run.derived_limits)
        self.assertIn("a member added to an archive rather than replaced", run.derived_limits)

        widened = replace(run, edit_kind=EditKind.SIZE_CHANGING, disposition=Disposition.ADDED)
        self.assertNotIn("an edit that changes a member's size", widened.derived_limits)
        self.assertNotIn(
            "a member added to an archive rather than replaced", widened.derived_limits
        )

    def test_the_roadmap_paragraph_is_the_rendered_one(self) -> None:
        """The build and the doc cannot drift apart, because both are printed from one record."""
        self.assertIn(
            self.flatten(roadmap_paragraph("pic.mpq")),
            self.flatten(self.roadmap()),
            "docs/roadmap.md no longer matches tools/engine_acceptance.py. Whichever moved, the "
            "facts are the source: change them there and paste what roadmap_paragraph prints.",
        )

    def test_the_roadmap_still_records_the_acceptance_the_facts_claim(self) -> None:
        run = ACCEPTANCE["pic.mpq"].run
        self.assertIn(
            "- [x] Put a rewritten `pic.mpq` in front of the engine. **Observed in gameplay "
            f"{run.date}**",
            self.roadmap(),
            "the facts claim an accepted pic.mpq run on that date and the roadmap does not record "
            "it; the build caveat follows the roadmap rather than leading it",
        )

    def test_the_build_metadata_carries_the_structure_and_not_only_the_sentence(self) -> None:
        metadata = build_metadata()
        self.assertEqual(sorted(metadata), sorted(ACCEPTANCE))
        pic = metadata["pic.mpq"]
        self.assertEqual(pic["summary"], ACCEPTANCE["pic.mpq"].summary())
        self.assertEqual(pic["established"]["edit_kind"], "length_preserving")
        self.assertEqual(pic["established"]["disposition"], "replaced")
        self.assertEqual(pic["not_established"], list(ACCEPTANCE["pic.mpq"].not_established))
        for archive in ("imp.mpq", "sndfx.mpq", "special.mpq"):
            with self.subTest(archive=archive):
                self.assertIsNone(metadata[archive]["established"])
                self.assertIn("Never tested", metadata[archive]["summary"])

    def test_every_observation_is_a_sentence_the_documentation_already_carries(self) -> None:
        """The last free-text field, tied to prose a human reviewed.

        `observation` is the only sentence in the record, and free text is where a widened claim
        hides -- a reviewer changed it to "Each of the 1,071 members was re-encoded and accepted"
        and every structural assertion passed. Two things stop that now: `EngineRun` refuses a
        number the run does not record, and this test requires the sentence to appear in the
        documentation, so widening it means also writing the wider claim where a reader will see
        it.
        """
        prose = self.flatten(
            " ".join(
                (PROJECT_DIR / "docs" / name).read_text(encoding="utf-8")
                for name in ("roadmap.md", "build-pipeline.md")
            )
        ).replace("`", "")
        for name, acceptance in sorted(ACCEPTANCE.items()):
            if acceptance.run is None:
                continue
            with self.subTest(archive=name):
                self.assertIn(
                    self.flatten(acceptance.run.observation).rstrip(".").lower(),
                    prose.lower(),
                    "an observation has to be a claim the documentation makes too",
                )

    def test_an_observation_may_not_carry_a_quantity_the_run_does_not_record(self) -> None:
        with self.assertRaises(ValueError):
            EngineRun(
                date="2026-09-18",
                members=1,
                disposition=Disposition.REPLACED,
                edit_kind=EditKind.LENGTH_PRESERVING,
                mechanism="tools/pbm_patch.py",
                observation="Each of the 1,071 members was re-encoded and accepted.",
            )
        # And the other direction: the run's own count and its date are quantities it records, so
        # a sentence citing them is allowed. Without this the rule could be narrowed to forbid
        # every number and no test would notice.
        EngineRun(
            date="2026-09-18",
            members=3,
            disposition=Disposition.REPLACED,
            edit_kind=EditKind.LENGTH_PRESERVING,
            mechanism="tools/pbm_patch.py",
            observation="The engine read 3 members on 2026-09-18.",
        )

    def test_the_summary_is_nothing_but_its_facts(self) -> None:
        """Rebuild every sentence from the record and demand equality. Deliberately brittle.

        Structure alone does not stop a renderer from appending a claim no field holds -- a
        reviewer demonstrated exactly that, with "In fact, the engine accepted a size-changing
        edit, an added member, and the full ByteRun1 encoder" added inside `summary()`. Asserting
        *properties* of the output cannot catch that, because the output is prose again by the time
        it is a string. So this reconstructs the string from the fields and compares it, which
        means a reflow of the template fails here and a human re-approves it. Brittle and loud
        beats permissive and quiet for a claim about what the engine has accepted.
        """
        for name, acceptance in sorted(ACCEPTANCE.items()):
            with self.subTest(archive=name):
                limits = "; ".join(acceptance.not_established)
                if acceptance.run is None:
                    expected = (
                        f"Never tested. No {name} this pipeline wrote has been put in front of "
                        f"the engine. Not established: {limits}."
                    )
                else:
                    run = acceptance.run
                    expected = (
                        f"Observed {run.date}, once: {run.members} member of {name}, "
                        f"{run.disposition.value}, with a {run.edit_kind.value} edit made by "
                        f"{run.mechanism}. {run.observation} Not established: {limits}."
                    )
                    if acceptance.storage_class:
                        expected += f" {acceptance.storage_class}"
                self.assertEqual(acceptance.summary(), expected)

    def test_the_build_writes_the_rendered_facts_unmodified(self) -> None:
        """The build's own dict has to be `build_metadata()` and not a transformation of it.

        Checked through the syntax tree rather than the text, because the bypass to catch is a
        wrapper -- a comprehension appending a sentence to every summary on the way into
        `build.json` passes every test that only looks at `engine_acceptance`'s own output.
        """
        source = (PROJECT_DIR / "tools" / "mod_build.py").read_text(encoding="utf-8")
        values = [
            value
            for node in ast.walk(ast.parse(source))
            if isinstance(node, ast.Dict)
            for key, value in zip(node.keys, node.values)
            if isinstance(key, ast.Constant) and key.value == "engine_acceptance"
        ]
        self.assertEqual(len(values), 1, "build.json should record engine acceptance exactly once")
        call = values[0]
        self.assertIsInstance(call, ast.Call, "the build must write the rendered facts, unwrapped")
        self.assertEqual(ast.unparse(call), "engine_acceptance.build_metadata()")

    def test_no_engine_acceptance_prose_is_written_by_hand_anywhere_else(self) -> None:
        """The regression that would undo all of this is someone pasting a sentence back in."""
        source = (PROJECT_DIR / "tools" / "mod_build.py").read_text(encoding="utf-8")
        for line in source.splitlines():
            if line.lstrip().startswith("#"):
                continue
            self.assertNotIn(
                "Not established",
                line,
                "engine-acceptance prose belongs in tools/engine_acceptance.py, rendered",
            )

    def test_a_run_that_cannot_have_happened_is_refused(self) -> None:
        with self.assertRaises(ValueError):
            EngineRun(
                date="2026-09-18",
                members=0,
                disposition=Disposition.REPLACED,
                edit_kind=EditKind.LENGTH_PRESERVING,
                mechanism="x",
                observation="y",
            )
        with self.assertRaises(ValueError):
            EngineRun(
                date="last Tuesday",
                members=1,
                disposition=Disposition.REPLACED,
                edit_kind=EditKind.LENGTH_PRESERVING,
                mechanism="x",
                observation="y",
            )
        with self.assertRaises(ValueError):
            ArchiveAcceptance(archive="imp.mpq", run=None)

    def test_the_gs_run_still_matches_the_prose_that_cites_it(self) -> None:
        run = ACCEPTANCE["gs.mpq"].run
        self.assertEqual(run.date, "2026-09-16")
        self.assertIn(
            f"attended {run.date} round trip of an `MPQ_FILE_IMPLODE` member of",
            (PROJECT_DIR / "docs" / "build-pipeline.md").read_text(encoding="utf-8"),
        )
        self.assertIn("0x80010100", ACCEPTANCE["gs.mpq"].storage_class)


if __name__ == "__main__":
    unittest.main()
