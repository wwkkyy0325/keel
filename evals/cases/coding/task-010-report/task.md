# Robust record totals

`report.total(path)` reads JSON Lines records and sums `amount`. It currently crashes on a blank or malformed line and accepts negative amounts. Skip blank and malformed lines, sum only positive numeric amounts, and ignore missing or non-numeric amounts.

Do not change the tests. Run the test suite to confirm the fix.
