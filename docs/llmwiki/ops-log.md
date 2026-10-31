# Operations log

Chronological record of implementation steps, verification commands and
commits. Design decisions live in `docs/adr/`; this file is the working trail.

## 2026-11-02

### M1 kernel

- Created the workspace with `crates/keel-core`, `crates/keel-runtime` and
  `hosts/keel-cli`.
- Implemented the turn FSM, event log, mock model, tool dispatch, context
  assembly, policy module, minimal permission module and the CLI.
- Added `echo`, `read_file` and `write_file`.
- Added cancellation, timeouts and the 20+ test suite.

### Permission and interaction refactor

- Reduced permissions to `workspace_auto` and `full_auto` (ADR 0002).
- Moved permission enforcement into the dispatcher through
  `Tool::permission_requests`.
- Added the hard deny baseline, path-prefix grants, `full_auto` escalation
  with TTL and `revoke_all`.
- Added soft-delete `TrashStore`; `write_file` backs up overwritten files.
- Added typed interactions, `InteractionPort`, `InteractionBroker`,
  `WaitInteraction`, permission confirmation, escalation and `TurnReport`
  (ADR 0003).
- Internalized plan mode as tool visibility.
- Added the acceptance checklist contract (ADR 0004).
- Added structured tool-call messages and provider cancellation.
- Re-check the context budget before every model call.
- `before_commit` accepts only `Commit`; other decisions return `Unsupported`.

### Verification

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace` — 86 tests
- `cargo deny check` — advisories, bans, licenses and sources ok
- `cargo run --release -p keel-runtime --example turn_overhead` — p50 2 us,
  p99 9 us over 1000 turns
- `keel-cli` coding and companion profiles both reach `Done`

### Environment

- Installed `cargo-deny v0.20.2` with `cargo install cargo-deny --locked`.
- Pinned the path workspace dependencies to `version = "0.1.0"` so the
  `wildcards = "deny"` policy accepts them.
- `deny.toml` reports `license-not-encountered` warnings for allowances that
  are not currently used; no check fails.

### Commits

- `feat: implement M1 kernel with permissions and interactions`
- `feat: add OpenAI-compatible wire mapping`
- `feat: add streaming OpenAI-compatible provider`

### M2 — OpenAI-compatible wire mapping

- Added `crates/keel-model-openai` with request JSON building, typed message
  and tool-schema mapping, tool-call accumulation, typed tool arguments and the
  reserved `keel_interact` control tool.
- Tool-call ids are normalized to `call-{n}` across assistant calls and tool
  results, so history stays consistent regardless of the provider id.
- Model-initiated interactions are limited to clarify, choice and plan
  approval; permission approval and escalation remain kernel-initiated.
- Verification: `cargo test -p keel-model-openai` (7 tests), workspace clippy,
  `cargo deny check`.

### M2 — Streaming OpenAI-compatible provider

- Added `OpenAiProvider` with a reqwest client, SSE parsing, text/reasoning
  deltas, accumulated tool calls, control-tool interactions, usage collection,
  HTTP error mapping, stream-cut detection and cancellation.
- Added local TCP HTTP server tests for text, tool calls, interactions, HTTP
  500, stream cut and cancellation.
- Verification: 13 tests in `keel-model-openai`, workspace clippy and
  `cargo deny check`.
