import unittest

from cache import LRUCache


class CacheTests(unittest.TestCase):
    def test_evicts_least_recent(self):
        cache = LRUCache(2)
        cache.put("a", 1)
        cache.put("b", 2)
        cache.put("c", 3)
        self.assertIsNone(cache.get("a"))
        self.assertEqual(cache.get("b"), 2)
        self.assertEqual(cache.get("c"), 3)

    def test_get_refreshes_recency(self):
        cache = LRUCache(2)
        cache.put("a", 1)
        cache.put("b", 2)
        self.assertEqual(cache.get("a"), 1)
        cache.put("c", 3)
        self.assertIsNone(cache.get("b"))
        self.assertEqual(cache.get("a"), 1)

    def test_update_keeps_one_entry(self):
        cache = LRUCache(2)
        cache.put("a", 1)
        cache.put("a", 2)
        self.assertEqual(cache.get("a"), 2)
        self.assertEqual(len(cache.data), 1)

    def test_invalid_capacity(self):
        for value in (0, -1, 1.5, True):
            with self.assertRaises(ValueError):
                LRUCache(value)


if __name__ == "__main__":
    unittest.main()
