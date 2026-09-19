"""install-dev and restore-dev, run for real against a fabricated `Applications` directory.

These are not unit tests of a Python function; they run the shell scripts, with
`LOM_APPLICATIONS_DIR` and `LOM_ARTIFACTS_DIR` pointed at a temporary tree. The archives are a few
bytes of nonsense rather than real MPQs, because nothing under test opens one: creating a profile,
approving a path, hashing a file and verifying a restore are all archive-agnostic.

**Nothing here reads or writes anything under `~/Applications`.** The most important assertion in
the file is `assert_other_profiles_untouched`, which re-hashes every fabricated profile after every
operation. It is called by every test that writes anything.
"""

import contextlib
import hashlib
import io
import json
import os
import re
import shutil
import subprocess
import tempfile
import time
import unittest
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace

from tools.engine_acceptance import (
    ACCEPTANCE,
    ArchiveAcceptance,
    Disposition,
    EditKind,
    EngineRun,
    Mechanism,
    StorageClass,
    build_metadata,
    roadmap_paragraph,
    roadmap_region,
)
from tools.mod_build import command_report
from tools.mpq_shape import MANIFEST_COLUMNS
from tools.mod_tree import GAME_SUBPATH, PROFILE_APPS

PROJECT_DIR = Path(__file__).resolve().parent.parent
DEV_PROFILE_NAME = "Lords of Magic Development.app"
GAME_IS_UP = "lomse.exe is running"
BASELINE = PROFILE_APPS["vanilla"]
ARCHIVES = ("gs.mpq", "pic.mpq")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _game_pattern() -> str:
    r"""The guard's regex, derived from `GAME_SUBPATH` exactly as the shell does.

    Derived rather than written out, because two hand-maintained copies of a safety pattern drift
    and the drift is silent. `test_the_shell_and_python_patterns_are_the_same` asserts they agree.
    """
    tail = GAME_SUBPATH.split("drive_c/", 1)[1]
    escaped = "".join(f"[{c}]" if c in "[](){}.*+?^$|" else c for c in tail)
    escaped = escaped.replace("/", r"[\\]")
    # The directory group is OPTIONAL: the 3.02 profile runs `d:\lomse.exe` from the drive root
    # while Development runs the full path. A pattern written from either profile alone misses the
    # other, which is this guard's entire defect history.
    return rf"^[A-Za-z]:[\\]({escaped}[\\])?lomse[.]exe([[:space:]]|$)"


GAME_PATTERN = _game_pattern()
BROAD_PATTERN = r"lomse\.exe"


def matching_processes(pattern: str) -> list[str]:
    """`pid command` for every process whose command line matches, newest first.

    Named rather than counted, because the whole problem here is that a match is not evidence of
    what matched.
    """
    try:
        found = subprocess.run(
            ["pgrep", "-i", "-f", pattern], capture_output=True, check=False, text=True
        )
    except OSError:
        # No pgrep: assume clear rather than skip the suite on a machine that cannot answer.
        return []
    described = []
    for pid in found.stdout.split():
        listed = subprocess.run(
            ["ps", "-p", pid, "-o", "pid=,command="],
            capture_output=True,
            check=False,
            text=True,
        )
        described.append(" ".join(listed.stdout.split()) or pid)
    return described


def game_processes() -> list[str]:
    r"""Processes that are the running game, as distinct from processes that mention it.

    **The pattern is anchored on purpose**, and matches `scripts/lib-mod-pipeline.sh`'s. `pgrep -f`
    matches the whole command line, so the obvious pattern -- `lomse.exe` -- answers "does anything
    mention this name?" rather than "is the game running?". It matches this project's own tools,
    which take that path as an argument (`engine_probe.py`, `dumpva`, the save survey, the
    corpus-gated disassembly test), and it matches any agent, editor or shell whose command line
    quotes the name. Three agents tripped it in one day, and writing the fix tripped it: a comment
    in the patch command contained `d:\lomse.exe`.

    Merely requiring the backslash form is not enough for the same reason -- it still matches
    anything that *quotes* a Wine path. The game's own command line **begins** with a DOS drive
    path, so the pattern is anchored to a leading drive letter.

    **The executable is not at the drive root, and assuming it was cost this guard its whole
    purpose.** The first version of this pattern read `^[A-Za-z]:[\\]lomse[.]exe`, which can only
    match a command line where `lomse.exe` follows the drive letter immediately. The real one,
    observed 2026-09-19 against the live process, PID 77245, is:

        c:\program files (x86)\steam\steamapps\common\lords of magic special edition\english\lomse.exe /* MVK_CONFIG_FULL_IMAGE_VIEW_SWIZZLE=1

    So the guard matched nothing for a full day, and `install-dev.sh` would have overwritten
    archives under a live process -- the exact corruption it exists to prevent. It was "verified in
    both directions" at the time, but the decoy was `d:\lomse.exe`, written from the same wrong
    assumption as the pattern: a fixture shaped like the belief under test cannot refute it.
    `GameGuardPattern` below now spawns the command line above, verbatim.

    **No pattern is sufficient on its own**, which is why `run_script` retries rather than trusting
    this. Sampling `ps` continuously through a failing run caught **zero** matching command lines,
    so some matches are processes that exit within milliseconds -- anchoring shrinks the
    false-positive population but cannot win a race against a process that is already gone.

    The stakes are the skip, not the tidiness. A false positive here would *skip* the install,
    profile-creation and restore coverage, silently, exactly while the corpus tooling runs. A guard
    that cannot be wrong loudly reports safety it has not earned, which is the failure this file's
    neighbours have spent several rounds removing.
    """
    return matching_processes(GAME_PATTERN)


# The command line the game really presents, observed 2026-09-19 against the live process, PID
# 77245, while rung 7 of the engine-acceptance ladder was being listened to. Copied verbatim: the
# point of these tests is that a decoy invented from what we expected to see proved nothing.
OBSERVED_GAME_ARGV0 = (
    r"c:\program files (x86)\steam\steamapps\common"
    r"\lords of magic special edition\english\lomse.exe"
)

# The SAME game, from a different profile, observed live on 2026-09-19 as PID 47723: the 3.02
# profile runs from the DRIVE ROOT. Two profiles, two command lines, and every previous version of
# this guard was written against exactly one of them.
OBSERVED_GAME_ARGV0_DRIVE_ROOT = r"d:\lomse.exe"

# Not observed -- constructed. Windows paths and executable names are case-insensitive and a
# Wineskin profile may sit on any drive, so this is a command line the guard CLAIMS to cover.
# It is the single fixture that pins both `-i` and the drive-letter class: without `-i` the
# upper-case name misses, and with `[CDcd]` or any narrowed class the `e:` misses.
UPPERCASE_OTHER_DRIVE_ARGV0 = r"E:\LOMSE.EXE"

# What the guard said before 2026-09-19. Kept so the defect has a test that fails if it returns,
# rather than a comment saying it used to be there.
# What the guard said before 2026-09-19, and what it briefly said after. Both are kept so each
# defect has a test that fails if it returns, rather than a comment saying it used to be there.
RETIRED_PATTERN = r"^[A-Za-z]:[\\]lomse[.]exe"
# The first fix. It matched the real game -- and also anything whose command line merely mentions
# the executable, which is the false-positive class the anchor exists to prevent.
OVERBROAD_PATTERN = r"^[A-Za-z]:[\\].*lomse[.]exe"


@contextlib.contextmanager
def decoy_process(argv0: str):
    """Run a harmless process presenting `argv0` as its command line, and wait until `ps` shows it.

    `exec -a` is the only portable way to put an arbitrary string, spaces and backslashes included,
    where `pgrep -f` will read it. The process is `sleep`, so a leak costs nothing and dies on its
    own; it is killed and reaped here regardless.

    Waiting is not politeness. Spawning is asynchronous, and asserting "no match" against a process
    that has not appeared yet passes for the wrong reason -- the failure mode that makes a negative
    result worthless.
    """
    spawned = subprocess.Popen(
        ["bash", "-c", 'exec -a "$1" sleep 30', "decoy", argv0],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline:
            listed = subprocess.run(
                ["ps", "-p", str(spawned.pid), "-o", "command="],
                capture_output=True,
                check=False,
                text=True,
            )
            if argv0[:40] in listed.stdout:
                break
            time.sleep(0.05)
        else:
            raise AssertionError(
                f"the decoy never presented its command line to ps: {argv0!r}"
            )
        yield spawned
    finally:
        spawned.kill()
        spawned.wait()


class GameGuardPattern(unittest.TestCase):
    r"""Does the guard fire on the command line the game actually has?

    It did not. From 2026-09-18 to 2026-09-19 `refuse_if_game_running` matched nothing at all,
    because its pattern required `lomse.exe` to sit immediately after the drive letter and the
    executable is six directories down. `install-dev.sh` advertises that it "refuses while
    lomse.exe is running" and would instead have swapped archives under the live process.

    It had been checked in both directions and passed both. The decoy was `d:\lomse.exe` -- built
    from the same assumption as the pattern, so the check could only ever agree with it. These
    tests use the observed string instead, and drive the **shipped** shell function rather than a
    copy of the regex, so a future edit to `scripts/lib-mod-pipeline.sh` alone cannot pass them.
    """

    def invoke_shipped_guard(self) -> subprocess.CompletedProcess:
        return subprocess.run(
            [
                "bash",
                "-c",
                'source "$1"/scripts/lib-mod-pipeline.sh; refuse_if_game_running',
                "bash",
                str(PROJECT_DIR),
            ],
            capture_output=True,
            check=False,
            text=True,
        )

    def skip_if_the_real_game_is_running(self) -> None:
        """A `quiet` assertion cannot be read while the game is actually up.

        The guard refuses for a correct reason then, and the test would fail claiming a false
        positive. Found by running this suite during an attended session: two tests went red
        because the guard was doing its job.
        """
        running = game_processes()
        if running:
            raise unittest.SkipTest(f"the game is running, so a quiet guard is not expected: {running}")

    def test_the_shipped_guard_refuses_while_the_real_command_line_is_up(self) -> None:
        with decoy_process(OBSERVED_GAME_ARGV0):
            completed = self.invoke_shipped_guard()
        self.assertNotEqual(completed.returncode, 0, completed.stderr)
        self.assertIn(GAME_IS_UP, completed.stderr)

    def test_the_shipped_guard_is_quiet_when_a_process_merely_mentions_the_path(self) -> None:
        """The anchor's whole job. This project's own tools take that path as an argument."""
        self.skip_if_the_real_game_is_running()
        with decoy_process(f"grep --fixed-strings {OBSERVED_GAME_ARGV0}"):
            completed = self.invoke_shipped_guard()
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertNotIn(GAME_IS_UP, completed.stderr)

    def test_the_shipped_guard_refuses_for_the_drive_root_profile_too(self) -> None:
        r"""The 3.02 profile runs `d:\lomse.exe`, and a guard that misses it is a guard.

        Both observed command lines are the same game. Development runs the full path; 3.02 runs
        from the drive root. Each previous version of this pattern was written against exactly one
        profile and silently failed on the other, which is why both are tested here rather than
        whichever one was in front of us last.
        """
        with decoy_process(OBSERVED_GAME_ARGV0_DRIVE_ROOT):
            completed = self.invoke_shipped_guard()
        self.assertNotEqual(completed.returncode, 0, completed.stderr)
        self.assertIn(GAME_IS_UP, completed.stderr)

    def test_the_shipped_guard_covers_the_whole_drive_letter_range_and_case(self) -> None:
        r"""`E:\LOMSE.EXE` -- the one fixture that pins both `-i` and `[A-Za-z]`.

        Two independent Codex runs found the same gap: every other fixture here drives `c:` or
        `d:` in lower case, so narrowing the class to `[CDcd]`, or widening it to `[A-z]`, or
        dropping `-i` altogether, left all ten tests green while the guard stopped covering a
        profile it claims to cover. Windows paths and executable names are case-insensitive and a
        Wineskin profile can sit on any drive.

        The comment that came with `-i` had the justification backwards: observing a lower-case
        command line is not a reason for `-i`, because `[A-Za-z]` and a lower-case literal already
        match that. Protecting against case VARIATION is the reason.
        """
        with decoy_process(UPPERCASE_OTHER_DRIVE_ARGV0):
            completed = self.invoke_shipped_guard()
        self.assertNotEqual(completed.returncode, 0, completed.stderr)
        self.assertIn(GAME_IS_UP, completed.stderr)

    def test_the_shipped_guard_is_quiet_while_a_drive_anchored_decoy_runs(self) -> None:
        r"""Drives the SHIPPED shell function against a false positive, not just the regex.

        This exists because a mutation survived without it. Reverting the call site to the
        over-broad `^[A-Za-z]:[\\].*lomse[.]exe` left every other test in this class green: the
        pattern-agreement test compares `game_command_pattern`'s OUTPUT, which that mutation does
        not touch, and the decoy in the other quiet test begins with a program name, so the anchor
        excludes it under either pattern.

        `c:\tools\notlomse.exe` is the discriminating case. It begins with a drive letter, so the
        anchor admits it, and only the full-path requirement rejects it.
        """
        self.skip_if_the_real_game_is_running()
        with decoy_process(r"c:\tools\notlomse.exe"):
            completed = self.invoke_shipped_guard()
        self.assertEqual(
            completed.returncode,
            0,
            "the guard refused while a process that is NOT the game was running; a false positive "
            f"here silently skips install and restore coverage. stderr: {completed.stderr}",
        )
        self.assertNotIn(GAME_IS_UP, completed.stderr)

    def test_the_retired_pattern_could_not_have_matched_the_real_command_line(self) -> None:
        """The defect, pinned. If this ever passes a match, the old pattern is back."""
        self.assertIsNone(
            re.match(RETIRED_PATTERN, OBSERVED_GAME_ARGV0, re.IGNORECASE),
            "the retired pattern matched the observed command line, so this test no longer "
            "describes the defect it was written for",
        )
        self.assertIsNotNone(
            re.match(GAME_PATTERN, OBSERVED_GAME_ARGV0, re.IGNORECASE),
            f"{GAME_PATTERN!r} does not match the command line the game really has",
        )

    # Command lines that are NOT the game, each one a false positive the guard must not produce.
    # A false positive SKIPS this file's install, profile-creation and restore coverage silently,
    # which is worse than the red it would replace.
    NOT_THE_GAME = (
        r"c:\tools\notlomse.exe",
        r"d:\notlomse.exe",
        # `[A-z]` is `[A-Za-z]` plus the six ASCII characters between `Z` and `a` -- [ \ ] ^ _ `
        # -- and every test here drove `c:` or `d:`, so that widening passed all ten. A drive
        # letter that is not a letter is the fixture that separates them.
        r"_:\lomse.exe",
        # The right name under the WRONG directory. This is why the optional group is a specific
        # path and not `.*` -- `.*` would admit it.
        r"c:\games\lomse.exe",
        r"c:\windows\system32\cmd.exe /c dir c:\games\lomse.exe",
        OBSERVED_GAME_ARGV0 + ".bak",
        OBSERVED_GAME_ARGV0_DRIVE_ROOT + ".bak",
        f"grep --fixed-strings {OBSERVED_GAME_ARGV0}",
    )

    def test_the_pattern_rejects_command_lines_that_merely_mention_the_game(self) -> None:
        r"""The anchor's real job, with the cases that defeated the first fix.

        `^[A-Za-z]:[\\].*lomse[.]exe` matched every one of these. `.*` was introduced to fix a false
        NEGATIVE -- the executable is six directories down, not at the drive root -- and it
        reintroduced the false POSITIVES the anchor existed to prevent. Both directions need a test
        or the pattern oscillates between the two defects.
        """
        for line in self.NOT_THE_GAME:
            with self.subTest(line=line):
                self.assertIsNone(
                    re.match(GAME_PATTERN.replace("[[:space:]]", r"\s"), line, re.IGNORECASE),
                    f"{line!r} is not the game, but the guard would refuse to install",
                )

    def test_each_decoy_is_pinned_to_the_defect_it_demonstrates(self) -> None:
        r"""The two retired patterns failed on DIFFERENT decoys, and the distinction is the point.

        `OVERBROAD_PATTERN` is still anchored to a drive letter, so it never matched a command line
        beginning with a program name -- that case is the ANCHOR's job and belongs to the original
        bare-name pattern. Asserting all four decoys against the over-broad pattern conflated the
        two failure classes and this test failed until they were separated, which is what a pinning
        test is for.
        """
        # Anchored at a drive letter, so only the over-broad `.*` admits them. `c:\games\lomse.exe`
        # is included deliberately: the over-broad pattern admitted it and the current one does not.
        # `_:\lomse.exe` is excluded for the same reason `grep ...` is: it demonstrates a
        # DIFFERENT defect (the `[A-z]` widening), and the over-broad pattern still required a
        # letter drive, so it never admitted it. Lumping the three classes together is what made
        # an earlier version of this test fail.
        drive_anchored = [
            line
            for line in self.NOT_THE_GAME
            if not line.startswith("grep ") and not line.startswith("_:")
        ]
        for line in drive_anchored:
            with self.subTest(pattern="overbroad", line=line):
                self.assertIsNotNone(
                    re.match(OVERBROAD_PATTERN, line, re.IGNORECASE),
                    f"{line!r} no longer demonstrates the over-broad pattern's failure",
                )
        # Begins with a program name, so the anchor already excluded it; only the bare name admits
        # it. This is the case that skipped three agents' installs in one day.
        mentions = next(line for line in self.NOT_THE_GAME if line.startswith("grep "))
        self.assertIsNone(
            re.match(OVERBROAD_PATTERN, mentions, re.IGNORECASE),
            "the anchor should already exclude a command line that begins with a program name",
        )
        self.assertIsNotNone(
            re.search(BROAD_PATTERN, mentions, re.IGNORECASE),
            f"{mentions!r} no longer demonstrates the bare-name pattern's failure",
        )

    def test_the_shell_and_python_patterns_are_the_same(self) -> None:
        """Two hand-maintained copies of a safety pattern drift, and the drift is silent.

        Both are derived from the same path constant; this asserts the derivations agree
        character for character, which an earlier draft of this fix did not -- the shell emitted
        escaped backslash pairs where Python emitted single ones, so the two regexes meant
        different things while looking alike in a diff.
        """
        completed = subprocess.run(
            [
                "bash",
                "-c",
                'source "$1"/scripts/lib-mod-pipeline.sh; game_command_pattern',
                "bash",
                str(PROJECT_DIR),
            ],
            capture_output=True,
            check=False,
            text=True,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        # NOT `.strip()`. The contract this test exists to enforce is character-for-character
        # equality, and stripping would hide a generator that emitted trailing whitespace.
        # `game_command_pattern` uses printf and deliberately writes no trailing newline.
        self.assertEqual(completed.stdout, GAME_PATTERN)

    def test_the_pattern_is_derived_from_the_path_the_pipeline_installs_to(self) -> None:
        """If the install layout moves, the guard must move with it rather than silently miss."""
        self.assertIn("Lords of Magic Special Edition", GAME_PATTERN)
        self.assertIn(GAME_SUBPATH.rsplit("/", 1)[-1], GAME_PATTERN)

    def test_game_processes_names_the_decoy(self) -> None:
        """The Python side and the shell side must agree, or the suite's skip is wrong."""
        with decoy_process(OBSERVED_GAME_ARGV0):
            found = game_processes()
        self.assertTrue(found, "game_processes() missed a process presenting the game's own name")
        self.assertTrue(
            any("lomse.exe" in line for line in found),
            f"game_processes() matched something that is not the decoy: {found}",
        )


GAME_PROCESSES = game_processes()


@unittest.skipIf(
    bool(GAME_PROCESSES),
    f"the game is running, so install/restore refuse: {GAME_PROCESSES}",
)
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

    def _invoke(self, name: str, arguments: tuple[str, ...]) -> subprocess.CompletedProcess:
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

    def run_script(self, name: str, *arguments: str) -> subprocess.CompletedProcess:
        r"""Run a pipeline script, and decide honestly what a "game is running" refusal means.

        The scripts refuse while the game is up, which is correct and makes every assertion about
        their output fail with a message about the game rather than about the code. The
        class-level `skipIf` asks once, before any test runs, so a game that appears *during* the
        suite slips past it.

        Skipping on the refusal alone would be worse than the red it replaces. The shell guard is
        a `pgrep -f` against the same anchored pattern this module uses; it used to be the bare
        name, which matched this project's own tools -- they take that path as an argument -- and
        matched anything that merely quoted it. Skipping on *that* would have deleted this file's
        install, profile-creation and restore coverage silently, at exactly the moment the corpus
        tooling runs. So:

        - a process that really is the game (Wine's `c:\...\english\lomse.exe`) -> skip, naming it;
        - a refusal with no such process -> retry once, because the match may have been a
          process that has already exited, and then **fail**, printing whatever a looser match
          finds, because a refusal that cannot be attributed to the game is a defect in the guard
          rather than a state to tolerate.

        **The retry is load-bearing, not caution, and deleting it restores the flake it fixes.**
        Anchoring shrinks the false-positive population but cannot win a race: sampling `ps`
        continuously through a failing run caught **zero** matching command lines, so some matches
        are processes that exit within milliseconds and are gone before anything can name them.
        Retrying is safe by construction -- the guard refuses *before* the script writes anything,
        so a refused run has changed nothing to re-run over.
        """
        completed = self._invoke(name, arguments)
        if GAME_IS_UP not in completed.stderr:
            return completed

        running = game_processes()
        if running:
            raise unittest.SkipTest(f"the game started while the suite ran: {running}")

        broad = matching_processes(BROAD_PATTERN)
        completed = self._invoke(name, arguments)
        if GAME_IS_UP not in completed.stderr:
            return completed

        running = game_processes()
        if running:
            raise unittest.SkipTest(f"the game started while the suite ran: {running}")
        self.fail(
            f"{name} refused twice because its `pgrep -f` matched, but nothing matches "
            f"{GAME_PATTERN!r}, so the game is not running. A looser `lomse.exe` match found: "
            f"{broad or 'nothing, by the time this test could look'}. A persistent match that is "
            "not the game is a defect in the guard (scripts/lib-mod-pipeline.sh), not a state to "
            "tolerate."
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
        self.assertIs(run.mechanism, Mechanism.PBM_PATCH)

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

    def test_every_archive_with_a_run_has_a_marked_region_that_is_the_render(self) -> None:
        """Enumerated, not hardcoded, and equal rather than contained.

        Two earlier versions of this were weaker in ways that only show up when someone adds an
        archive: one checked `pic.mpq` by name, so `gs.mpq` -- and any future archive -- was
        coupled to nothing; and one asked whether a sentence appeared *anywhere* in the document,
        which cannot see polarity or place. `docs/build-pipeline.md` quotes a refuted claim in
        order to refute it, and any sub-span of that quotation would have satisfied the old rule.
        A marked region compared for equality has neither problem.
        """
        roadmap = self.roadmap()
        covered = 0
        for archive, acceptance in sorted(ACCEPTANCE.items()):
            if acceptance.run is None:
                continue
            covered += 1
            with self.subTest(archive=archive):
                open_marker, close_marker = roadmap_region(archive)
                self.assertIn(open_marker, roadmap, f"{archive} has no marked region")
                self.assertIn(close_marker, roadmap, f"{archive}'s marked region is not closed")
                region = roadmap.split(open_marker, 1)[1].split(close_marker, 1)[0]
                self.assertEqual(
                    self.flatten(region),
                    self.flatten(roadmap_paragraph(archive)),
                    f"docs/roadmap.md's {archive} region is not what "
                    "tools/engine_acceptance.py renders. The facts are the source: change them "
                    "there and paste what roadmap_paragraph prints.",
                )
        self.assertGreater(covered, 1, "at least gs.mpq and pic.mpq have runs")

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

    def test_every_observation_reaches_the_documentation_through_its_region(self) -> None:
        """The one free-text field, pinned to a place rather than to a document.

        `observation` is the only sentence a record still writes, and the previous rule -- does it
        appear somewhere in `roadmap.md` or `build-pipeline.md` -- was blind to both polarity and
        place. It now has to appear inside that archive's own marked region, which the test above
        holds equal to the render, so widening it means writing the wider claim into the roadmap
        where a reader will meet it.
        """
        roadmap = self.roadmap()
        for archive, acceptance in sorted(ACCEPTANCE.items()):
            if acceptance.run is None:
                continue
            with self.subTest(archive=archive):
                open_marker, close_marker = roadmap_region(archive)
                region = roadmap.split(open_marker, 1)[1].split(close_marker, 1)[0]
                self.assertIn(
                    self.flatten(acceptance.run.observation),
                    self.flatten(region),
                    "an observation has to be a claim the roadmap makes in this archive's region",
                )

    def test_an_observation_may_not_carry_a_quantity_the_run_does_not_record(self) -> None:
        with self.assertRaises(ValueError):
            EngineRun(
                date="2026-09-18",
                members=1,
                disposition=Disposition.REPLACED,
                edit_kind=EditKind.LENGTH_PRESERVING,
                mechanism=Mechanism.PBM_PATCH,
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
            mechanism=Mechanism.PBM_PATCH,
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
                        f"{run.mechanism.value}. {run.observation} Not established: "
                        f"{limits}."
                    )
                    if acceptance.storage_class:
                        expected += f" {acceptance.storage_class.value}"
                self.assertEqual(acceptance.summary(), expected)

    def test_the_build_json_a_real_report_writes_is_the_rendered_facts(self) -> None:
        """Run the build's own report command and read what it wrote.

        This replaces a check on the *shape of the source* -- an AST walk asserting the dict
        literal held `engine_acceptance.build_metadata()`. That could not see what happened to the
        dict afterwards, and a reviewer walked straight past it by mutating `build` between the
        literal and `write_text`. A source-shape check cannot bound runtime behaviour. One
        equality on the file the command actually produces closes the whole class.
        """
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            mod = root / "caveat-probe"
            (mod / "archives" / "gs.mpq" / "units").mkdir(parents=True)
            (mod / "mod.toml").write_text(
                'id = "caveat-probe"\n'
                'name = "Caveat probe"\n'
                'version = "0.0.1"\n'
                'description = "A tree that exists only so the report command can run."\n'
                'base_profile = "vanilla"\n',
                encoding="utf-8",
            )
            member = mod / "archives" / "gs.mpq" / "units" / "orinf.gs"
            member.write_bytes(b"/hit_points 18 def")

            manifest = root / "gs.tsv"
            manifest.write_text(
                "\t".join(MANIFEST_COLUMNS)
                + "\n"
                + "\t".join(
                    [
                        "units\\orinf.gs",
                        "3",
                        "7",
                        "18",
                        "18",
                        "0x80010100",
                        "0",
                        "a" * 64,
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            def facts_file(name: str, member: str) -> Path:
                # `--gs-facts` output: the base file is keyed by member path, the mod file by the
                # path inside the tree, which is how `build_change_report` looks each of them up.
                path = root / name
                path.write_text(
                    json.dumps(
                        {
                            "name": member,
                            "sha256": "a" * 64,
                            "token_sha256": "b" * 64,
                            "token_count": 4,
                            "parse_error": None,
                            "scalar_definitions": {"hit_points": "18"},
                        }
                    )
                    + "\n",
                    encoding="utf-8",
                )
                return path

            base_facts = facts_file("base-facts.jsonl", "units\\orinf.gs")
            mod_facts = facts_file("mod-facts.jsonl", "archives/gs.mpq/units/orinf.gs")
            symbols = root / "symbols.tsv"
            symbols.write_text(
                "name\tkind\tevidence\tprofiles\tmember\tline\n", encoding="utf-8"
            )
            output = root / "out"
            output.mkdir()

            arguments = SimpleNamespace(
                mod=mod,
                base_manifest=[("gs.mpq", str(manifest))],
                base_gs_facts=str(base_facts),
                mod_gs_facts=str(mod_facts),
                base_file=[],
                base_digest=[],
                tool_digest=[],
                output_digest=[],
                symbols=str(symbols),
                build_id="probe",
                output_dir=str(output),
            )
            # The command prints its change report; this test is about the file it writes.
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(command_report(arguments), 0)
            written = json.loads((output / "build.json").read_text(encoding="utf-8"))

        self.assertEqual(written["engine_acceptance"], build_metadata())

    def test_a_run_that_cannot_have_happened_is_refused(self) -> None:
        with self.assertRaises(ValueError):
            EngineRun(
                date="2026-09-18",
                members=0,
                disposition=Disposition.REPLACED,
                edit_kind=EditKind.LENGTH_PRESERVING,
                mechanism=Mechanism.PBM_PATCH,
                observation="y",
            )
        with self.assertRaises(ValueError):
            EngineRun(
                date="last Tuesday",
                members=1,
                disposition=Disposition.REPLACED,
                edit_kind=EditKind.LENGTH_PRESERVING,
                mechanism=Mechanism.PBM_PATCH,
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
        self.assertIs(ACCEPTANCE["gs.mpq"].storage_class, StorageClass.IMPLODE_PROVED)
        self.assertIn("0x80010100", ACCEPTANCE["gs.mpq"].storage_class.value)


if __name__ == "__main__":
    unittest.main()
