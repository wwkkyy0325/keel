# Public suite — 2026-10-31

## Setup

- Profile: `variants/coding/profile.eval.toml`.
- Model: `deepseek-flash`.
- Dataset source: EvalPlus, downloaded through the local proxy and imported
  with `evals/harness/import_public.py`; cases are gitignored and not vendored.
- Harness: `evals/harness/run_suite.py`.

## HumanEval+

- Cases: 20
- Passed: 20/20
- Tokens: 115,847 (105,478 input / 10,369 output)
- Wall clock: 105 s

## MBPP+

- Cases: 20
- Passed: 19/20
- Tokens: 105,723 (93,398 input / 12,325 output)
- Wall clock: 119 s

## Failure

`Mbpp/16` asks for a function that returns true when a string "contains
sequences of lowercase letters joined with an underscore". The reference and
hidden tests implement a full-string match with repeated underscore groups
(`^[a-z]+(_[a-z]+)*$`). The model used a full-string match without repetition.
This is a genuine ambiguity in the public prompt, not an importer failure.

## Importer fixes found by the run

- MBPP rows have no `entry_point`; derive it from the canonical `code` and the
  function name used by the tests.
- Choose the test-referenced function when the canonical code defines helpers
  first (for example `is_Power_Of_Two` before `differ_At_One_Bit_Pos`).
- Generate a setup skeleton from the canonical signature so the exact function
  name is visible to the model.
- Match suite filters against POSIX paths on Windows.

## Limitations

- Function-level tasks only; they do not exercise multi-file repository work.
- One model seed and one provider.
- The datasets server required a local proxy; the download path was exercised
  through it.
- **This 20 + 20 set is the regression baseline.** Expanding to 50 + 50 was
  deferred for cost. The suite now stops at 300,000 cumulative tokens by
  default.
