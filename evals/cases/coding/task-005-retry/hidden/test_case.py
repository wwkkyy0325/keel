import unittest

from retry import retry


class RetryTests(unittest.TestCase):
    def test_returns_first_success(self):
        self.assertEqual(retry(lambda: 7, 3), 7)

    def test_retries_value_error(self):
        calls = {"count": 0}

        def operation():
            calls["count"] += 1
            if calls["count"] < 3:
                raise ValueError("not yet")
            return "ok"

        self.assertEqual(retry(operation, 3), "ok")
        self.assertEqual(calls["count"], 3)

    def test_non_retryable_error_propagates(self):
        with self.assertRaises(RuntimeError):
            retry(lambda: (_ for _ in ()).throw(RuntimeError("boom")), 3)

    def test_exhaustion_reraises_last_error(self):
        with self.assertRaises(ValueError):
            retry(lambda: (_ for _ in ()).throw(ValueError("nope")), 2)

    def test_zero_attempts_is_rejected(self):
        with self.assertRaises(ValueError):
            retry(lambda: 1, 0)


if __name__ == "__main__":
    unittest.main()
