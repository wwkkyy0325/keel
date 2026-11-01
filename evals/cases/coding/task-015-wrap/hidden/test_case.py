import unittest

from textwrap_lite import wrap


class WrapTests(unittest.TestCase):
    def test_wraps_on_spaces(self):
        self.assertEqual(wrap("one two three", 7), "one two\nthree")

    def test_long_word_stays_intact(self):
        self.assertEqual(wrap("abcdefghij", 3), "abcdefghij")

    def test_explicit_newlines_are_kept(self):
        self.assertEqual(wrap("a\nb", 10), "a\nb")

    def test_fits_on_one_line(self):
        self.assertEqual(wrap("a b c", 10), "a b c")

    def test_invalid_width(self):
        with self.assertRaises(ValueError):
            wrap("a", 0)


if __name__ == "__main__":
    unittest.main()
