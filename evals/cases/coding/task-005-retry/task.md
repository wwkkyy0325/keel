# Retry only the intended exceptions

`retry.retry(operation, attempts)` retries every exception and loses the last error. Retry only `ValueError` by default, return the first success, re-raise the last matching error after the final attempt, and reject `attempts < 1` with `ValueError`.

Do not change the tests. Run the test suite to confirm the fix.
