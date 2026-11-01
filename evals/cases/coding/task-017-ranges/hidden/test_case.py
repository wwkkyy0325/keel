import unittest

from ranges import merge


class MergeTests(unittest.TestCase):
    def test_overlap(self):
        self.assertEqual(merge([(1, 3), (2, 5), (8, 9)]), [(1, 5), (8, 9)])

    def test_adjacent(self):
        self.assertEqual(merge([(1, 2), (3, 4)]), [(1, 4)])

    def test_gap_is_kept(self):
        self.assertEqual(merge([(1, 2), (4, 5)]), [(1, 2), (4, 5)])

    def test_unsorted_input(self):
        self.assertEqual(merge([(8, 9), (1, 3), (2, 5)]), [(1, 5), (8, 9)])

    def test_invalid_range(self):
        with self.assertRaises(ValueError):
            merge([(3, 1)])


if __name__ == "__main__":
    unittest.main()
