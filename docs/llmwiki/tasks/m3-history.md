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
- [ ] 7. Tool summaries: head and tail by character count, spill file under
      `.keel/outputs/`, marker carries the path — verify: a 200 kB output
      truncates, spills once, and is readable through `read_file`
- [ ] 8. Kernel segmentation heuristic — verify: transcript fixtures seal on
      topic change, verified completion, and size cap, and do not seal on a
      follow-up that references the same artifact
- [ ] 9. L2 turn digest and L3 segment record with the sufficiency gate —
      verify: a record missing a required field keeps raw entries and emits
      `CompressionSkipped`; a complete record replaces them
- [ ] 10. Facts: extraction from structured signals, session and global scope,
      strict promotion, contradiction handling — verify: a verified constraint
      is promoted; a transient path is not; a contradiction demotes
- [ ] 11. Window matrix: 8k, 16k, 32k, 64k, 128k and 1M with scripted growth,
      companion and coding, mock model — verify: success rate, overflow rate,
      compactions, wrap-ups and tokens per window are recorded, and the
      measured knee replaces the provisional floors in ADR 0006
- [ ] 12. Companion profile and 20-turn real run — verify: profile-only diff
      from M2, no tools, no kernel branch; transcript and metrics recorded

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
