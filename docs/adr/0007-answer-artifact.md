# ADR 0007 — The turn answer is a separate artifact

## Status

Accepted. Version 1: kernel projection, no model-driven editor.

## Context

`TurnOutcome.text` is every assistant text delta concatenated over the turn.
Real eval transcripts show the result: narration such as "I'll inspect the
workspace first." runs into the final summary, and a turn that produced no
text still ends in `Done` with an empty string.

The user-visible result of a turn is not the transcript. It is a smaller,
structured artifact: what was done, what changed, what was verified, and what
is still open. Treating it as "the last message" is also wrong: the last
message may be narration, and earlier results would be lost.

## Decision

1. The turn produces an `Answer`, separate from the transcript in `text`:

   ```
   Answer {
     revision, body, changes, pending, verification, notes, missing
   }
   ```

2. `body` is the latest assistant round that carried no tool call and no
   interaction. A later round replaces it; text is never concatenated. Text
   sent together with a tool call is narration and never enters the body.
3. `changes` accumulates artifacts the kernel observed the turn write. The
   kernel uses its own tool-call record, not the model's claim, and keeps one
   entry per path.
4. `pending` accumulates unresolved items. Deferred interactions are the first
   source; failed required checks join when the segment record lands.
5. `verification` is the kernel verifier's latest report.
6. `notes` is the raw wrap-up report, kept without parsing.
7. If the body is empty when the turn ends, it falls back to `notes`; if it is
   still empty, `missing` is set. A host must not present an empty body as a
   result.
8. `text` remains the transcript and audit trail. Hosts print the answer; the
   CLI does so as of this ADR.
9. There is no model-driven editor in version 1. A reserved control tool for
   incremental edits is deferred until an eval shows the last text round losing
   an earlier conclusion.

## Consequences

- The CLI prints `answer:` and a `changes:` line instead of raw accumulated
  text.
- The segment record in a later step can use the answer as its seed instead of
  re-deriving goal, result, changes and pending items.
- `missing` makes an empty turn observable instead of silently `Done`.
- The answer is per turn. Persisting it across turns belongs to the segment
  work; this ADR deliberately does not add it yet.
- Tests can assert the answer independently of the transcript, including that
  narration is excluded.

## Alternatives rejected

- Keeping the concatenated transcript as the result: mixes narration and the
  answer, and loses nothing only by accident.
- Last-message-only: a narration round can be last, and earlier conclusions
  would be dropped.
- A control tool from day one: more protocol surface and more model calls for
  a problem the projection already solves.
- Parsing tags out of assistant text: brittle and injectable.
