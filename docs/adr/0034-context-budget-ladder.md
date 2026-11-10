# ADR 0034 — A context budget ladder

## Status

Accepted.

## Context

`[context] budget_tokens` was a fixed working set per model call. It protects
cost and quality, but it is a policy value from an earlier model generation:
with a 1M-token window the kernel still compresses history at 64k and never
uses the capacity the model actually has. Raising the fixed value for everyone
would send more raw text (and cost) on turns that do not need it; lowering it
would force early compression on turns that do.

The budget is a ceiling, not a reservation: content is only billed when it is
actually assembled. The real cost of a larger working set appears only when a
session has that much content, and the real quality cost is dilution.

## Decision

1. `budget_tokens` is the **base** working set. `max_budget_tokens` is the
   ceiling; absent means the base, which disables the ladder.
2. The working set may **double within a session** (64k → 128k → 256k) when a
   real signal says the content does not fit:
   - pressure reaches Red (or the next call would cross the ceiling);
   - the per-call estimate exceeds the current budget.
   Those are the same signals that otherwise trigger a wrap-up instruction or
   tool-result elision.
3. The ceiling is `min(profile max, model window − output reserve)`. The
   ladder never widens beyond what the model can hold.
4. Escalation is **monotonic for the engine's lifetime** and does not decay.
   A stable budget keeps the history prefix (and its prompt cache) unchanged
   across turns. The ladder resets when the engine reopens.
5. Wrap-up and elision remain the fallback **at the cap**; they are not
   skipped, only postponed while the ladder can still move.
6. Every step emits `ContextEscalated { turn_id, from, to, reason }`. There is
   no user-facing switch; hosts may display the current working set.

## Consequences

- Normal turns stay at the base; only sessions with genuinely large content
  pay for a larger working set.
- The history assembled for the current turn is not rebuilt mid-turn: an
  escalation raises the ceiling for the remaining calls and for the next
  turn's assembly. That is deliberate — rebuilding mid-turn would change the
  prefix mid-conversation and cost a cache miss.
- A conversation that grew in one engine and is reopened later starts at the
  base and climbs again from its stored history.
- Compression (segments, summaries, recall, digests) still runs; a larger
  budget changes when it runs, not whether it exists.

## Alternatives rejected

- **Fixed value**: predictable but stale for large-window models.
- **Fraction of the window**: easy to derive, but one size for chat and
  large-codebase work, and it changes with every model swap.
- **Always the maximum**: pays for dilution and prefill on every turn.
- **Per-turn dynamic budget**: the assembly and prefix change constantly,
  which defeats prompt caching and makes behaviour hard to reason about.
