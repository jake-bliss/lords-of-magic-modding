"""`mod_build.entry_kind`: what the repack is asked to do with each source file.

The classification decides which expectation the shape check is handed, so getting it wrong does
not produce a wrong archive -- it produces a RIGHT archive checked against the wrong question. A
member that came back unchanged under a `replace` expectation is refused; a member that changed
under an `unchanged` one is refused; and an addition under either is refused for the wrong reason.

`unchanged` is a DECLARED classification (`mod.toml`'s `expect_unchanged`), not one inferred from
comparing the file's bytes to the base member. A byte-identical file that is NOT declared must
still come out `replace`, so that `tools/mpq_shape.py`'s `declared_change_not_applied` can refuse a
repack that silently did nothing -- that refusal can only fire while an undeclared no-op is
classified as a declared change.
"""

import hashlib
import sys
import tempfile
import unittest
from pathlib import Path

PROJECT_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PROJECT_DIR / "tools"))

from mod_build import entry_kind  # noqa: E402
from mod_tree import SourceMember  # noqa: E402
from mod_validate import normalise_member  # noqa: E402
from mpq_shape import Member  # noqa: E402


def base_member(sha256: str) -> Member:
    return Member(
        path="iface\\cursors.imp",
        block_index=0,
        hash_index=0,
        size=4,
        compressed_size=4,
        flags="0x80010100",
        locale=0,
        sha256=sha256,
    )


class EntryKindTest(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.root = Path(self._temporary.name)
        self.addCleanup(self._temporary.cleanup)

    def source(self, contents: bytes, member: str = "iface\\cursors.imp") -> SourceMember:
        path = self.root / "cursors.imp"
        path.write_bytes(contents)
        return SourceMember(
            archive="imp.mpq",
            member=member,
            path=path,
            relative="archives/imp.mpq/iface/cursors.imp",
        )

    def test_an_undeclared_byte_identical_member_is_still_a_replacement(self) -> None:
        """The regression this file exists to pin.

        `entry_kind` once computed `unchanged` from a digest comparison alone, which made a
        forgotten edit indistinguishable from a declared no-op: both produced byte-identical
        output, and both were classified the same way. That silently disarmed
        `declared_change_not_applied` for every mod that never touched `expect_unchanged` --
        which was every mod, since the field did not exist yet. A modder who seeded a tree and
        forgot to apply the edit got a green build.
        """
        contents = b"same"
        kind = entry_kind(
            self.source(contents), base_member(hashlib.sha256(contents).hexdigest())
        )
        self.assertEqual(kind, "replace")

    def test_a_declared_no_op_member_is_unchanged(self) -> None:
        contents = b"same"
        kind = entry_kind(
            self.source(contents),
            base_member(hashlib.sha256(contents).hexdigest()),
            frozenset({"iface\\cursors.imp"}),
        )
        self.assertEqual(kind, "unchanged")

    def test_declaration_matches_by_member_name_case_and_slash_insensitively(self) -> None:
        """Same normalisation `new_members` gets, via the same `normalise_member`.

        The caller (`mod_build.command_plan`) normalises `mod.toml`'s `expect_unchanged` list once
        with `normalise_member`, the same helper `new_members` is checked against; `entry_kind`
        then normalises the source's own spelling before comparing. A mod tree's own spelling is
        checked exactly elsewhere (`mod_validate.check_member_resolution`'s case-mismatch
        finding); this normalisation is only about matching a declaration to the source it
        describes, forward slashes and casing included.
        """
        contents = b"same"
        kind = entry_kind(
            self.source(contents, member="IFACE\\CURSORS.IMP"),
            base_member(hashlib.sha256(contents).hexdigest()),
            frozenset({normalise_member("iface/cursors.imp")}),
        )
        self.assertEqual(kind, "unchanged")

    def test_a_file_differing_from_its_base_member_is_a_replacement(self) -> None:
        kind = entry_kind(
            self.source(b"different"), base_member(hashlib.sha256(b"same").hexdigest())
        )
        self.assertEqual(kind, "replace")

    def test_a_file_with_no_base_member_is_an_addition_even_if_declared_unchanged(self) -> None:
        """`add` wins over a stray `expect_unchanged` entry.

        There is no base member for `unchanged` to mean anything about, and `add` is what
        validation has already insisted the mod also declare in `new_members` --
        `resolve_base_members` guarantees `base` is `None` here only when that happened.
        """
        self.assertEqual(
            entry_kind(
                self.source(b"brand new"), None, frozenset({"iface\\cursors.imp"})
            ),
            "add",
        )

    def test_declaration_is_not_re_derived_from_the_files_current_bytes(self) -> None:
        """`entry_kind` answers from the declaration, not from a fresh digest.

        This is deliberate, not a gap: whether a declared no-op actually held is
        `mpq_shape.compare`'s question, checked against the PACKED archive
        (`declared_no_op_changed_content`), not this function's. Folding that check back into
        `entry_kind` is exactly the shape the original bug had, just moved to a different bit of
        state (the declaration) instead of the base digest.
        """
        source = self.source(b"same")
        base = base_member(hashlib.sha256(b"same").hexdigest())
        expect_unchanged = frozenset({"iface\\cursors.imp"})
        self.assertEqual(entry_kind(source, base, expect_unchanged), "unchanged")

        source.path.write_bytes(b"edited after the fact")
        self.assertEqual(entry_kind(source, base, expect_unchanged), "unchanged")


if __name__ == "__main__":
    unittest.main()
