import unittest

from unique import unique


class UniqueTests(unittest.TestCase):
    def test_preserves_first_occurrence_order(self):
        self.assertEqual(unique(["b", "a", "b", "c", "a"]), ["b", "a", "c"])

    def test_empty(self):
        self.assertEqual(unique([]), [])

    def test_input_not_mutated(self):
        items = ["b", "a", "b"]
        unique(items)
        self.assertEqual(items, ["b", "a", "b"])


if __name__ == "__main__":
    unittest.main()
