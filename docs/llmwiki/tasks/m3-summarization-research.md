# M3 — Summarization research

Open item from ADR 0006. The structural design does not depend on the answer;
the `L2` and `L3` implementations do.

## Question

Who writes the compressed history, and with what mechanism, so that facts
survive compression, cost stays bounded, and tests stay deterministic?

## Candidate approaches

| Approach | Cost | Determinism | Quality | Failure mode |
|---|---|---|---|---|
| A. Structured extract, no model | zero | exact | good for outcomes, weak for prose | misses implicit constraints |
| B. Model summary per sealed segment | per seal | none | best on prose | drift, hallucinated facts |
| C. Incremental rolling summary | per turn | none | degrades over summaries | summary-of-summary loss |
| D. Hybrid: structured, model only when the sufficiency gate fails | rare | mostly | close to B on hard segments | gate calibration |
| E. Retrieval instead of compression | index cost | mostly | high recall | misses "always" style constraints |

## Risks to measure

- summary drift and hallucinated facts;
- lost negation ("do not use X" becoming "use X");
- summary-of-summary degradation;
- prompt injection from tool output surviving into a summary;
- non-determinism leaking into tests;
- cost per compaction and per 20-turn session.

## Proposed experiment

- 30 to 50 synthetic multi-turn transcripts with known facts, constraints,
  contradictions and a probe question per fact.
- Run A now, D behind a flag; B and C only if D fails the probe threshold.
- Metrics: token reduction ratio, fact recall on probes, contradiction rate,
  sufficiency-gate pass rate, cost per session, byte-identical repeat runs.
- Budget: one model, at most 200k tokens for the whole experiment.

## Decision rule

- If A keeps at least 90% of probe facts and costs nothing, ship A and keep the
  model path unbuilt.
- If A falls below that, build D and gate the model call on the sufficiency
  gate.
- B and C are rejected unless they beat D on both recall and cost.

## Outcome

Decided on 2026-11-03: model summaries are required, but they live at the
session level, not the segment level.

- `L2` turn digests stay structured and model-free; they are per-turn and would
  be too expensive to model-summarize.
- `L3` segment records keep only the kernel's structured fields.
- `L4` is one rolling session memo, rewritten at every seal from the previous
  memo, the new record, the newest transcript and any unabsorbed records. This
  replaces the per-segment model summary: two prose layers would double the
  calls and the drift.
- The memo budget is ten percent of the working set, floor 256 and ceiling
  4096 tokens; at orange or red the next update uses half. The call receives
  `max_output_tokens` and a token-capped input clipped at both ends.
- The prompt fixes the sections, forbids invented facts, requires paths,
  identifiers, commands and numbers to stay exact, keeps entity names in their
  original language, and makes contradictions replace the old statement.
- Records with open items are never absorbed. Failure keeps the previous memo
  and leaves the records visible; the next update catches up.
- Summary usage is folded into the turn's reported token usage.
- The probe experiment remains available if quality needs to be compared later.
