"""Tokenizer tests, and a parity check against the authority.

`spikes/asset-viewer/src/gamescript.rs` is the lexer the build pipeline validates with, so it is
the authority on this grammar and this module is the second implementation. Three divergences from
it were closed on 2026-09-18 -- the LF-only `;` comment rule, a `\\` escape inside strings, and `/`
not ending a name -- plus `str.isspace()` being wider than `is_ascii_whitespace`. The
`AuthorityParityTest` class below asserts agreement with the real Rust lexer rather than with a
literal written here, so it fails if either implementation moves.
"""

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tools.gs_syntax import normalized_bytes, tokens

PROJECT_DIR = Path(__file__).resolve().parents[1]
VIEWER = PROJECT_DIR / "spikes" / "asset-viewer" / "target" / "release" / "lom-asset-viewer"


class GsSyntaxTest(unittest.TestCase):
    def test_ignores_layout_and_comments_but_preserves_strings(self) -> None:
        compact = 'hidecursor"gs/text.gs"run{name 1 def}if'
        formatted = 'hidecursor\n"gs/text.gs" run { name 1 def } if ; note\n'
        self.assertEqual(tokens(compact), tokens(formatted))

    def test_semicolon_inside_string_is_not_a_comment(self) -> None:
        self.assertEqual(tokens('"keep;this" print'), ['"keep;this"', "print"])

    def test_comment_ends_at_a_bare_carriage_return(self) -> None:
        """Bare CR is a line ending here, and 25 GS5R3 members have no LF anywhere.

        Ending a `;` comment at `\\n` alone made the rest of such a member read as comment text,
        so every statement after the first comment disappeared. The fixture reproduces the shape
        rather than shipping a member; the corpus measurement is in `docs/roadmap.md`.
        """
        source = (
            "; a Mac-line-ended header comment\r/kept{1}def\r; another comment\r/also_kept{2}def\r"
        )
        self.assertEqual(
            tokens(source),
            ["/kept", "{", "1", "}", "def", "/also_kept", "{", "2", "}", "def"],
        )
        # The same bytes with LF line endings must tokenize identically; if they do not, the
        # tokenizer is treating one of the two as text.
        self.assertEqual(tokens(source), tokens(source.replace("\r", "\n")))

    def test_crlf_tokenizes_like_lf(self) -> None:
        """A no-regression guard, not evidence for the CR fix: it passes under the LF-only rule
        too, because the LF of a CRLF pair already terminated the comment."""
        source = "; header\r\n/kept{1}def\r\n"
        self.assertEqual(tokens(source), ["/kept", "{", "1", "}", "def"])
        self.assertEqual(tokens(source), tokens("; header\n/kept{1}def\n"))

    def test_mixed_crlf_and_bare_cr_endings(self) -> None:
        """Nine of the 34 affected members mix CRLF with bare CR, so both endings in one file.

        The bare CR after the comment is the only thing separating it from the next statement,
        and an LF-only rule runs the comment on to the end of the CRLF line below it.
        """
        source = "/a{1}def\r\n; a comment ended by a bare CR\r/kept{2}def\r\n"
        self.assertEqual(
            tokens(source),
            ["/a", "{", "1", "}", "def", "/kept", "{", "2", "}", "def"],
        )

    def test_unterminated_trailing_comment_ends_at_end_of_source(self) -> None:
        self.assertEqual(tokens("/kept{1}def ; trailing comment with no line ending"), [
            "/kept",
            "{",
            "1",
            "}",
            "def",
        ])

    def test_carriage_return_inside_a_string_is_kept(self) -> None:
        """Also a guard rather than evidence: the string branch never consulted the comment rule."""
        self.assertEqual(tokens('"two\rlines" print'), ['"two\rlines"', "print"])

    def test_a_string_ends_at_the_first_quote_even_after_a_backslash(self) -> None:
        """The authority has no escape rule, and GameScript paths are backslash-separated.

        `read_string` in `spikes/asset-viewer/src/gamescript.rs` matches on `"` and takes every
        other byte verbatim. Treating `\\` as an escape made a string ending in one swallow its
        own closing quote and run to the next quote in the file -- the same defect as the LF-only
        comment rule, in the other scanner. Five shipped members trip it, in all three profiles.
        """
        self.assertEqual(
            tokens('/path "gs\\dungeons\\" def /next 5 def'),
            ["/path", '"gs\\dungeons\\"', "def", "/next", "5", "def"],
        )

    def test_the_shipped_punctuation_table_is_one_string(self) -> None:
        """Corpus-shaped: `gs\\Dlg\\lib_dlg.gs` in all three profiles holds this literal.

        Under the escape rule that member lexed to 3,247 tokens against its real 2,324.
        """
        self.assertEqual(
            tokens('/punctuation "@#${}()[]\\" def'),
            ["/punctuation", '"@#${}()[]\\"', "def"],
        )

    def test_an_unterminated_string_runs_to_end_of_source(self) -> None:
        """The authority calls this a parse error; this tokenizer has no error channel and keeps
        what it has, which is the pre-existing behaviour and is left unchanged."""
        self.assertEqual(tokens('/a "no closing quote'), ["/a", '"no closing quote'])


if __name__ == "__main__":
    unittest.main()


@unittest.skipUnless(
    VIEWER.is_file(), "lom-asset-viewer is not built; run cargo build --release in spikes/asset-viewer"
)
class AuthorityParityTest(unittest.TestCase):
    """The two implementations tokenize the same bytes the same way.

    Each fixture is one of the divergences closed on 2026-09-18, kept as a case that can still
    fail. The corpus-wide version of this check -- all 4,692 `.gs` members of the three installed
    profiles, zero disagreements -- is recorded in `docs/gamescript-format.md` with its evidence
    class, because the archives are not in this repository and no test here can reach them.
    """

    def facts(self, raw: bytes) -> dict:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "member.gs"
            path.write_bytes(raw)
            completed = subprocess.run(
                [str(VIEWER), "--gs-facts", str(path)],
                capture_output=True,
                check=True,
                text=True,
            )
        return json.loads(completed.stdout.splitlines()[0])

    def assert_parity(self, raw: bytes) -> None:
        """Same token stream, compared through the digest both sides compute the same way."""
        facts = self.facts(raw)
        self.assertIsNone(facts["parse_error"])
        self.assertEqual(
            hashlib.sha256(normalized_bytes(raw)).hexdigest(),
            facts["token_sha256"],
            f"token streams differ for {raw!r}",
        )

    def assert_boundary_parity(self, raw: bytes) -> None:
        """Parity for a member with a high byte, where the two digests cannot match directly.

        The Rust lexer decodes with `from_utf8_lossy`, so a byte like 0x85 becomes U+FFFD in its
        token text while this module's latin1 decoding keeps it. Applying only that decoder to this
        module's tokens -- and nothing else -- makes the digests comparable again, so the
        comparison is still about where the tokens begin. Comparing token counts instead does not
        work: a name split in two by a whitespace rule and a name that keeps its byte give the
        same total.
        """
        facts = self.facts(raw)
        self.assertIsNone(facts["parse_error"])
        lossy = "\0".join(
            token.encode("latin1").decode("utf-8", "replace") for token in tokens(raw.decode("latin1"))
        )
        self.assertEqual(
            hashlib.sha256(lossy.encode("utf-8")).hexdigest(),
            facts["token_sha256"],
            f"token streams differ for {raw!r}",
        )

    def test_a_comment_ended_by_a_bare_carriage_return(self) -> None:
        self.assert_parity(b"; header\r/kept{1}def\r; another\r/also{2}def\r")

    def test_mixed_crlf_and_bare_cr(self) -> None:
        self.assert_parity(b"/a{1}def\r\n; comment\r/kept{2}def\r\n")

    def test_a_string_ending_in_a_backslash(self) -> None:
        self.assert_parity(b'/path "gs\\dungeons\\" def /next 5 def')

    def test_the_shipped_punctuation_table(self) -> None:
        self.assert_parity(b'/punctuation "@#${}()[]\\" def')

    def test_a_slash_inside_a_name(self) -> None:
        self.assert_parity(b"/spell{ w/name \"Chain Lightning\" def }def")

    def test_control_bytes_str_isspace_accepts_and_the_authority_does_not(self) -> None:
        """`\x0b` and `\x1c`-`\x1f` are whitespace to `str.isspace()` and not to the authority.

        These stay on the digest comparison because they are ASCII: `from_utf8_lossy` leaves them
        alone, so both sides hash the same bytes and a moved token boundary is visible. Counting
        tokens would not see it -- a name split in two by a layout rule and a name that keeps its
        control byte yield the same total here.
        """
        for raw in (
            b"/hit_points\x0b 13 def",
            b"/hit_points\x1c 13 def",
            b"/hit_points\x1f 13 def",
            # The other direction: form feed IS layout to the authority, so dropping it from the
            # set would glue a name to the token after it.
            b"/hit_points\x0c13 def",
        ):
            with self.subTest(raw=raw):
                self.assert_parity(raw)

    def test_a_high_byte_str_isspace_accepts(self) -> None:
        """0x85 -- the one such byte in the corpus, in GS5R3's `shield_balkoth.gs`.

        Compared through `assert_boundary_parity`, because the two digests cannot be compared
        directly for a high byte. A token-count comparison was tried first and is not enough: the
        `str.isspace()` rule and the authority's rule both yield three tokens here, so the count
        survives the mutation that matters.
        """
        for raw in (b"/hit_points\x85 13 def", b"/hit_points\xa0 13 def"):
            with self.subTest(raw=raw):
                self.assert_boundary_parity(raw)
