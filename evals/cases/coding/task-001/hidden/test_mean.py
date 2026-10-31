import unittest

from calc import mean


class MeanTests(unittest.TestCase):
    def test_empty_list_returns_zero(self):
        self.assertEqual(mean([]), 0.0)

    def test_single_value(self):
        self.assertEqual(mean([7]), 7.0)

    def test_values(self):
        self.assertAlmostEqual(mean([2, 4, 6]), 4.0)

    def test_negative_values(self):
        self.assertAlmostEqual(mean([-3, 3]), 0.0)


if __name__ == "__main__":
    unittest.main()
