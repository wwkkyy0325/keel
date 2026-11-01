# M6 evals

Evidence has to be reproducible: the command, profile, model, budget and the
raw per-case report. Model runs stay local because they need a key; CI covers
the deterministic parts.

## Steps

- [x] Ten coding cases under `evals/cases/coding/`, each with setup, hidden
      tests, reference solution and `verify.sh`.
- [x] Run the full suite against `deepseek-flash` and record pass/fail,
      latency, tokens and cache hits. Verify:
      `evals/reports/2026-11-04-coding-suite.md`: 43/46, 302k tokens, 82.4%
      cache hit, ¥0.36; harness usage parsing fixed in the same change.
- [ ] Grow the coding set to twenty cases (15 now; task-011 to task-015 were
      added and validated).
- [ ] Five companion personas and a 20-turn run per persona.
- [x] CI runs fmt, clippy, tests and `cargo deny`; the harness job compiles the
      scripts and validates the dataset importer without a model key.
- [ ] Reproducible report: command, profile, model version, date, token
      budget and failures with their logs.
- [ ] README, architecture note and case study.
- [ ] License and asset audit.

## Suite runs

| Date | Profile | Cases | Passed | Tokens | Notes |
|---|---|---|---|---|---|
| 2026-11-04 | `variants/coding/profile.eval.toml` | 46 | 43 | 302,068 | curated 10/10, HumanEval 20/20, MBPP 13/16; 4 skipped at 300k; ¥0.36; report `2026-11-04-coding-suite.md` |
| 2026-11-04 | `variants/coding/profile.eval.toml` | 15 | 14 | 189,069 | curated set only; 84.9% cache hit; ¥0.26; task-008 ended without a final answer while its tests passed |

## Notes

- `run_case.py` parses the `usage:` line; it was updated to the current format
  with `calls`, `cached` and `hit`, so reports carry cache numbers again.
- A case passes only when the CLI exits zero, the turn is `Done` and
  `verify.sh` exits zero.
