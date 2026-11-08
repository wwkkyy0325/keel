# How much of a turn's input is tool results

Date: 2026-11-07. Model: `deepseek-flash`, profile
`variants/coding/profile.deepseek.consumer.toml` (24k working set, `full_auto`,
`shell`, `code_search`, `todo`).

## Why

Programmatic tool calling (PTC) moves orchestration into model-written code so
intermediate tool results never enter the context. It is worth weeks of work
only if tool results dominate the context in the first place. This run measures
that premise on our own workloads before considering any implementation.

## Method

`run_case.py --keep` now keeps the eval state home as well as the workspace, so
the persisted event stream (`keel.db`) survives the run. `tool_share.py`
computes, for every consecutive pair of model calls in a turn:

```
injected(n) = input(n) - input(n-1) - output(n-1)
```

The next request carries the earlier conversation, the assistant message whose
size is the previous output count, and the new tool results; the residual is
what the tools added. A reminder or an elided segment moves the number in
either direction, so negative deltas are counted rather than clipped (none
appeared here).

Three cases, one run each, chosen to span the range: a one-file fix, a
data-plus-code fix, and a twenty-file refactor.

## Results

| Case | Model calls | Tool calls | Input | Injected | Share of input | Longest read chain |
|---|---|---|---|---|---|---|
| task-001 (one file) | 8 | 7 | 17,380 | 346 | 2.2% | 2 |
| task-010-report (data + code) | 8 | 10 | 29,549 | 2,716 | 9.7% | 4 |
| long-refactor (20 files) | 12 | 13 | 57,311 | 2,785 | 5.0% | 4 |

Context peaked at 2,813 / 5,596 / 6,948 tokens against a 24,000 budget: no case
came close to context pressure.

Cost ceiling, if every tool result disappeared entirely (miss 1.0, cache hit
0.1, output 4.0; `ceil` also prices the re-sends a token would have made):

| Case | Cost units | Ceiling | Realistic at 70% removed |
|---|---|---|---|
| task-001 | 8,394 | 5.3% | 3.7% |
| task-010-report | 14,536 | 25.3% | 17.7% |
| long-refactor | 22,784 | 21.4% | 15.0% |

## Findings

1. **The premise does not hold.** Tool results are 2-10% of model input on
   these workloads, not the majority. The largest case, twenty files, injects
   5%: the model reads one file and a search result, then uses `shell` for the
   bulk edit, so twenty file edits cost thirteen tool calls and little context.
2. **The shell already is our programmatic tool call.** In `long-refactor` the
   model never read all twenty modules; it ran a bulk edit through `shell`.
   Intermediate content stayed in the process, not the context. What PTC adds
   over that is structured per-call permission and structured results, not a
   smaller context.
3. **Cost, not tokens, is where the remaining case lives.** Because
   everything after the first call is cached, the injected content is
   disproportionately cache-miss priced: eliminating it entirely would reach
   15-25% of cost on the two larger cases. PTC would also pay output tokens to
   write the orchestration code, priced at four times input, so the real figure
   is lower still.
4. **Round trips are high.** 8, 8 and 12 model calls per turn. That is the one
   number that supports a code-orchestration mode, and it is a latency argument
   rather than a token argument.

## Decision

The pre-committed rule was: adopt only if tool results are at least 50% of
input and a turn takes at least 8 model calls. The second condition holds, the
first fails by a factor of five to twenty. **Do not build a code-execution
sandbox for this.** The kernel keeps the dispatcher as the only path to a tool,
which is what preserves the permission gate, path confinement and the
read-only/parallel policy.

Revisit if a workload appears where a single task requires reading large parts
of a large repository; re-run `tool_share.py` on it first.

## Limitations

- Three small-workspace cases, one run each. The variance note from the
  ablation report applies: these are observations, not a repeated experiment.
- The delta method attributes the whole residual to tool results. Reminders,
  the assistant's tool-call message and any elision net out inside it.
- Price ratios are assumptions (cache hit 0.1, output 4.0); the token shares do
  not depend on them.

## Side finding

`long-refactor` passed its verification (`python check.py` exit 0, state
`Done`) but fails the case's mechanism rule, `tool_calls 13 < 20`: the bulk
`shell` edit replaced twenty `write_file` calls. The threshold encodes an
assumption about how the work is done, not whether it was done. Either the
profile or the threshold should change.

## Reproduction

```bash
CARGO_TARGET_DIR=target/verify python evals/harness/run_case.py \
  evals/cases/coding/task-010-report --keep \
  --profile variants/coding/profile.deepseek.consumer.toml
python evals/harness/tool_share.py <state_home from the report> --detail
```

The three runs cost about 105k tokens, roughly ¥0.13 at peak `deepseek-flash`
prices.
