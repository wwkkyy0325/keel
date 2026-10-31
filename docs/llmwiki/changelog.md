# Changelog

## 2026-11-02

- Started M1: workspace with `keel-core`, `keel-runtime` and `hosts/keel-cli`.
- Added the core contracts, turn FSM, mock model, tool dispatch, context
  assembly, minimal permissions, event log and the CLI mock turn.
- Profiles for the coding and companion variants are data; the companion path
  exercises the no-tools policy without kernel changes.
- Recorded M1 ambiguities in `docs/spec/open-questions.md`.

### Verification

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` pass (58 tests).
- Turn overhead with the mock model: p50 2 us, p99 7 us over 1000 turns.
- `keel-cli` runs one turn for both the coding and companion profiles.
- `cargo deny check` remains pending until `cargo-deny` is installed.

## 2026-11-02 — Permission and interaction refactor

- Reduced permission levels to `WorkspaceAuto` and `FullAuto`; removed
  `read_only` and `manual`; internalized the sandbox and plan mode.
- Moved permission enforcement into the dispatcher via
  `Tool::permission_requests`; added the hard deny baseline.
- Added soft-delete `TrashStore` and overwrite backup; no hard-delete fallback.
- Added typed interactions (`ModelChunk::Interaction`), the `InteractionPort`,
  `InteractionBroker`, `WaitInteraction`, permission confirmation, escalation
  and `TurnReport` deferred items.
- Added plan-mode tool visibility and the two-level profile schema.
- Recorded ADR 0002, ADR 0003 and ADR 0004.
- Verification: 77 tests; clippy clean; mock overhead p50 2 us, p99 6 us.

### Follow-up

- Added path-prefix permission grants, session escalation TTL and `revoke_all`.
- Re-check the full message budget before every model call; overflow becomes a
  typed error until compaction lands.
- `before_commit` now rejects `Reprompt` and `Ask` explicitly instead of
  ignoring them.
- Verification: 84 tests; clippy clean; mock overhead p50 3 us, p99 7 us.

### M2 prerequisites

- Added structured assistant tool-call messages and tool-result call ids, and
  included tool-call payloads in the context budget estimate.
- `ModelProvider::stream` now receives the cancellation token.
- Verification: 86 tests; clippy clean; mock overhead p50 2 us, p99 9 us.
- Installed `cargo-deny v0.20.2`; `cargo deny check` passes. Pinned path
  workspace dependencies to `0.1.0` to satisfy the wildcard ban.

## 2026-11-02 — M2 OpenAI-compatible provider

- Added `keel-model-openai` with typed request/message/tool mapping, streamed
  tool-call accumulation and the reserved `keel_interact` control tool.
- Added a streaming HTTP provider with SSE parsing, usage, error mapping,
  stream-cut detection and cancellation; tested against a local HTTP server.
- Host provider selection is still pending.

### Secret handling

- Added best-effort secret detection and redaction in `keel-core`.
- The CLI refuses secret-like input before the model call and directs the user
  to `.env.example`; the event log redacts detected secrets.
- Added `.env.example`; `.env` stays gitignored.

### Host provider selection

- Profiles can select `mock` or `openai-compatible` with `base_url` and an
  `api_key_env` name.
- The CLI loads `.env` and reads the key from the environment; missing keys
  point to `.env.example`.
- Added `variants/coding/profile.openai.toml`.

### Secret input policy

- Replaced the hard refusal of secret-like input with a warning. Test keys,
  hashes and config examples trigger the same patterns, so blocking them made
  the CLI unusable.
- The event log still redacts detected secrets before append; `.env` remains
  the supported credential path.
- Verification: CLI test covers a provider-style key, ordinary text and a
  SHA-256 hash; workspace tests, clippy and `cargo deny check` pass.

### Secret input notice

- Moved the warning out of a pre-turn banner. The driver emits a
  `SecretWarning` event on the turn and returns the detected kind in
  `TurnOutcome`; the CLI prints one notice with that turn's result.
- The event log still redacts the input and final text.
- Verification: runtime test checks the event and outcome and confirms the
  turn still completes; CLI smoke test shows the notice after the turn output.

### Acceptance verifier

- Added `AcceptanceItem`, `VerifySpec`, `Evidence` and `VerificationReport` to
  `keel-core`; the model cannot construct passing evidence.
- Added `AcceptanceVerifier` with real compute and file-hash checks. Compute
  uses a bounded arithmetic parser; file hashes use SHA-256 over a
  workspace-relative path.
- Command, test and human items return `Unverified` until the structured shell
  and confirmation paths land.
- Recorded deferred context, profile and pipeline-registry work in
  `docs/llmwiki/backlog.md` with triggers.
- Verification: 112 tests; clippy, fmt and `cargo deny check` pass.

### Read-only coding tools

- Added `list_dir` and `code_search`; both are read-only, request read
  permission, do not follow symlinks, and bound output.
- `code_search` skips `.git`, `.keel`, `target`, `node_modules`, `dist`,
  `build`, virtualenv and cache directories; it skips files over 1 MiB and
  non-UTF-8 content.
- Coding profiles expose `echo`, `read_file`, `write_file`, `list_dir` and
  `code_search`.
- Verification: 114 tests; clippy, fmt, `cargo deny check` and a CLI mock turn
  pass.
