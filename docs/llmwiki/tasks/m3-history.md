# M3 — History assembly and companion

ADR 0006 defines the design. This task turns it into steps that can be verified
one at a time. Summarization is blocked on
`docs/llmwiki/tasks/m3-summarization-research.md`.

## Steps

- [ ] 1. Session and segment types — verify: unit test appends turns, opens a
      segment, seals it, and keeps entries ordered
- [ ] 2. Contract change: `BuildCtx` carries the remaining budget and a session
      view; a context stage may emit several messages — verify: builder test
      with a multi-message stage and an unchanged single-message stage
- [ ] 3. CLI session loop — verify: 20-turn mock session reaches `Done` each
      turn and keeps history across turns
- [ ] 4. Budget-driven assembly: detail window sized in tokens (70% share,
      two-turn floor, no turn ceiling), digests for older turns, records for
      sealed segments — verify: fixture sessions at small, medium and large
      budgets produce the expected composition, a short-turn chat keeps many
      turns verbatim, a tool-heavy turn is digested sooner, and
      `HistoryAssembled` reports the composition
- [ ] 5. Tool summaries: head and tail by character count, spill file under
      `.keel/outputs/`, marker carries the path — verify: a 200 kB output
      truncates, spills once, and is readable through `read_file`
- [ ] 6. Kernel segmentation heuristic — verify: transcript fixtures seal on
      topic change, verified completion, and size cap, and do not seal on a
      follow-up that references the same artifact
- [ ] 7. L2 turn digest and L3 segment record with the sufficiency gate —
      verify: a record missing a required field keeps raw entries and emits
      `CompressionSkipped`; a complete record replaces them
- [ ] 8. Facts: extraction from structured signals, session and global scope,
      strict promotion, contradiction handling — verify: a verified constraint
      is promoted; a transient path is not; a contradiction demotes
- [ ] 9. Companion profile and 20-turn real run — verify: profile-only diff
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
