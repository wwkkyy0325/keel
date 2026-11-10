# ADR 0038 — A segment seals on text size

## Status

Accepted and implemented in `keel-core`. Released as 0.2.1.

## Context

A segment sealed on a verified turn, a topic change, or twelve turns. A long
run can stay on one topic for ten or more turns; the replay's active segment
did, and its text and tool summaries stayed in the verbatim/digest portion of
the history while it grew. The summary only covers sealed segments, so the
active segment is the part attention dilution comes from. The turn cap of
twelve is too coarse when answers and tool summaries are large.

## Decision

1. Add `SEGMENT_TOKEN_CAP` (12,000 estimated text tokens). `seal_reason`
   returns `size` when the open segment's entries reach it, checked beside the
   turn cap and before the follow-up marker, so "继续" cannot hold a large
   segment open.
2. The estimate uses the same `Message::estimated_tokens` the context pipeline
   uses; tool summary blocks in assistant entries count.
3. The reason stays the existing `size` string, so event consumers and tests
   need no new variant.

## Consequences

- Long same-topic conversations seal earlier; the sealed text becomes a
  structured record and, at the next rewrite, part of the rolling summary.
- A segment that seals without a complete goal/result fails the compression
  gate; its text remains in history, so the cap cannot lose content.
- More segments mean more records and more rewrite candidates; the summary
  cadence (every third seal or Amber pressure) keeps the rewrite count bounded.
- This bounds the *history* contribution. A single turn with many tool calls
  can still raise the per-call context; tool-result elision is a separate
  mechanism and was not changed.
