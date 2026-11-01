import unittest

from stats import mean


class MeanTest(unittest.TestCase):
    def test_empty_list_is_zero(self):
        self.assertEqual(mean([]), 0.0)

    def test_evenly_spaced_values(self):
        self.assertEqual(mean([2, 4]), 3.0)

    def test_single_value(self):
        self.assertEqual(mean([7]), 7.0)


if __name__ == "__main__":
    unittest.main()
