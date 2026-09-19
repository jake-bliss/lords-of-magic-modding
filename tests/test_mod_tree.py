"""The mod source tree contract, exercised against trees built in a temporary directory.

No game is installed for any of these. That is the point of keeping `tools/mod_tree.py` free of
archive access: the mapping from a file's path to an archive member name is pure, so every shape
it refuses can be built and asserted here.
"""

import tempfile
import unittest
from pathlib import Path

from tools.mod_tree import (
    PROFILE_APPS,
    SUPPORTED_ARCHIVES,
    ModTreeError,
    load,
    load_manifest,
    member_name_for,
    source_digest,
)

MANIFEST = """\
id = "{mod_id}"
name = "A mod"
version = "0.1.0"
base_profile = "{profile}"
"""


class TreeFixture:
    """A mod tree under a temporary directory."""

    def __init__(self, root: Path, mod_id: str = "example", profile: str = "vanilla") -> None:
        self.root = root / mod_id
        self.root.mkdir(parents=True)
        (self.root / "mod.toml").write_text(
            MANIFEST.format(mod_id=mod_id, profile=profile), encoding="utf-8"
        )
        (self.root / "archives").mkdir()

    def add(self, relative: str, content: bytes = b"/a 1 def") -> Path:
        path = self.root / "archives" / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path

    def manifest(self, text: str) -> None:
        (self.root / "mod.toml").write_text(text, encoding="utf-8")


class MemberNameTest(unittest.TestCase):
    def test_a_nested_path_becomes_a_backslash_member_name(self) -> None:
        self.assertEqual(member_name_for("units/orinf.gs"), "units\\orinf.gs")
        self.assertEqual(member_name_for("gs/dlg/ARTINFO5.gs"), "gs\\dlg\\ARTINFO5.gs")

    def test_an_archive_root_file_keeps_its_bare_name(self) -> None:
        """The reason `archives/<archive>/` is not optional.

        `START.GS` and `gs\\hotkey.gs` are both real `gs.mpq` members. A single `gs/` source
        directory could not express the first without inventing a rule about which files live at
        the root.
        """
        self.assertEqual(member_name_for("START.GS"), "START.GS")

    def test_case_is_preserved_exactly(self) -> None:
        """Folding here would destroy the difference the case-exactness check exists to find."""
        self.assertEqual(member_name_for("LBM/ACTIONS5R3A.lbm"), "LBM\\ACTIONS5R3A.lbm")


class LoadTest(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.base = Path(self._temporary.name)

    def tearDown(self) -> None:
        self._temporary.cleanup()

    def fixture(self, **kwargs) -> TreeFixture:
        return TreeFixture(self.base, **kwargs)

    def test_a_nested_file_maps_to_its_member(self) -> None:
        tree = self.fixture()
        tree.add("gs.mpq/units/orinf.gs")
        loaded = load(tree.root)

        self.assertEqual(loaded.rejected, [])
        self.assertEqual(len(loaded.members), 1)
        member = loaded.members[0]
        self.assertEqual(member.archive, "gs.mpq")
        self.assertEqual(member.member, "units\\orinf.gs")
        self.assertEqual(member.relative, "archives/gs.mpq/units/orinf.gs")
        self.assertTrue(member.is_gamescript)

    def test_an_archive_root_file_maps_to_a_root_member(self) -> None:
        tree = self.fixture()
        tree.add("gs.mpq/START.GS")
        loaded = load(tree.root)

        self.assertEqual([item.member for item in loaded.members], ["START.GS"])

    def test_two_archives_are_kept_apart(self) -> None:
        tree = self.fixture()
        tree.add("gs.mpq/a.gs")
        tree.add("pic.mpq/LBM/ART.lbm", b"\x00")
        loaded = load(tree.root)

        self.assertEqual(loaded.archives(), ["gs.mpq", "pic.mpq"])
        self.assertEqual([item.member for item in loaded.members_for("pic.mpq")], ["LBM\\ART.lbm"])
        self.assertFalse(loaded.members_for("pic.mpq")[0].is_gamescript)

    # -- refusals ---------------------------------------------------------------------------

    def test_a_file_directly_under_archives_is_rejected(self) -> None:
        tree = self.fixture()
        (tree.root / "archives" / "loose.gs").write_bytes(b"/a 1 def")
        loaded = load(tree.root)

        self.assertEqual(loaded.members, [])
        self.assertEqual(len(loaded.rejected), 1)
        self.assertIn("no archive to belong to", loaded.rejected[0][1])

    def test_an_unsupported_archive_directory_is_rejected(self) -> None:
        # `sndfx.mpq` and `imp.mpq` were both once the example here and both are now supported.
        # The name is taken from the set's complement rather than written in, so widening the set
        # again cannot leave this test asserting a refusal of something the pipeline now accepts.
        unsupported = next(
            name
            for name in ("attr.mpq", "smk.mpq", "speech.mpq")
            if name not in SUPPORTED_ARCHIVES
        )
        tree = self.fixture()
        tree.add(f"{unsupported}/a.bin", b"\x00")
        loaded = load(tree.root)

        self.assertEqual(loaded.members, [])
        self.assertIn("unsupported archive", loaded.rejected[0][1])

    def test_every_supported_archive_directory_is_accepted(self) -> None:
        # The other half: a set that only ever refuses is a set nobody notices has gone empty.
        tree = self.fixture()
        for index, archive in enumerate(SUPPORTED_ARCHIVES):
            tree.add(f"{archive}/member{index}.bin", bytes((index,)))
        loaded = load(tree.root)

        self.assertEqual(loaded.rejected, [])
        self.assertEqual(
            sorted(loaded.archives()), sorted(SUPPORTED_ARCHIVES)
        )

    def test_finder_litter_is_rejected_by_name(self) -> None:
        """`.DS_Store` would otherwise be inferred as a member and reported as a puzzling miss."""
        tree = self.fixture()
        tree.add("gs.mpq/units/.DS_Store", b"\x00\x01")
        loaded = load(tree.root)

        self.assertEqual(loaded.members, [])
        self.assertIn("litter", loaded.rejected[0][1])

    def test_a_symlink_is_rejected(self) -> None:
        """A tree that links outside itself makes its own source digest a lie."""
        tree = self.fixture()
        outside = self.base / "outside.gs"
        outside.write_bytes(b"/a 1 def")
        (tree.root / "archives" / "gs.mpq").mkdir()
        (tree.root / "archives" / "gs.mpq" / "linked.gs").symlink_to(outside)
        loaded = load(tree.root)

        self.assertEqual(loaded.members, [])
        self.assertIn("symlink", loaded.rejected[0][1])

    def test_two_files_differing_only_in_case_are_rejected(self) -> None:
        """An MPQ's name hash is case-insensitive, so it cannot hold both."""
        tree = self.fixture()
        tree.add("gs.mpq/units/Orinf.gs")
        collided = tree.root / "archives" / "gs.mpq" / "units" / "orinf.gs"
        if collided.exists():
            self.skipTest("this filesystem is case-insensitive; the collision cannot be built")
        collided.write_bytes(b"/b 2 def")
        loaded = load(tree.root)

        self.assertEqual(len(loaded.members), 1)
        self.assertIn("collides case-insensitively", loaded.rejected[0][1])

    def test_a_tree_with_no_archives_directory_is_refused(self) -> None:
        root = self.base / "bare"
        root.mkdir()
        (root / "mod.toml").write_text(
            MANIFEST.format(mod_id="bare", profile="vanilla"), encoding="utf-8"
        )
        with self.assertRaisesRegex(ModTreeError, "no archives/"):
            load(root)

    # -- mod.toml ---------------------------------------------------------------------------

    def test_a_missing_required_key_is_refused_by_name(self) -> None:
        tree = self.fixture()
        tree.manifest('id = "example"\nname = "A mod"\n')
        with self.assertRaisesRegex(ModTreeError, "base_profile, version"):
            load(tree.root)

    def test_an_unknown_base_profile_is_refused(self) -> None:
        tree = self.fixture()
        tree.manifest(MANIFEST.format(mod_id="example", profile="development"))
        with self.assertRaisesRegex(ModTreeError, "not one of"):
            load(tree.root)
        self.assertNotIn("development", PROFILE_APPS)

    def test_the_id_must_match_the_directory_name(self) -> None:
        tree = self.fixture()
        tree.manifest(MANIFEST.format(mod_id="something-else", profile="vanilla"))
        with self.assertRaisesRegex(ModTreeError, "does not match the directory name"):
            load(tree.root)

    def test_an_id_with_characters_that_would_need_quoting_is_refused(self) -> None:
        for bad in ("Example", "ex ample", "ex/ample", "-example", ""):
            with self.subTest(mod_id=bad):
                root = self.base / f"case-{abs(hash(bad))}"
                root.mkdir()
                (root / "archives").mkdir()
                (root / "mod.toml").write_text(
                    MANIFEST.format(mod_id=bad, profile="vanilla"), encoding="utf-8"
                )
                with self.assertRaises(ModTreeError):
                    load(root)

    def test_a_key_the_pipeline_ignores_is_refused(self) -> None:
        """A key an author believes is doing something must not silently do nothing."""
        tree = self.fixture()
        tree.manifest(
            MANIFEST.format(mod_id="example", profile="vanilla") + 'compression = "implode"\n'
        )
        with self.assertRaisesRegex(ModTreeError, "does not understand"):
            load(tree.root)

    def test_new_members_defaults_to_empty_and_refused(self) -> None:
        tree = self.fixture()
        tree.add("gs.mpq/a.gs")
        loaded = load(tree.root)

        self.assertEqual(loaded.manifest.new_members, ())
        self.assertFalse(loaded.manifest.allow_new_members)

    def test_new_members_must_be_strings(self) -> None:
        tree = self.fixture()
        tree.manifest(MANIFEST.format(mod_id="example", profile="vanilla") + "new_members = [3]\n")
        with self.assertRaisesRegex(ModTreeError, "list of member-name strings"):
            load(tree.root)


class SourceDigestTest(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.base = Path(self._temporary.name)

    def tearDown(self) -> None:
        self._temporary.cleanup()

    def test_the_digest_moves_with_content_and_with_names(self) -> None:
        tree = TreeFixture(self.base, mod_id="a")
        tree.add("gs.mpq/units/orinf.gs", b"/hit_points 13 def")
        first = source_digest(load(tree.root))

        tree.add("gs.mpq/units/orinf.gs", b"/hit_points 18 def")
        after_content = source_digest(load(tree.root))
        self.assertNotEqual(first, after_content)

        tree.add("gs.mpq/units/orinf.gs", b"/hit_points 13 def")
        self.assertEqual(source_digest(load(tree.root)), first)

        tree.add("gs.mpq/units/orcav.gs", b"/hit_points 13 def")
        self.assertNotEqual(source_digest(load(tree.root)), first)

    def test_the_digest_moves_with_mod_toml(self) -> None:
        tree = TreeFixture(self.base, mod_id="b")
        tree.add("gs.mpq/a.gs")
        first = source_digest(load(tree.root))
        tree.manifest(
            MANIFEST.format(mod_id="b", profile="vanilla").replace("0.1.0", "0.2.0")
        )
        self.assertNotEqual(source_digest(load(tree.root)), first)

    def test_litter_does_not_move_the_digest(self) -> None:
        """Rejected files are not input to the build, so they must not change its identity."""
        tree = TreeFixture(self.base, mod_id="c")
        tree.add("gs.mpq/a.gs")
        first = source_digest(load(tree.root))
        tree.add("gs.mpq/.DS_Store", b"\x00\x99")
        self.assertEqual(source_digest(load(tree.root)), first)


if __name__ == "__main__":
    unittest.main()



class SeedBootstrapTest(unittest.TestCase):
    """`mod-seed.sh` is the step that CREATES `archives/`, so it cannot require it to exist.

    The first command in every mod's README failed on a clean checkout: `mods/*/archives/` is
    gitignored, `mod-seed.sh` asked `load()` for the base profile, and `load()` refuses a tree
    with no `archives/` directory. It went unnoticed because that directory is present in any
    working tree that has already seeded once.
    """

    def _mod(self, root: Path, mod_id: str = "demo") -> Path:
        mod = root / mod_id
        mod.mkdir(parents=True)
        (mod / "mod.toml").write_text(
            MANIFEST.format(mod_id=mod_id, profile="vanilla"), encoding="utf-8"
        )
        return mod

    def test_load_manifest_works_before_archives_exists(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            mod = self._mod(Path(tmp))
            self.assertFalse((mod / "archives").exists())
            self.assertEqual(load_manifest(mod).base_profile, "vanilla")

    def test_load_manifest_still_enforces_the_id_check(self) -> None:
        # The bootstrap path must not be a hole in the id/directory agreement rule: a mod
        # directory renamed after creation has to fail here, not silently at build time.
        with tempfile.TemporaryDirectory() as tmp:
            mod = self._mod(Path(tmp), "demo")
            renamed = mod.parent / "renamed"
            mod.rename(renamed)
            with self.assertRaises(ModTreeError) as caught:
                load_manifest(renamed)
            self.assertIn("does not match the directory name", str(caught.exception))

    def test_load_still_requires_archives_and_names_the_fix(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            mod = self._mod(Path(tmp))
            with self.assertRaises(ModTreeError) as caught:
                load(mod)
            message = str(caught.exception)
            self.assertIn("no archives/ directory", message)
            # A refusal that states a precondition without naming the step that satisfies it
            # sends its reader to the source. This one names mod-seed.sh.
            self.assertIn("mod-seed.sh", message)
