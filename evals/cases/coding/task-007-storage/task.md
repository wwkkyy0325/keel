# Safe JSON storage

`store.load(path)` raises when the file is missing, and `store.save(path, value)` fails when the parent directory does not exist. Make `load` return the supplied default (or `{}`), make `save` create parent directories, write UTF-8, and keep keys deterministic.

Do not change the tests. Run the test suite to confirm the fix.
