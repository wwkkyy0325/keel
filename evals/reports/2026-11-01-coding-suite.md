# Coding suite

Date: 2026-11-01. Model: `deepseek-flash`, `variants/coding/profile.eval.toml`
(16k working set, four file tools, no shell). Command:

```bash
python evals/harness/run_suite.py --max-total-tokens 300000
```

The suite discovers every `meta.toml` under `evals/cases`, so it ran the ten
curated coding cases plus the gitignored public cache that had already been
generated locally: 20 HumanEval+ cases and 26 MBPP+ cases (16 ran, 4 skipped by
the token budget). Public cases are not in git; reproduce them with
`evals/harness/import_public.py` before running the full set.

## Results

| Group | Cases | Passed | Notes |
|---|---|---|---|
| Curated coding | 10 | 10 | all `Done`, `verify.sh` exit 0 |
| HumanEval+ | 20 | 20 | all `Done` |
| MBPP+ | 16 | 13 | 3 failures below |
| **Total** | **46** | **43** | 4 more MBPP cases skipped at the 300k budget |

- 302,068 tokens: 47,312 uncached input, 222,208 cached, 32,548 output.
  Weighted cache hit 82.4%.
- About ¥0.36 at peak `deepseek-flash` prices. Three reruns for failure
  analysis added roughly ¥0.04.
- All 46 CLI runs exited 0 and reached `Done`; no timeouts.

## Failures

| Case | Symptom | Diagnosis |
|---|---|---|
| MBPP+ 16 | `text_lowercase_underscore` returns False for a single word | Genuine. The model used `^[a-z]+(_[a-z]+)+$`; the benchmark expects a single lowercase word to be True (reference uses `*`). The prompt says "sequences ... joined with an underscore", which is ambiguous. |
| MBPP+ 20 | `is_woodall(True)` returns False | Genuine. Python treats `True` as `1`, and 1 is Woodall; the model excluded booleans. All numeric cases passed. |
| MBPP+ 3 | first run: test runner ERROR | Transient. Rerun passed with the same prompt and profile; no workspace difference was found. |

Two of the three are specification-boundary failures, not tool or loop
failures: the model wrote plausible code, and the hidden generated tests take
the other reading. The curated ten do not have this ambiguity.

## Harness fix

`run_case.py` still matched the pre-cache `usage: input=...` line, so every
report carried `usage: null`. It now parses
`usage: calls=… input=… output=… cached=… hit=…% total=…` and stores
`cached_tokens` too. Mock-profile runs confirmed the parser without API cost.

## Reproduction

```bash
python evals/harness/import_public.py --dataset humaneval --download --limit 20
python evals/harness/import_public.py --dataset mbpp --download --limit 30
python evals/harness/run_suite.py --filter coding   # curated set only
python evals/harness/run_suite.py --max-total-tokens 300000
```

Seeds are fixed per case; the model provider is not seeded, so reruns can
differ. A case passes only when the CLI exits zero, the turn is `Done` and
`verify.sh` exits zero.

## Curated set after five additions (2026-11-01)

Five self-authored cases were added, bringing the curated set to 15:
`task-011-csv` (quoted CSV), `task-012-lru` (LRU eviction), `task-013-toc`
(table of contents outside code fences), `task-014-semver` (numeric version
comparison) and `task-015-wrap` (word wrapping). Each was validated twice
before the model ran: the setup fails `verify.sh`, and the reference solution
passes it.

```bash
python evals/harness/run_suite.py --filter coding --max-total-tokens 200000
```

| Metric | Value |
|---|---|
| Cases | 15 curated |
| Passed | 14 |
| Tokens | 189,069 (24,560 uncached, 138,624 cached, 25,885 output) |
| Cache hit | 84.9% |
| Cost | about ¥0.26 at peak prices |

`task-013-toc` failed its first model run on the empty-heading boundary; the
task text did not state that a heading with no title is ignored. The sentence
was added to the prompt and the case passed on rerun. That is a prompt clarity
fix, not a hidden-test relaxation.

`task-008-greeting` failed in this run with `state=Error` and `verification did
not run`, although `verify.sh` passed on the final workspace. The trace shows
six tool rounds and no final text round: the model wrote both files and stopped
without closing the turn. The same case passed in the earlier 46-case run, so
this is model variance and a turn-closure weakness, not a case defect. A turn
that ends without an answer is reported as Error by design; the host recovery
checks the checklist but does not relabel the turn.

## Twenty-case curated set (2026-11-01)

The last five cases were added: `task-016-query` (query strings),
`task-017-ranges` (interval merging), `task-018-fsm` (state machine errors),
`task-019-words` (frequency ranking) and `task-020-duration` (compact
durations). All twenty were re-run with the same profile and budget:

| Metric | Value |
|---|---|
| Cases | 20 |
| Passed | **20** |
| Tokens | 286,042 (35,223 uncached, 208,768 cached, 42,051 output) |
| Cache hit | 85.6% |
| Cost | about ¥0.42 at peak prices |
| Slowest | task-004-cli-args, 37 s |

Each new case was validated before the run: the setup fails `verify.sh` and
the reference passes it. This run includes the previously variable
`task-008-greeting`, which passed here, so the earlier failure remains
classified as model variance rather than a case defect. The curated set now
meets the M6 target of twenty cases.
