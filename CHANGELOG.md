# Changelog

Kernel crates and the reference host share the workspace version. A derived
project pins a version and updates deliberately; see `docs/kernel-contract.md`.

## 0.1.0 — 2026-11-04

First complete kernel and reference host.

### Kernel

- Turn FSM, driver, tool dispatcher, cancellation and turn deadlines.
- Typed tool protocol and model wire types; OpenAI-compatible streaming
  provider.
- Profile resolution into an immutable `RuntimeConfig`.
- Bounded context: governor with watermarks and tool headroom, segment
  records, rolling memo, scoped facts, lexical recall and tool elision.
- Acceptance verifier (compute, command, test, file hash, human, unverifiable)
  with reprompts.
- Permission gate, trash port, interaction port, event sink and summarizer
  port.
- `PermissionDecided` events record every gate decision (`allow`, `deny`,
  `confirm`) with tool, access, path and reason, so a UI or audit log does not
  have to infer it.
- `Event` implements `Serialize` with snake_case keys, so a host can persist
  the kernel event stream as structured data.
- Versioned session snapshots (`SNAPSHOT_VERSION = 1`); a newer format is
  refused rather than misread.

### Reference host

- SQLite store: resume, per-turn checkpoints, branches, deletion, projection,
  fact/task history, context snapshots, the kernel event stream and
  `turns.error`.
- Composed frontend views: sessions, session, turn, memory and events, with
  turn paging and a golden file.
- MCP plugins over stdio and streamable HTTP, schema mapping, roots/ping
  replies, process-tree cleanup and `--plugins-check [--json]`.
- Cache and usage telemetry; cost model in `docs/llmwiki/cache-economics.md`.

### Performance

- Release-only gates for turn overhead, 10k-message context assembly, tool
  dispatch, memory recall and event enqueue, plus a 500-turn stress run.
- Event persistence adds about 0.37 ms at p50 to a turn with 50 events;
  serialization is 0.003 ms and enqueue is under a microsecond. The kernel emits
  no per-token events.
- Report: `evals/reports/2026-11-04-kernel-performance.md`.

### Examples

- `crates/keel-runtime/examples/minimal_turn.rs` runs one turn without the
  reference CLI: profile, two stages, one tool, a mock model and a driver.

### Evidence

- 50-chapter novel: 57/57 turns, 0 overflow, 78.9% cache hit.
- 20 curated coding cases: 20/20.
- Five companion personas, 20 turns each: 100/100.
- Kernel contract tests, including the full event order for a tool turn, and
  301 Rust tests (254 in the kernel crates).
