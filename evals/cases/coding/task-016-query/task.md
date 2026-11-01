# Parse query strings with decoding and repeated keys

`query.parse_query(text)` splits on `&` and keeps the last value for a key, without decoding. Return a mapping from key to a list of values, decode percent escapes and `+` as a space, keep repeated keys in order, treat a pair with no `=` as an empty value, and skip empty segments.

Do not change the tests. Run the test suite to confirm the fix.
