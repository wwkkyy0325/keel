# ADR 0037 — Summary cadence, covered index and segment memos

## Status

Accepted and implemented in `keel-core` and `keel-runtime`. Released as
0.2.0.

## Context

The rolling summary is rewritten at every segment seal. Measured on a real
transcript replay, that has three costs:

1. Every rewrite changes the history prefix, so a seal streak pays a cache
   miss per turn even when the context is small.
2. Each rewrite drops "details that no longer matter", so a topic the user
   returns to can vanish from the context after two seals.
3. The summary is the only compressed view of covered segments; the
   structured records stop being injected once absorbed.

Recall is lexical and only fires when the new input shares terms with an
absorbed segment, so it is not a complete answer.

## Decision

1. **Cadence.** The driver rewrites the summary at most every
   `KernelOptions::summary_every_seals` seals (default 3), or immediately at
   Amber pressure or worse. A deferred seal leaves its record uncovered, so
   the structured record carries it until the next rewrite. Hosts can tune
   the cadence with `TurnDriver::with_summary_cadence`.
2. **Covered index.** Covered segments are rendered next to the summary as
   one line each (`#id goal -> result [files]`), bounded to about 600 tokens.
   A topic the rewrite dropped stays discoverable and its paths stay exact.
3. **Segment memo.** The summary prompt asks for one leading line,
   `SEGMENT_MEMO: ...`, describing the newly sealed segment. The driver splits
   it off, stores it on the segment record (`memo`), and recall renders the
   memo instead of a raw excerpt. A model that ignores the instruction falls
   back to the previous behavior: no memo, the whole text is the summary.
4. `SegmentRecord::memo` is optional and defaults to absent, so snapshots
   written before 0.2.0 still load.

## Consequences

- A seal streak no longer breaks the cache every turn, and recent topics stay
  as records until the summary catches up.
- Old topics remain discoverable through the index even when the summary
  drops them; the memo gives recall a compact, decision-shaped result.
- The index adds up to ~600 estimated tokens to the summary fragment; it is
  included in `summary_tokens` in `history_assembled`.
- The memo only exists for segments that were sealed at a rewrite. Segments
  sealed between rewrites rely on their record and the index until the next
  rewrite absorbs them.
