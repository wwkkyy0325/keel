# Mean of an empty list

`calc.mean(values)` currently divides by the list length. Calling it with an
empty list raises `ZeroDivisionError`.

Change `mean` so an empty list returns `0.0`. Keep the existing behavior for
non-empty lists.

Do not change the tests. Run the test suite to confirm the fix.
