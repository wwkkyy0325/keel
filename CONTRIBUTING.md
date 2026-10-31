# Contributing

Read `CLAUDE.md` and `docs/coding-rules.md` first.

## Workflow

1. Pick a milestone from `docs/spec/06-roadmap.md`.
2. Write or update the spec/ADR if the contract changes.
3. Write a failing test.
4. Implement the smallest change that passes.
5. Run the full verification:

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo deny check
```

6. Commit only when asked, with an English conventional commit and no AI
   attribution.

## Clean-room

Do not copy code, tests, docs, data or vocabulary from any other repository.
If a design input is not in `docs/spec/`, record it in
`docs/spec/05-provenance.md` before using it.
