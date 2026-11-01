# Cache economics of the context recipe

Does the bounded working set cost more because it misses the provider prompt
cache? This note fixes the arithmetic so the question can be answered with
measurements instead of intuition.

## Prices

DeepSeek `deepseek-flash`, CNY per million tokens, before the October 2026
discount that is already included:

| Token kind | Off-peak | Peak |
|---|---|---|
| Input, cache miss | 1 | 2 |
| Input, cache hit | 0.02 | 0.04 |
| Output | 4 | 8 |

A cache hit is 1/50 of a miss. Hit rate therefore dominates input cost far more
than volume does.

The kernel uses peak prices below unless stated. `TokenUsage.cached_input_tokens`
is parsed from the provider and printed per turn and per model call.

## Formula

For one run:

```
input_cost = sum(input_tokens) * (P_miss * (1 - h) + P_hit * h)
total_cost = input_cost + output_tokens * P_out
```

`h` is the token-weighted cache hit rate, not the fraction of calls that hit.

## What the recipe caches

Order: `system, task_state, memory, history, latest`.

- `system` is stable, about 1.4k tokens for the novel persona.
- `task_state` changes every turn and sits second, so the cross-turn prefix ends
  after the system block.
- `memory` is rewritten whenever a segment seals; in the novel workload that is
  every turn.
- `history` is rebuilt each turn from records, digests and the memo.
- Within a turn, the history is assembled once and tool rounds only append, so
  calls 2..n hit the prefix built by call 1.

Measured on the first two turns of the 24k novel run:

| Turn | Calls | Input | Cached | Hit |
|---|---|---|---|---|
| 1 | 9 | 105,404 | 87,264 | 82.8% |
| 2 | 4 | 44,417 | 29,952 | 67.4% |

So the rate is driven by within-turn reuse and by the number of calls per turn,
not by cross-turn reuse.

## Scenarios for a 50-turn novel

Assumptions: 50 turns, output 5.5k tokens per turn (visible plus reasoning),
chapter context growth about 3k tokens per turn, four main calls per turn for an
append-only transcript.

| Scenario | Input tokens | Hit rate | Effective input price | Input cost | Output cost | Total |
|---|---|---|---|---|---|---|
| A. Bounded 24k, current order | 3.9M | 75% | 0.53 | 2.07 | 2.20 | **4.27** |
| B. Bounded 24k, volatile blocks last | 3.9M | 90% | 0.24 | 0.92 | 2.20 | **3.12** |
| C. Append-only transcript | 15.4M | 98% | 0.08 | 1.22 | 2.20 | **3.42** |
| D. Append-only, 90% hit | 15.4M | 90% | 0.24 | 3.63 | 2.20 | **5.83** |

All values are CNY for one 50-turn run at peak prices. Off-peak halves them.

Two conclusions:

1. **The current order does not "lose money" in any material sense.** The
   difference between A and the best alternative is about one yuan per run.
   Output tokens are a third of the bill and do not change with context order.
2. **The ordering penalty is real but small.** Moving `task_state` and `memory`
   after `history` (scenario B) should recover most of it. A full append-only
   transcript only wins if its hit rate stays near 98% and the model window is
   large enough to hold the transcript; at 90% it is the most expensive option.

## Why append-only is not the default

- The transcript exceeds 128k around turn 40 at this output size. Append-only
  needs a 1M-token window and long-context quality to match a 24k working set.
- The kernel supports an 8k floor. A 100k prefix cannot run there at all.
- Long context increases latency and the chance that old, absorbed material
  distracts the model. The memo and records exist for that reason.

The bounded recipe trades roughly one yuan per run for a small, predictable
context. At 10k runs the difference is about 10k CNY, which is where scenario B
becomes worth the A/B measurement rather than an assumption.

## What to measure next

The CLI now prints one `call:` line per main model call with `input`, `cached`,
`hit` and `tools`. From those lines:

- first-call hit rate per turn estimates cross-turn reuse;
- later-call hit rate estimates within-turn reuse;
- the split shows whether an ordering change moves the first call or only the
  tail.

Run the cache-order profile against the same workload before changing the
default recipe. An ordering change also moves the current task away from the
top of the prompt, so acceptance and task compliance are the guardrail, not the
hit rate alone.

## Caveats

- Prices and cache behavior are the provider's; hits are best-effort and can be
  evicted. Long gaps between turns reduce cross-turn reuse.
- The scenario table is a model, not a measurement. Only A has partial measured
  support (two turns).
- One model, one quantisation, one workload.
