"""`mod_build.entry_kind`: what the repack is asked to do with each source file.

The classification decides which expectation the shape check is handed, so getting it wrong does
not produce a wrong archive -- it produces a RIGHT archive checked against the wrong question. A
member that came back unchanged under a `replace` expectation is refused; a member that changed
under an `unchanged` one is refused; and an addition under either is refused for the wrong reason.
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

    def source(self, contents: bytes) -> SourceMember:
        path = self.root / "cursors.imp"
        path.write_bytes(contents)
        return SourceMember(
            archive="imp.mpq",
            member="iface\\cursors.imp",
            path=path,
            relative="archives/imp.mpq/iface/cursors.imp",
        )

    def test_a_file_matching_its_base_member_is_a_no_op(self) -> None:
        contents = b"same"
        kind = entry_kind(
            self.source(contents), base_member(hashlib.sha256(contents).hexdigest())
        )
        self.assertEqual(kind, "unchanged")

    def test_a_file_differing_from_its_base_member_is_a_replacement(self) -> None:
        kind = entry_kind(
            self.source(b"different"), base_member(hashlib.sha256(b"same").hexdigest())
        )
        self.assertEqual(kind, "replace")

    def test_a_file_with_no_base_member_is_an_addition(self) -> None:
        self.assertEqual(entry_kind(self.source(b"brand new"), None), "add")

    def test_the_digest_comes_from_the_file_on_disk(self) -> None:
        """Not from anything recorded earlier.

        A build that decided `unchanged` from a stale note would hand the shape check an
        expectation about bytes it is not packing -- and then a genuine edit would be refused as a
        no-op that moved, which reads as a pipeline fault rather than as a stale note.
        """
        source = self.source(b"same")
        base = base_member(hashlib.sha256(b"same").hexdigest())
        self.assertEqual(entry_kind(source, base), "unchanged")

        source.path.write_bytes(b"edited after the fact")
        self.assertEqual(entry_kind(source, base), "replace")


if __name__ == "__main__":
    unittest.main()
