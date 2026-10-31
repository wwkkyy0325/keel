import os
import tempfile
import unittest

from store import load, save


class StorageTests(unittest.TestCase):
    def test_missing_file_returns_default(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(load(os.path.join(directory, "missing.json")), {})
            self.assertEqual(
                load(os.path.join(directory, "missing.json"), {"x": 1}), {"x": 1}
            )

    def test_save_creates_parent_and_round_trips(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "nested", "data.json")
            save(path, {"b": 2, "a": 1})
            self.assertEqual(load(path), {"a": 1, "b": 2})
            with open(path, encoding="utf-8") as handle:
                self.assertTrue(handle.read().startswith('{"a"'))


if __name__ == "__main__":
    unittest.main()
