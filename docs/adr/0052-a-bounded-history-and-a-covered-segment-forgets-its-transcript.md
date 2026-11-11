# ADR 0052 — A bounded history entry, and a covered segment forgets its transcript

## Status

Accepted.

## Context

A real conversation reached 1.1 MB on disk. Measuring the document by field
showed the snapshot at 434k characters, of which 423k were the verbatim
`assistant` entries of one segment: eleven entries of about 38k characters,
mostly `[tools]` summaries. The earlier steps removed the per-turn event
stream and bounded the host's stored tool output, but the kernel's own
transcript of the turn was unbounded.

Two facts made the obvious fix insufficient. First, the largest document had
a single open segment: it never sealed, so no summary ever covered it and a
coverage-based rule would never fire. Second, recall reads an excerpt from a
segment's entries, so simply dropping entries loses the detail recall exists
to bring back.

## Decision

1. **One entry body is bounded.** `record_turn` keeps at most
   `MAX_ENTRY_CHARS` (12 000) characters of an assistant body, followed by
   `(+N characters kept out of this turn's transcript)`. The live turn still
   saw the whole text and the tool output; this only bounds the history copy.
2. **A covered segment forgets its transcript.** When a rolling summary
   covers a segment (`absorbed_in` set, segment id in `summary.covers`), its
   entries are dropped, except for the newest non-empty segment, which stays
   verbatim so the latest exchange is unchanged. The segment keeps its
   record, its memo, and the summary text.
3. **Recall keeps a fallback.** Before the entries go, the record stores a
   bounded `excerpt` (`user: <goal>` and `assistant: <result>`); recall uses
   the per-query excerpt when entries exist and the stored one otherwise.
4. **Restore applies both rules.** A document written before this change is
   capped and pruned on load, so it shrinks on its next save instead of
   staying large forever.

## Consequences

- A turn now costs at most about 12k characters of snapshot instead of an
  unbounded body. A conversation grows with the number of segments in
  flight, not with how much each turn wrote.
- The model's view of an older turn can be truncated with a note. The record,
  memo and summary carry what the turn was about; a detail that matters
  should be in a file or asked again.
- Recall for an absorbed segment is coarser: one stored exchange instead of
  the best-matching entries. The memo already carried most of this.
- The snapshot schema gains one optional field (`excerpt`), which older
  readers ignore and older documents omit.

## Alternatives rejected

- Splitting the host document into an index and per-turn blobs: the snapshot
  is one large field, so the blob would be just as large and the parse cost
  would stay.
- Lowering the segment turn/token caps: more summaries, more model calls, and
  the same unbounded entry within the cap.
- Dropping entries without an excerpt: recall tests failed, and the feature
  that brings old details back would have silently lost its detail.
- Storing images in the transcript: out of scope; images are turn-scoped
  (ADR 0051).
