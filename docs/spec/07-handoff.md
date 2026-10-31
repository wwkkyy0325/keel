# 07 — Handoff to the implementation session

This repository is implemented in a **fresh Claude session rooted at
`D:\workspace-new\wwkkyy0325\Keel`**. That session must not read any other
repository.

## First prompt

```text
Read CLAUDE.md and docs/spec/00-brief.md through docs/spec/07-handoff.md.
This is a clean-room project: do not read or reference any other repository.
Before coding, restate:
  1. the kernel invariants,
  2. the kernel/middle/plugin boundary,
  3. the M1 file plan and acceptance tests.
Then implement M1 only: keel-core types, mock turn loop, event log,
cancellation, and the three tools. Do not add a UI, real model, channels,
memory backend or extra tools.
```

## Working protocol

1. One milestone at a time; do not jump ahead.
2. Write the failing test before the implementation.
3. Run `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`,
   `cargo test --workspace` and `cargo deny check`.
4. Commit only when asked, never with AI attribution.
5. If the spec is ambiguous, write the question in
   `docs/spec/open-questions.md`; do not invent employer-specific behavior.
6. If a change alters a contract, update the spec/ADR in the same commit.

## Review checkpoints

- End of M1: FSM, cancellation, tool protocol and event log reviewed.
- End of M3: companion profile runs with no kernel changes.
- End of M4: adding a third plugin needs no kernel edit.
- End of M6: eval report reproducible.

## Do not

- Read any repository outside this project root, including any `reference/`
  directory.
- Copy tests, prompts, docs or vocabulary from another repository.
- Add `if variant == ...` to the kernel.
- Let a plugin bypass permissions or audit.
- Commit secrets or generated artifacts.
