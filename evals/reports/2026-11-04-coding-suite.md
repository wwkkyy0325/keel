# Coding suite

Date: 2026-11-04. Model: `deepseek-flash`, `variants/coding/profile.eval.toml`
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
python evals/harness/run_suite.py --filter coding   # curated ten only
python evals/harness/run_suite.py --max-total-tokens 300000
```

Seeds are fixed per case; the model provider is not seeded, so reruns can
differ. A case passes only when the CLI exits zero, the turn is `Done` and
`verify.sh` exits zero.
