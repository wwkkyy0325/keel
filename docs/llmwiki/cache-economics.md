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

This note uses peak prices below unless stated. `TokenUsage.cached_input_tokens`
is parsed from the provider and printed per turn and per model call.

## Formula

For one run:

```
input_cost = sum(input_tokens) * (P_miss * (1 - h) + P_hit * h)
total_cost = input_cost + output_tokens * P_out
```

`h` is the token-weighted cache hit rate, not the fraction of calls that hit.

## What the recipe caches

Runtime order is decided by stage priority, not by the order in the profile:

| Stage | Priority | Content | Changes |
|---|---|---|---|
| system | 0 | persona, about 1.4k tokens | never |
| memory | 5 | global/workspace facts (often empty) | on fact update |
| task_state | 6 | the full todo list, about 1k tokens | every turn |
| history | 9 | memo, records, recalled segments, detail | memo at every seal |
| latest | 10 | current input | every turn |

Stages are built in priority order and consume the remaining budget first come,
so priority is also the drop order under pressure. Within a turn the history is
assembled once and tool rounds only append, so calls 2..n hit the prefix built
by call 1. Cross-turn reuse stops at `task_state`: the prefix ends after the
system block.

The list order in the profile only breaks ties between equal priorities. An
"inverted" profile that lists `history` before `task_state` is a no-op at
runtime; a test loading such a profile would prove nothing.

Measured on the first two turns of the 24k novel run:

| Turn | Calls | Input | Cached | Hit |
|---|---|---|---|---|
| 1 | 9 | 105,404 | 87,264 | 82.8% |
| 2 | 4 | 44,417 | 29,952 | 67.4% |

So the rate is driven by within-turn reuse and by the number of calls per turn,
not by cross-turn reuse.

### What inversion would cost

Moving `task_state` (and the memo inside history) after the history means:

1. **Budget starvation.** The task block would be built after history has
   consumed the allowance. Under pressure it can be truncated or dropped,
   exactly when it matters. Today it survives because it is built early. Making
   inversion safe needs reserved tokens for the task block and the latest input,
   the way history currently leaves room for latest.
2. **Weaker steering.** The current task and the memo are the briefing. After a
   long transcript the model may plan from old detail instead of the current
   item. Acceptance catches missing files, not subtle task drift.
3. **A small upside.** The stable prefix grows by the task block, about 1k
   tokens. At a 50:1 hit ratio that is worth roughly one yuan per 50-turn run,
   and only when the memo does not change every turn.

The cache-friendly version of this idea is not a reorder but a rule: never
rewrite a prefix the provider may have cached. Keep the rendered history
byte-identical between turns, append the task update as a new message, and
rewrite only at a seal. That also needs a reserved budget so appended blocks
cannot be starved.

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

Scenario B is the hypothetical outcome of a stable-prefix rule, not of a
profile reorder.

Two conclusions:

1. **The current order does not "lose money" in any material sense.** The
   difference between A and the best alternative is about one yuan per run.
   Output tokens are a third of the bill and do not change with context order.
2. **The ordering penalty is real but small.** A stable-prefix rule (scenario
   B) should recover most of it. A full append-only transcript only wins if its
   hit rate stays near 98% and the model window is large enough to hold the
   transcript; at 90% it is the most expensive option.

## Large contexts: 500k and beyond

Same prices and four calls per turn, output 5.5k. The bounded row is the
measured 78k input per turn at a 75% hit rate.

| Context | Hit rate | Input per turn | Total per turn | Versus bounded |
|---|---|---|---|---|
| 24k bounded | 75% | 0.041 | 0.085 | 1.00x |
| 100k | 95% | 0.055 | 0.099 | 1.16x |
| 100k | 98% | 0.032 | 0.076 | 0.89x |
| 200k | 98% | 0.063 | 0.107 | 1.26x |
| 200k | 99% | 0.048 | 0.092 | 1.08x |
| 500k | 99% | 0.120 | 0.164 | 1.93x |
| 500k | 99.5% | 0.100 | 0.144 | 1.69x |
| 1M | 99.5% | 0.199 | 0.243 | 2.86x |
| 1M | 99.9% | 0.176 | 0.220 | 2.59x |

A large context beats the bounded working set on price only around 100k with a
98% hit rate. From 200k upward the extra tokens outweigh the discount, even at
99.5%.

The more important number is the blast radius of one miss:

| Context | One full miss (peak) |
|---|---|
| 24k | 0.048 |
| 100k | 0.20 |
| 500k | 1.00 |
| 1M | 2.00 |

The whole bounded 50-turn run costs about 2.07 in input. **One uncached 1M
prefix costs as much as the entire bounded run.**

That is why the working set and the rebuild belong together. Today the history
is rebuilt every turn, so a large-context design would not miss one prefix per
run; it would miss most of it every turn. At 500k that is about 1 per turn, or
50 per run, versus 4 for the bounded design. High hit rates are only reachable
with an append-only transcript, which is a different context architecture, not
a setting.

Large contexts also pay in ways the table does not show: decode attends to the
whole prefix even when prefill is cached, lost-in-the-middle degrades recall,
providers rate-limit and evict best-effort prefixes, and a 500k workload needs
a frontier model. The bounded design runs on an 8k floor and keeps latency flat.

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

The next experiment is a stable-prefix rule, not a reorder: keep the rendered
history byte-identical between turns and append the task update. It needs a
reserved budget for the task and latest blocks, so acceptance and task
compliance are the guardrail, not the hit rate alone.

## Caveats

- Prices and cache behavior are the provider's; hits are best-effort and can be
  evicted. Long gaps between turns reduce cross-turn reuse.
- The scenario table is a model, not a measurement. Only A has partial measured
  support (two turns).
- One model, one quantisation, one workload.
