import unittest

from tools.gs_syntax import tokens


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

    def test_comment_ends_at_the_carriage_return_of_a_crlf_pair(self) -> None:
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
        self.assertEqual(tokens('"two\rlines" print'), ['"two\rlines"', "print"])


if __name__ == "__main__":
    unittest.main()
