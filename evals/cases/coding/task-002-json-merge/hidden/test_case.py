import unittest

from config import merge


class MergeTests(unittest.TestCase):
    def test_does_not_mutate_base(self):
        base = {"a": {"x": 1, "y": 2}}
        merged = merge(base, {"a": {"y": 3}})
        self.assertEqual(base, {"a": {"x": 1, "y": 2}})
        self.assertEqual(merged, {"a": {"x": 1, "y": 3}})

    def test_adds_new_keys(self):
        self.assertEqual(merge({"a": 1}, {"b": 2}), {"a": 1, "b": 2})

    def test_override_scalar(self):
        self.assertEqual(merge({"a": {"x": 1}}, {"a": 3}), {"a": 3})


if __name__ == "__main__":
    unittest.main()
