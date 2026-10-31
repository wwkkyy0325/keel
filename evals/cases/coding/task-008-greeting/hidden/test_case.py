import unittest

from app.format import format_name
from app.main import greeting


class GreetingTests(unittest.TestCase):
    def test_name_order(self):
        self.assertEqual(format_name("Ada", "Lovelace"), "Ada Lovelace")

    def test_missing_part(self):
        self.assertEqual(format_name("Ada", "  "), "Ada")
        self.assertEqual(format_name(None, "Lovelace"), "Lovelace")

    def test_greeting(self):
        self.assertEqual(greeting("Ada", "Lovelace"), "Hello, Ada Lovelace!")

    def test_empty_greeting(self):
        self.assertEqual(greeting("", " "), "Hello!")


if __name__ == "__main__":
    unittest.main()
