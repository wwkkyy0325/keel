import unittest

from cli import parse_args


class CliTests(unittest.TestCase):
    def test_parses_pairs(self):
        self.assertEqual(
            parse_args(["prog", "--name", "keel", "--count", "2"]),
            {"name": "keel", "count": "2"},
        )

    def test_hyphen_becomes_underscore(self):
        self.assertEqual(parse_args(["prog", "--dry-run", "yes"]), {"dry_run": "yes"})

    def test_missing_value(self):
        with self.assertRaises(ValueError):
            parse_args(["prog", "--name"])

    def test_positional_is_rejected(self):
        with self.assertRaises(ValueError):
            parse_args(["prog", "oops"])


if __name__ == "__main__":
    unittest.main()
