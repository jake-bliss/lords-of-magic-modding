"""Refusal tests for the one-entry install allowlist.

Every case here builds its own `Applications` directory in a temporary folder and points the real
guard at it. Nothing under `~/Applications` is read or written by this file, and the guard being
exercised is the same function the installer calls -- there is no test-only path through it.

The cases are chosen to be the ones a "the target is not the baseline" check would approve:
a sibling profile, a path nobody named, a `..` escape, and a symlink that resolves into a profile
with no backup.
"""

import os
import tempfile
import unittest
from pathlib import Path

from tools.install_guard import (
    DEV_PROFILE_NAME,
    InstallRefused,
    assert_writable,
    dev_profile_root,
    resolve_write_target,
)

BASELINE = "Steambuild 32 64bit DXVK.app"
GAME_SUBPATH = (
    "Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/"
    "common/Lords of Magic Special Edition/English"
)


class AllowlistTest(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.applications = Path(self._temporary.name) / "Applications"
        for name in (
            BASELINE,
            "Lords of Magic 3.02.app",
            "Lords of Magic GS5R3.app",
            DEV_PROFILE_NAME,
        ):
            (self.applications / name / GAME_SUBPATH).mkdir(parents=True)
            (self.applications / name / GAME_SUBPATH / "gs.mpq").write_bytes(b"MPQ\x1a")
            # The loose map/ directory, which has no backup anywhere. Present so a test can name
            # a real path inside it.
            (self.applications / name / GAME_SUBPATH / "map").mkdir()

    def tearDown(self) -> None:
        self._temporary.cleanup()

    def refuse(self, path) -> str:
        with self.assertRaises(InstallRefused) as caught:
            assert_writable(path, self.applications)
        return str(caught.exception)

    # -- what is permitted ------------------------------------------------------------------

    def test_the_development_profile_itself_is_permitted(self) -> None:
        root = self.applications / DEV_PROFILE_NAME
        self.assertEqual(assert_writable(root, self.applications), root.resolve())

    def test_a_file_inside_the_development_profile_is_permitted(self) -> None:
        target = self.applications / DEV_PROFILE_NAME / GAME_SUBPATH / "gs.mpq"
        self.assertEqual(assert_writable(target, self.applications), target.resolve())

    def test_a_path_that_does_not_exist_yet_is_permitted_inside_the_profile(self) -> None:
        """The profile is created by writing paths that do not exist yet."""
        target = self.applications / DEV_PROFILE_NAME / ".lom-pipeline" / "MANIFEST.sha256"
        self.assertEqual(assert_writable(target, self.applications), target.resolve())

    def test_the_root_is_permitted_before_it_exists_at_all(self) -> None:
        fresh = Path(self._temporary.name) / "Fresh"
        fresh.mkdir()
        target = fresh / DEV_PROFILE_NAME
        self.assertEqual(assert_writable(target, fresh), target.resolve())

    # -- what is refused --------------------------------------------------------------------

    def test_the_preserved_baseline_is_refused(self) -> None:
        message = self.refuse(self.applications / BASELINE / GAME_SUBPATH / "gs.mpq")
        self.assertIn("refusing to write outside the development profile", message)

    def test_each_other_installed_profile_is_refused(self) -> None:
        for name in (BASELINE, "Lords of Magic 3.02.app", "Lords of Magic GS5R3.app"):
            with self.subTest(profile=name):
                self.refuse(self.applications / name / GAME_SUBPATH / "gs.mpq")

    def test_the_unbacked_map_directory_of_another_profile_is_refused(self) -> None:
        self.refuse(self.applications / BASELINE / GAME_SUBPATH / "map" / "anything.scn")

    def test_the_applications_directory_itself_is_refused(self) -> None:
        self.refuse(self.applications)

    def test_a_sibling_directory_nobody_named_is_refused(self) -> None:
        """The weak form of this check approves everything it was not told about."""
        self.refuse(self.applications / "Some Other Thing.app" / "gs.mpq")

    def test_a_dot_dot_escape_is_refused_by_the_literal_path(self) -> None:
        target = (
            self.applications / DEV_PROFILE_NAME / ".." / BASELINE / GAME_SUBPATH / "gs.mpq"
        )
        message = self.refuse(target)
        self.assertIn("'..'", message)

    def test_a_dot_dot_that_resolves_back_inside_is_still_refused(self) -> None:
        """Refused on the literal path, before resolution, even though it would resolve inside.

        A path that needs `..` to reach its target is a path somebody assembled by accident.
        """
        target = self.applications / DEV_PROFILE_NAME / "x" / ".." / "gs.mpq"
        self.refuse(target)

    def test_a_symlinked_development_profile_is_refused(self) -> None:
        """The hole in "compare the resolved paths".

        If the development profile is a link into the baseline, resolving the requested path and
        the allowed root the same way makes every write to the baseline compare equal.
        """
        fresh = Path(self._temporary.name) / "Linked"
        fresh.mkdir()
        os.symlink(self.applications / BASELINE, fresh / DEV_PROFILE_NAME)
        with self.assertRaises(InstallRefused) as caught:
            assert_writable(fresh / DEV_PROFILE_NAME / GAME_SUBPATH / "gs.mpq", fresh)
        self.assertIn("is a symlink", str(caught.exception))

    def test_a_symlink_inside_the_profile_pointing_out_is_refused(self) -> None:
        inside = self.applications / DEV_PROFILE_NAME / "escape"
        os.symlink(self.applications / BASELINE, inside)
        self.refuse(inside / GAME_SUBPATH / "gs.mpq")

    def test_a_sibling_whose_name_merely_starts_with_the_profile_name_is_refused(self) -> None:
        """A prefix comparison would approve this. Parent containment does not."""
        self.refuse(self.applications / (DEV_PROFILE_NAME + ".backup") / "gs.mpq")

    def test_an_absolute_path_far_outside_is_refused(self) -> None:
        self.refuse(Path(self._temporary.name) / "elsewhere" / "gs.mpq")

    def test_the_home_directory_is_refused(self) -> None:
        self.refuse(Path.home())

    def test_the_root_directory_is_refused(self) -> None:
        self.refuse(Path("/"))

    # -- the target it returns --------------------------------------------------------------

    def test_the_approved_target_carries_the_root_it_was_approved_against(self) -> None:
        target = self.applications / DEV_PROFILE_NAME / "gs.mpq"
        approved = resolve_write_target(target, self.applications)
        self.assertEqual(approved.root, (self.applications / DEV_PROFILE_NAME).resolve())
        self.assertEqual(approved.path, target.resolve())

    def test_the_profile_name_is_a_constant_no_caller_can_widen(self) -> None:
        """`dev_profile_root` takes an Applications directory and nothing else.

        The name is not a parameter anywhere, so there is no argument a caller could pass to make
        the allowlist admit a second directory.
        """
        self.assertEqual(
            dev_profile_root(self.applications).name, "Lords of Magic Development.app"
        )


if __name__ == "__main__":
    unittest.main()
