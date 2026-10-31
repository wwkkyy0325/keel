# Fix greeting across two modules

The `app` package formats names in `app/format.py` and builds a greeting in `app/main.py`. The greeting currently swaps the names and leaves a trailing space when one part is empty. Fix both modules so the full name is ordered correctly. `greeting` must return `Hello, <first> <last>!`, or `Hello!` when the name is empty. Treat `None` and blank parts as absent.

Do not change the tests. Run the test suite to confirm the fix.
