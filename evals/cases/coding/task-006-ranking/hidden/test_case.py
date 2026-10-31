import unittest

from ranking import rank


class RankingTests(unittest.TestCase):
    def test_score_then_name(self):
        players = [
            {"name": "zoe", "score": 10},
            {"name": "amy", "score": 10},
            {"name": "bob", "score": 12},
        ]
        self.assertEqual(
            rank(players),
            [
                {"name": "bob", "score": 12},
                {"name": "amy", "score": 10},
                {"name": "zoe", "score": 10},
            ],
        )

    def test_input_not_mutated(self):
        players = [{"name": "zoe", "score": 1}, {"name": "amy", "score": 2}]
        rank(players)
        self.assertEqual(players[0]["name"], "zoe")

    def test_empty(self):
        self.assertEqual(rank([]), [])


if __name__ == "__main__":
    unittest.main()
