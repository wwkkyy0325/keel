# Wrap text on word boundaries

`textwrap_lite.wrap(text, width)` cuts every line at a fixed width, splitting words. Wrap at spaces instead, keep words longer than the width intact, preserve explicit newlines, and raise `ValueError` for a non-positive width.

Do not change the tests. Run the test suite to confirm the fix.
