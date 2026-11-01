import unittest

from docs import toc


class TocTests(unittest.TestCase):
    def test_levels(self):
        text = "# Title\n## Part\ntext\n### Detail\n"
        self.assertEqual(toc(text), [(1, "Title"), (2, "Part"), (3, "Detail")])

    def test_code_fence_is_skipped(self):
        text = "# Title\n```\n# not a heading\n```\n## Real\n"
        self.assertEqual(toc(text), [(1, "Title"), (2, "Real")])

    def test_hashtag_is_not_a_heading(self):
        self.assertEqual(toc("#hashtag\n# Real\n"), [(1, "Real")])

    def test_level_four_is_ignored(self):
        self.assertEqual(toc("#### Deep\n## Shallow\n"), [(2, "Shallow")])

    def test_empty_title_is_ignored(self):
        self.assertEqual(toc("# \n# Real\n"), [(1, "Real")])


if __name__ == "__main__":
    unittest.main()
