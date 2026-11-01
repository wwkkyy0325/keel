# Parse compact durations

`durations.parse(text)` reads only the last character and fails on multi-part input. Parse a compact duration such as `1h30m` or `45s` into seconds, accept `s`, `m` and `h` in any order with optional spaces, reject an empty string, a repeated unit, a missing unit and any other character with `ValueError`.

Do not change the tests. Run the test suite to confirm the fix.
