"""The change report: what it says about a `.gs` member beyond "the bytes differ"."""

import tempfile
import unittest
from pathlib import Path

from tools.mod_report import (
    ADDED,
    MODIFIED,
    REFORMATTED,
    UNCHANGED,
    describe_gamescript_change,
    read_symbol_members,
)


def facts(
    *,
    sha256: str,
    token_sha256: str = "t" * 64,
    token_count: int = 200,
    definitions: dict | None = None,
    scalars: dict | None = None,
    executables: dict | None = None,
    line_endings: dict | None = None,
    parse_error: dict | None = None,
) -> dict:
    return {
        "sha256": sha256,
        "token_sha256": token_sha256,
        "token_count": token_count,
        "definition_names": definitions or {},
        "scalar_definitions": scalars or {},
        "executable_names": executables or {},
        "line_endings": line_endings or {"crlf": 0, "bare_cr": 0, "bare_lf": 0},
        "parse_error": parse_error,
    }


class GameScriptChangeTest(unittest.TestCase):
    def test_identical_bytes_are_unchanged_with_nothing_to_say(self) -> None:
        base = facts(sha256="a" * 64)
        status, detail = describe_gamescript_change(base, dict(base), None, Path("x"))
        self.assertEqual(status, UNCHANGED)
        self.assertEqual(detail, [])

    def test_a_scalar_value_change_is_named_with_both_values(self) -> None:
        """The whole point of the report: `hit_points: 13 -> 18`, not "1798 bytes differ"."""
        base = facts(sha256="a" * 64, scalars={"hit_points": "13", "armor": "5"})
        new = facts(
            sha256="b" * 64,
            token_sha256="u" * 64,
            scalars={"hit_points": "18", "armor": "5"},
        )
        status, detail = describe_gamescript_change(base, new, None, Path("x"))
        self.assertEqual(status, MODIFIED)
        self.assertIn("values changed: hit_points: 13 -> 18", detail)
        # Asserted in both directions: an unchanged value must not appear.
        self.assertFalse(any("armor" in line for line in detail))

    def test_layout_only_changes_are_classified_as_reformatted(self) -> None:
        base = facts(sha256="a" * 64, token_sha256="t" * 64)
        new = facts(sha256="b" * 64, token_sha256="t" * 64)
        status, detail = describe_gamescript_change(base, new, None, Path("x"))
        self.assertEqual(status, REFORMATTED)
        self.assertIn("layout and comments only", detail[0])

    def test_a_token_count_delta_is_reported_with_its_sign(self) -> None:
        base = facts(sha256="a" * 64, token_count=200)
        new = facts(sha256="b" * 64, token_sha256="u" * 64, token_count=203)
        _, detail = describe_gamescript_change(base, new, None, Path("x"))
        self.assertIn("tokens 200 -> 203 (+3)", detail)

        new_smaller = facts(sha256="b" * 64, token_sha256="u" * 64, token_count=197)
        _, detail = describe_gamescript_change(base, new_smaller, None, Path("x"))
        self.assertIn("tokens 200 -> 197 (-3)", detail)

    def test_definitions_and_call_sites_are_reported_in_both_directions(self) -> None:
        base = facts(
            sha256="a" * 64, definitions={"gone": 1, "kept": 1}, executables={"old": 1}
        )
        new = facts(
            sha256="b" * 64,
            token_sha256="u" * 64,
            definitions={"kept": 1, "fresh": 1},
            executables={"new": 1},
        )
        _, detail = describe_gamescript_change(base, new, None, Path("x"))
        self.assertIn("definitions added: fresh", detail)
        self.assertIn("definitions dropped: gone", detail)
        self.assertIn("now calls: new", detail)
        self.assertIn("no longer calls: old", detail)
        self.assertFalse(any("kept" in line for line in detail))

    def test_a_line_ending_change_is_reported(self) -> None:
        base = facts(sha256="a" * 64)
        new = facts(
            sha256="b" * 64,
            token_sha256="u" * 64,
            line_endings={"crlf": 0, "bare_cr": 0, "bare_lf": 34},
        )
        _, detail = describe_gamescript_change(base, new, None, Path("x"))
        self.assertTrue(any("line endings" in line for line in detail))

    def test_a_new_member_is_reported_as_added(self) -> None:
        new = facts(sha256="b" * 64)
        status, detail = describe_gamescript_change(None, new, None, Path("x"))
        self.assertEqual(status, ADDED)
        self.assertIn("new member, 200 tokens", detail)

    def test_a_file_that_does_not_lex_says_so_rather_than_being_skipped(self) -> None:
        base = facts(sha256="a" * 64)
        new = facts(
            sha256="b" * 64,
            parse_error={"message": "unterminated string", "line": 4, "column": 9},
        )
        status, detail = describe_gamescript_change(base, new, None, Path("x"))
        self.assertEqual(status, MODIFIED)
        self.assertIn("DOES NOT LEX at line 4 column 9", detail[0])

    def test_a_byte_change_with_no_sub_finding_still_says_something(self) -> None:
        """A silent row would read as "we found nothing" rather than "we looked and found none"."""
        base = facts(sha256="a" * 64)
        new = facts(sha256="b" * 64, token_sha256="u" * 64, token_count=200)
        _, detail = describe_gamescript_change(base, new, None, Path("x"))
        self.assertEqual(detail, ["tokens 200 -> 200 (+0)"])


class PythonLexerDisagreementTest(unittest.TestCase):
    """`tools/gs_syntax.py` ends a `;` comment at LF only. The report says when that matters.

    A bare-CR member with a comment normalises, in Python, to far fewer tokens than it has, so the
    Python answer to "did the tokens change" can differ from the CR-aware Rust answer. Reporting
    the disagreement turns the repo's recorded latent defect into a visible finding instead of a
    quietly wrong column.
    """

    def test_a_disagreement_is_reported_and_the_rust_answer_is_used(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            # Everything after the `;` is one comment to gs_syntax.py, so both files normalise to
            # the same (empty) token stream there, while the real token streams differ.
            base_path = root / "base.gs"
            new_path = root / "new.gs"
            base_path.write_bytes(b"; note\r/hit_points 13 def")
            new_path.write_bytes(b"; note\r/hit_points 18 def")

            base = facts(sha256="a" * 64, token_sha256="t" * 64)
            new = facts(sha256="b" * 64, token_sha256="u" * 64)
            status, detail = describe_gamescript_change(base, new, base_path, new_path)

        self.assertEqual(status, MODIFIED)
        self.assertTrue(any("DISAGREES" in line for line in detail))
        self.assertTrue(any("gs_syntax.py" in line for line in detail))

    def test_agreement_produces_no_disagreement_line(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            base_path = root / "base.gs"
            new_path = root / "new.gs"
            base_path.write_bytes(b"/hit_points 13 def")
            new_path.write_bytes(b"/hit_points 18 def")

            base = facts(sha256="a" * 64, token_sha256="t" * 64)
            new = facts(sha256="b" * 64, token_sha256="u" * 64)
            _, detail = describe_gamescript_change(base, new, base_path, new_path)

        self.assertFalse(any("DISAGREES" in line for line in detail))


class SymbolIndexTest(unittest.TestCase):
    def test_symbols_are_read_for_the_named_profile_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "symbols.tsv"
            path.write_text(
                "name\tkind\tevidence\tprofiles\tmember\tline\n"
                "orinf\tunit\tx\tvanilla,patch302,gs5r3\tunits\\orinf.gs\t1\n"
                "custom\tartifact\tx\tgs5r3\tgs\\custom.gs\t1\n",
                encoding="utf-8",
            )
            vanilla = read_symbol_members(path, "vanilla")
            gs5r3 = read_symbol_members(path, "gs5r3")

        self.assertEqual(vanilla["units\\orinf.gs"], ["orinf (unit)"])
        self.assertNotIn("gs\\custom.gs", vanilla)
        self.assertIn("gs\\custom.gs", gs5r3)

    def test_a_missing_symbols_file_yields_an_empty_index_rather_than_raising(self) -> None:
        self.assertEqual(read_symbol_members(Path("/nonexistent/symbols.tsv"), "vanilla"), {})


if __name__ == "__main__":
    unittest.main()
