import unittest

from tools.gs_syntax import tokens


class GsSyntaxTest(unittest.TestCase):
    def test_ignores_layout_and_comments_but_preserves_strings(self) -> None:
        compact = 'hidecursor"gs/text.gs"run{name 1 def}if'
        formatted = 'hidecursor\n"gs/text.gs" run { name 1 def } if ; note\n'
        self.assertEqual(tokens(compact), tokens(formatted))

    def test_semicolon_inside_string_is_not_a_comment(self) -> None:
        self.assertEqual(tokens('"keep;this" print'), ['"keep;this"', "print"])


if __name__ == "__main__":
    unittest.main()
