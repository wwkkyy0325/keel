import unittest

from durations import parse


class DurationTests(unittest.TestCase):
    def test_parts(self):
        self.assertEqual(parse("1h30m"), 5400)
        self.assertEqual(parse("45s"), 45)
        self.assertEqual(parse("2m"), 120)

    def test_spaces_and_order(self):
        self.assertEqual(parse("1h 30m"), 5400)
        self.assertEqual(parse("30m1h"), 5400)

    def test_duplicate_unit(self):
        with self.assertRaises(ValueError):
            parse("1h1h")

    def test_missing_unit(self):
        with self.assertRaises(ValueError):
            parse("10")

    def test_empty(self):
        with self.assertRaises(ValueError):
            parse("")

    def test_unknown_character(self):
        with self.assertRaises(ValueError):
            parse("1d")


if __name__ == "__main__":
    unittest.main()
