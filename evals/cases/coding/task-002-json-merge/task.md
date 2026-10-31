# Merge nested configuration without mutating the base

`config.merge(base, override)` currently mutates `base` and only shallow-merges nested dictionaries. Return a new merged mapping, keep nested keys that are not overridden, and do not mutate either input.

Do not change the tests. Run the test suite to confirm the fix.
