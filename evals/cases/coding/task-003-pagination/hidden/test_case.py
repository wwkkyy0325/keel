import unittest

from pagination import page


class PaginationTests(unittest.TestCase):
    def test_first_page(self):
        self.assertEqual(page([1, 2, 3], 1, 2), [1, 2])

    def test_second_page(self):
        self.assertEqual(page([1, 2, 3], 2, 2), [3])

    def test_past_end_is_empty(self):
        self.assertEqual(page([1, 2, 3], 5, 2), [])

    def test_invalid_values_raise(self):
        with self.assertRaises(ValueError):
            page([1], 0, 1)
        with self.assertRaises(ValueError):
            page([1], 1, 0)


if __name__ == "__main__":
    unittest.main()
