import unittest

from versions import compare


class CompareTests(unittest.TestCase):
    def test_numeric_not_lexical(self):
        self.assertEqual(compare("1.2.10", "1.2.9"), 1)
        self.assertEqual(compare("1.9.9", "1.10.0"), -1)

    def test_equal(self):
        self.assertEqual(compare("1.2.3", "1.2.3"), 0)

    def test_missing_component_is_zero(self):
        self.assertEqual(compare("1.2", "1.2.0"), 0)

    def test_suffix_ignored(self):
        self.assertEqual(compare("2.0.0-beta", "2.0.0"), 0)


if __name__ == "__main__":
    unittest.main()
