# Synthetic window matrix — 2026-11-03

Run of `crates/keel-runtime/tests/window_matrix.rs`. It verifies the kernel
machinery deterministically: budget arithmetic, digests, records, wrap-up and
overflow branches. It does **not** prove real token counts or model behaviour;
the provider-specific evidence lives in
`evals/reports/2026-11-03-companion-20-turn.md`.

## Setup

- 40 turns per window, mock model, deterministic.
- Each turn answers with 2000 characters (about 500 estimated tokens).
- The coding variant also makes one `echo` call per turn with a 2000-character
  result, so history carries a `[tools]` block.
- `context_window` is the window under test; `budget_tokens` is the window minus
  the output reserve (4096 coding, 1024 companion), so the working set matches
  the capacity.
- Token counts use the kernel estimate (four characters per token), not a
  provider tokenizer.

## Results

| Window | Variant | Done | Overflow | Wrap-ups | Digest turns | Records |
|---|---|---|---|---|---|---|
| 8k | companion | 40/40 | 0 | 0 | 15 | 48 |
| 16k | companion | 40/40 | 0 | 0 | 0 | 48 |
| 32k | companion | 40/40 | 0 | 0 | 0 | 48 |
| 64k | companion | 40/40 | 0 | 0 | 0 | 48 |
| 128k | companion | 40/40 | 0 | 0 | 0 | 48 |
| 1M | companion | 40/40 | 0 | 0 | 0 | 48 |
| 8k | coding | 37/40 | 3 | 35 | 136 | 48 |
| 16k | coding | 40/40 | 0 | 0 | 75 | 48 |
| 32k | coding | 40/40 | 0 | 0 | 0 | 48 |
| 64k | coding | 40/40 | 0 | 0 | 0 | 48 |
| 128k | coding | 40/40 | 0 | 0 | 0 | 48 |
| 1M | coding | 40/40 | 0 | 0 | 0 | 48 |

## Findings

- A no-tool profile is reliable at 8k: 40/40, with digest compression used only
  in the smallest window.
- A tool-using coding profile is reliable at 16k: 40/40, with 75 digest turns
  of compression work. At 32k and above the same 40 turns need no digests.
- 8k with tools is below the floor: 37/40, three `ContextOverflow` turns and 35
  wrap-up injections. The governor warns and wraps up, but the turn does not
  always finish in time.
- Segments seal every 12 turns by the size cap in every run, which is why each
  window shows record entries.

## What this proves and does not prove

- Proves: the stages, governor and compression branches execute, and the
  budget arithmetic is internally consistent at each window.
- Does not prove: real tokenizer counts, provider template overhead, or whether
  a model answers well after compression. The mock reports usage computed by
  the same estimator, so it cannot calibrate that estimator.

## Decision

- Keep the matrix as a cheap regression for the machinery.
- Keep the 16k coding and 8k no-tool floors as configured values. The 8k
  no-tool floor now has real-model evidence from the 20-turn companion run;
  the coding floor has real single-turn evidence but not a real 20-turn run.
- Recalibrate the estimate against provider usage whenever the model changes.

## Limits

- One synthetic growth shape and one tool shape; a real provider tokenizer can
  differ from the four-character estimate.
- The mock model never retries or fails, so the numbers measure context
  pressure only.
