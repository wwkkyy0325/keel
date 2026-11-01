import unittest

from query import parse_query


class QueryTests(unittest.TestCase):
    def test_basic(self):
        self.assertEqual(parse_query("a=1&b=2"), {"a": ["1"], "b": ["2"]})

    def test_repeated_key_keeps_order(self):
        self.assertEqual(parse_query("a=1&a=3"), {"a": ["1", "3"]})

    def test_decoding(self):
        self.assertEqual(parse_query("q=hello+world%21"), {"q": ["hello world!"]})

    def test_missing_value(self):
        self.assertEqual(parse_query("flag"), {"flag": [""]})

    def test_empty_segments(self):
        self.assertEqual(
            parse_query("a=1&&b=2&"), {"a": ["1"], "b": ["2"]}
        )


if __name__ == "__main__":
    unittest.main()
