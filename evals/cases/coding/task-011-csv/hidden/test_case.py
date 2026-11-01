import unittest

from csvlite import parse


class ParseTests(unittest.TestCase):
    def test_simple_rows(self):
        self.assertEqual(parse("a,b\nc,d\n"), [["a", "b"], ["c", "d"]])

    def test_quoted_comma(self):
        self.assertEqual(parse('a,"b,c"\n'), [["a", "b,c"]])

    def test_doubled_quote(self):
        self.assertEqual(parse('"a""b",c\n'), [['a"b', "c"]])

    def test_newline_inside_quotes(self):
        self.assertEqual(parse('"line1\nline2",x\n'), [["line1\nline2", "x"]])

    def test_no_trailing_newline(self):
        self.assertEqual(parse("a,b"), [["a", "b"]])


if __name__ == "__main__":
    unittest.main()
