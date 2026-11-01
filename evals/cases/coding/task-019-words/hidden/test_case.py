import unittest

from words import top


class WordTests(unittest.TestCase):
    def test_counts(self):
        self.assertEqual(top("b a b c b a", 2), [("b", 3), ("a", 2)])

    def test_ties_are_alphabetical(self):
        self.assertEqual(top("b a", 2), [("a", 1), ("b", 1)])

    def test_case_and_punctuation(self):
        self.assertEqual(
            top("Hello, hello! World.", 2), [("hello", 2), ("world", 1)]
        )

    def test_apostrophe_is_kept(self):
        self.assertEqual(top("don't don't stop", 2), [("don't", 2), ("stop", 1)])

    def test_non_positive_count(self):
        self.assertEqual(top("a b", 0), [])

    def test_count_larger_than_vocabulary(self):
        self.assertEqual(top("a b", 5), [("a", 1), ("b", 1)])


if __name__ == "__main__":
    unittest.main()
