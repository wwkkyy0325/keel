# Window matrix — 2026-11-03

Synthetic run of `crates/keel-runtime/tests/window_matrix.rs` to place the
supported context floors with evidence instead of guesses.

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

## Decision

- Coding floor stays at 16k; no-tool floor stays at 8k. The provisional values
  in ADR 0006 are now measured values for this workload.
- The matrix is deterministic and cheap; rerun it when the token estimate or
  the context stages change.

## Limits

- One synthetic growth shape and one tool shape; a real provider tokenizer can
  differ from the four-character estimate.
- The mock model never retries or fails, so the numbers measure context
  pressure only.
