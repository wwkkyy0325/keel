# Parse quoted CSV fields

`csvlite.parse(text)` splits every line on commas, which breaks quoted fields. Parse the text into rows with quoted fields, doubled quotes inside a quoted field, and commas or newlines inside quotes. A trailing newline does not add a row.

Do not change the tests. Run the test suite to confirm the fix.
