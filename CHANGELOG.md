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
- Versioned session snapshots (`SNAPSHOT_VERSION = 1`); a newer format is
  refused rather than misread.

### Reference host

- SQLite store: resume, per-turn checkpoints, branches, deletion, projection,
  fact/task history and context snapshots.
- Composed frontend views: sessions, session, turn and memory, with turn
  paging and a golden file.
- MCP plugins over stdio and streamable HTTP, schema mapping, roots/ping
  replies, process-tree cleanup and `--plugins-check [--json]`.
- Cache and usage telemetry; cost model in `docs/llmwiki/cache-economics.md`.

### Evidence

- 50-chapter novel: 57/57 turns, 0 overflow, 78.9% cache hit.
- 20 curated coding cases: 20/20.
- Five companion personas, 20 turns each: 100/100.
- Kernel contract tests and 296 Rust tests (252 in the kernel crates).
