# Compare dotted versions numerically

`versions.compare(left, right)` compares the strings themselves, so `1.2.10` sorts before `1.2.9`. Compare the first three numeric components of each version numerically, treat missing components as zero, and ignore non-numeric suffixes.

Do not change the tests. Run the test suite to confirm the fix.
