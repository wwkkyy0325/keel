# Permission, sandbox and interaction refactor

Applies the decisions from the design discussion to the M1 kernel.

## Steps

- [x] 1. Permission core: two levels, deny list, dispatcher enforcement, profile simplification — verify: `cargo test --workspace`
- [x] 2. Soft delete: `TrashStore`, overwrite backup, trash protection — verify: trash/overwrite tests
- [x] 3. Interaction primitives: typed requests, `InteractionPort`, permission confirm and deferred report — verify: timeout/defer/security tests
- [x] 4. Plan mode as tool visibility — verify: plan mode visibility test
- [x] 5. ADRs and spec: permission levels, interactions, acceptance checklist contract — verify: docs review, no code change

## Result

- `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
- `cargo test --workspace` passes: 77 tests.
- `cargo run --release -p keel-runtime --example turn_overhead` over 1000
  turns: p50 2 us, p99 6 us (budget p50 < 5 ms, p99 < 20 ms).
- CLI: coding profile 17 events, companion profile 11 events, both `Done`.
- ADR 0002 (permission levels), ADR 0003 (interactions) and ADR 0004
  (acceptance checklist contract) are recorded.
- `cargo deny check` passes after installing `cargo-deny v0.20.2` and pinning
  the path workspace dependencies to `0.1.0`.

## Follow-up

- [x] Path-prefix grants, `FullAuto` escalation TTL and `revoke_all`.
- [x] Context budget re-checked before every model call; overflow ends in
      `ContextOverflow`.
- [x] `before_commit` accepts only `Commit`; other decisions return
      `Unsupported` instead of being ignored.
- [x] Structured assistant tool-call messages and tool-result call ids.
- [x] `ModelProvider::stream` receives the cancellation token.
- [x] Tests: 86 total; clippy clean; benchmark p50 2 us, p99 9 us over 1000
      turns.

## Decisions applied

- Two permission levels: `WorkspaceAuto` and `FullAuto`.
- Remove `read_only` and `manual`; read-only and plan mode are tool visibility.
- Permission enforcement moves to the dispatcher via `Tool::permission_requests`.
- Hard deny list is always enforced, including `FullAuto`.
- Soft delete only; trash failure never falls back to hard delete.
- Overwrite backs up the old file to trash before writing.
- Interactions are kernel primitives, not domain tools.
- Interaction timeout defaults to defer-and-continue for automation and block for interactive hosts.
- Acceptance checklist contract is documented now, implemented with M2 verifiers.

## M1 scope note

No `delete_file` tool and no verifier engine in this step. `TrashStore` is consumed by
overwrite backup; the delete tool and OS recycle-bin backend arrive with the
coding profile. The acceptance checklist is an ADR/spec contract, not M1 code.
