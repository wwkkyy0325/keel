# M3 — History assembly and companion

ADR 0006 defines the design. This task turns it into steps that can be verified
one at a time. Summarization is blocked on
`docs/llmwiki/tasks/m3-summarization-research.md`.

## Steps

- [x] 1. Session and segment types — verify: unit test appends turns, opens a
      segment, seals it, and keeps entries ordered
- [x] 2. Contract change: `BuildCtx` carries the remaining budget and the
      history slice; a context stage may emit several messages — verify:
      builder test with a multi-message stage and the remaining-budget probe
- [x] 3. CLI session loop — verify: 20-turn mock session reaches `Done` each
      turn and keeps history across turns
- [x] 4. Budget-driven assembly: detail window sized in tokens (two-turn
      floor, no turn ceiling), digests for older turns — verify: a fixture
      session keeps the newest two turns verbatim, compresses the older four
      into one digest block, drops the oldest, and `HistoryAssembled` reports
      the composition. Sealed segment records arrive in step 9.
- [x] 5. Context governor: gauge over `min(model_window, budget_tokens)`,
      headroom measured in calls at the observed growth rate, level-dependent
      detail share, wrap-up at red — verify: every level is unit-tested; red
      injects the wrap-up exactly once and only when it fits; `ContextPressure`
      is emitted at turn start and before every call; a profile whose budget
      plus output reserve exceeds `context_window`, or whose window is below
      the floor, is rejected at load time. Tool-result clearing at amber lands
      with step 7; mid-turn compaction at critical lands with step 9, so an
      over-budget turn still ends in `ContextOverflow`.
- [x] 6. Turn-budget wrap-up: reserve one model call for the final answer and
      forbid new tool calls on it — verify: the last request carries zero tool
      schemas and one instruction message; a model that still requests a tool
      gets no dispatch and the turn ends in `Error` instead of a silent
      `Done`; `FinalCallForced` is emitted once
- [x] 7. Tool summaries: head and tail by character count, spill file under
      `.keel/outputs/`, marker carries the path — verify: a 200 kB output
      truncates to 2000 + 1000 characters, spills once, and is readable through
      `read_file`; arguments are clipped at 200 characters; args and output are
      redacted before recording or spilling
- [x] 8. Kernel segmentation heuristic — verify: fixtures seal on topic change,
      verified completion and the 12-turn cap, and keep the segment when a
      follow-up shares terms or names a recorded artifact. The driver emits
      `SegmentOpened` and `SegmentSealed` with the reason; history still spans
      sealed segments until records land in step 9.
- [x] 9. L2 turn digest and L3 segment record with the sufficiency gate —
      verify: a sealed segment with a goal and a result becomes a record that
      replaces its raw entries in history; a segment whose last turn has no
      assistant text keeps its raw entries and emits `CompressionSkipped` with
      the missing field. The gate currently requires goal and result; error
      tracking is not recorded per turn yet, so errors are not a gate field.
- [x] 10. Facts: a three-tier store (session, workspace, global) with
      conservative promotion and rollback. Verify: only sentence-initial
      imperative constraints count (questions, first-person narration,
      mid-sentence cues and task-scoped phrasing are rejected); an explicit
      request promotes to workspace, and only an explicit request that also
      says "everywhere / all projects / 全局" reaches global; without an
      explicit request the same statement must appear in at least two separate
      turns, and it stops at workspace; artifacts and verified outcomes stay in
      the session; a contradiction replaces the key across all tiers
      (demotion), and "forget X / 忘记 X" removes matching facts from every
      tier. A `MemoryStage` emits global, workspace and session facts as one
      bounded block. Limits: cue matching is heuristic (no model), and
      workspace/global facts live in-process until persistence lands in M5.
- [x] 11. Window matrix: 8k, 16k, 32k, 64k, 128k and 1M with scripted growth,
      companion and coding, mock model — verify: recorded in
      `docs/llmwiki/window-matrix.md`. It proves the machinery, not real token
      counts: the mock reports usage from the same estimator, so it cannot
      calibrate it. Real calibration came from step 12.
- [x] 12. Companion profile and 20-turn real run — verify: the DeepSeek
      companion profile differs from M2 only in data; 20/20 turns reached
      `Done`, 22.9k input tokens, no overflow, and the remembered fact was used
      across later turns. Report:
      `evals/reports/2026-11-03-companion-20-turn.md`. The run also found and
      fixed the CJK token estimate, provider overhead calibration, Chinese
      segmentation and the newest-segment detail window.
      The coding path was verified the same way: 20/20 `Done` with four tools
      at a 16k window, 292k tokens, generated tests passing 15/15 externally.
      Report: `evals/reports/2026-11-03-coding-20-turn.md`. It found that
      `write_file` could not create a missing parent directory; the tool now
      does.

## Conversation memo

- [x] Replace the per-segment model summary with one rolling session memo,
      rewritten at every seal from the previous memo, the new record and the
      newest transcript; scale the output budget with the working set and
      shrink it under pressure — verify: the second update sees the first
      summary, a short segment still updates, and the memo appears in the next
      request
- [x] Cap the summarizer input by the window, clip the transcript at both ends,
      trim the output at a sentence boundary, and share the prompt sections —
      verify: long messages keep head and tail; the budget has a floor and a
      ceiling
- [x] Absorb only records without open items, keep coverage cumulative, and
      leave the previous memo in place when the call fails — verify: pending
      records stay structured, absorbed records leave the projection, and a
      failed update leaves the record block visible

## Answer artifact (ADR 0007)

- [x] Turn answer as a separate artifact: body from the latest text-only
      round, kernel-observed changes, deferred items, verification, wrap-up
      notes, `missing` when empty — verify: narration is excluded, a written
      file appears in `changes`, and a text-less turn is marked missing

## L4 and storage design

- [x] Fold a record when every change is covered by an artifact fact and it has
      no pending items; keep it in storage and drop it from the context
      projection — verify: covered records fold, records with pending items or
      missing facts do not
- [x] Append unabsorbed records, including folded ones, to the next memo
      update, marked `[segment N]`; the new revision lists them in `covers` and
      each record gets `absorbed_in` — verify: the summarizer input contains the
      marker and the absorbed record leaves the catch-up set
- [x] Storage layout for the audit, transcript, derived-context and memory
      planes in ADR 0008 — verify: every in-memory field maps to a column;
      implementation deferred to M5

## Targeted recall

- [x] Recall absorbed segments by lexical overlap with the input; reuse the
      segment term signature and artifact paths, require a score threshold,
      return at most two, and attach a clipped, redacted excerpt from the best
      matching turn — verify: a distinctive early term recalls the segment,
      unrelated input recalls nothing, and a record still in the projection is
      not duplicated
- [x] Cap recall at a fifth of the remaining history allowance and 400 tokens;
      emit `HistoryRecalled` and print it from the CLI — verify: the block
      appears before the fallback records and the event reports segment ids and
      cost
- [x] Real 24-turn companion run with two probes after many topic switches —
      verify: 24/24 `Done`, 20 memo revisions with a max of 2679 characters,
      context 843–1479 tokens, recall fired for segments 0 and 4, and the
      answers were `config/marlin.toml` and `aglio e olio`. Report:
      `evals/reports/2026-11-03-companion-24-turn-recall.md`.

## Verification commands

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

## Notes

- Do not add a user-facing compaction command; compression is automatic.
- Do not let the model open or seal a segment.
- Do not replay reasoning content from history.
- Do not truncate the current turn's tool results in place.
- Keep each file under 900 lines; split history into `history/` submodules when
  it grows.
