# Validate CLI arguments

`cli.parse_args(argv)` accepts `--name value` pairs. It currently crashes on a missing value and silently accepts positional arguments. Raise `ValueError` for missing values, unknown positional arguments, and flags without a value. Convert hyphens in option names to underscores.

Do not change the tests. Run the test suite to confirm the fix.
