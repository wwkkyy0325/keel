# Changelog

Kernel crates and the reference host share the workspace version. A derived
project pins a version and updates deliberately; see `docs/kernel-contract.md`.

## 0.1.0 — 2026-11-01

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
- Bounded command execution: both pipes are streamed into a head and tail
  (64 KiB per stream) instead of being buffered in full; a timeout or
  cancellation kills the process tree; `read_file` refuses files over 8 MiB
  before reading them.
- Windows command execution resolves `.cmd`/`.bat` shims from PATH after a
  bare name is not found, so `npm`, `npx`, `pnpm` and `yarn` start without a
  shell.
- Trash recovery: each backup records its original path, `TrashStore` gains
  `list`/`find` (with defaults for existing backends), and the `list_trash`
  and `restore_file` tools restore a version through the permission gate. A
  restore backs up the current version first.
- `EventLog::clear` lets a host drop records it has persisted; sequence
  numbers stay monotonic within the process.
- `ToolFinished` carries a bounded, redacted `output_preview` for a live UI.
  Serde skips the field, so tool output never enters the persisted event
  stream.
- A tool declared `read_only` may only declare read access. The dispatcher
  rejects a write or execute request before the permission gate, so a
  mislabelled tool cannot hide inside the concurrent read batch.
- Path confinement before the gate: the dispatcher canonicalizes every
  requested path, including a not-yet-existing write target through its
  deepest existing ancestor, so a symlink inside the workspace cannot point a
  read or write outside it. `..` no longer climbs above the drive or
  filesystem root; a dangling symlink is refused.
- The outputs directory (default `.keel/outputs`) is readable so a truncated
  tool result can be re-read; the rest of `.keel` stays denied and the
  exception never permits a write.
- Extra roots: `PermissionEngine::with_root(path, access)` adds a read-only or
  read-write directory outside the workspace. The most specific root wins,
  execution stays confirm-only, and a drive root or system directory is
  refused. `RootScope` is part of the permission surface.
- New built-in tools: `make_dir`, `move_file` and `delete_file`.
  `delete_file` uses the new `TrashStore::discard`; the local backend moves the
  file into the trash instead of copying it and falls back to copy across
  volumes. `move_file` backs up an existing destination and stays on one
  volume. The coding profiles expose all three.

### Reference host

- SQLite store: resume, per-turn checkpoints, branches, deletion, projection,
  fact/task history, context snapshots, the kernel event stream and
  `turns.error`.
- Composed frontend views: sessions, session, turn, memory and events, with
  turn paging and a golden file.
- MCP plugins over stdio and streamable HTTP, schema mapping, roots/ping
  replies, process-tree cleanup and `--plugins-check [--json]`.
- Cache and usage telemetry; cost model in `docs/llmwiki/cache-economics.md`.
- `--tool-timeout-ms` overrides the per-tool timeout for long builds; the shell
  tool uses the same value the dispatcher enforces.
- Coding profiles expose `list_trash` and `restore_file`; the evaluation and
  novel profiles keep their previous tool list.
- Session processes clear the in-memory event log after each turn is printed
  and saved, so a long `--session` run does not grow with its history.
- Checkpoints default to the 20 most recent per session; `0` keeps all, and a
  fork point is never pruned.
- MCP messages are capped at 8 MiB (stdio line and HTTP body), and the HTTP
  body read is under the request timeout.
- `--tui` runs an interactive terminal host: streaming text, tool progress
  lines, an inline permission prompt, slash commands, Ctrl-C cancellation and
  session persistence. A finished turn collapses to the answer and one line
  of aggregate fields; intermediate text and tool lines are dropped. The
  renderer is tested with `TestBackend` goldens; the kernel has no UI
  dependency.
- `--demo` (mock provider only) paces a scripted turn: preamble, a `shell`
  permission prompt, a two-second command, then an answer streamed piece by
  piece, so the streaming and approval paths can be seen without a model key.
- Permission prompts list every option vertically; `↑`/`↓` selects and `Enter`
  confirms, with the default on Allow once. `Shift+Tab` switches the session
  between `workspace_auto` and `full_auto`, reflected in the bottom status
  line; the driver exposes `escalate_full_auto` and `revoke_permission_grants`
  for that host switch.
- Finished turns keep their process as hidden blocks: `Ctrl-O` shows or hides
  tool calls and intermediate text for the whole history. The transcript is
  no longer truncated; `PgUp`/`PgDn` scroll it and a scrollbar appears when it
  overflows.
- Tool lines keep the head and tail of their output while a turn runs and the
  whole bounded preview under `Ctrl-O`; the preview is redacted and never
  persisted.
- `examples/demo-workspace` is a failing-test workspace for the first-run
  walkthrough in the README.
- MCP `number` parameters no longer flatten to integers. The tool uses the
  single JSON `arguments` mode instead, so fractional values survive the
  kernel's integer-only parameter model.
- MCP servers start in the workspace directory and `roots/list` advertises
  every scope.
- `--root PATH[:ro|:rw]` adds a labeled scope; `/roots` lists them in the TUI.
  One writable root per phase, the rest read-only, is the intended use.
- `--bundle PATH` loads a composition file that names the profile, optional
  plugin file and optional state directory; its paths are relative to the
  bundle. `--bundle` is mutually exclusive with `--profile`/`--plugins`.
  Reference bundles live under `variants/coding` and `variants/companion`.
- `--state-dir PATH` (or a bundle's `state_dir`) moves the SQLite store to
  `<state>/keel.db`, the trash to `<state>/trash` and truncated tool output to
  `<state>/outputs`; `--store` still overrides the database.
  `LocalTrashStore::at` accepts the trash directory directly.
- Tool spill is configurable: `TurnDriver::with_outputs_dir` writes it,
  `PermissionEngine::with_outputs` allows reading it, and the truncation marker
  carries the absolute path. The default remains
  `<workspace>/.keel/outputs`. The outputs directory keeps the newest 200
  `turn-*-call-*.txt` files; older ones are pruned best-effort and unrelated
  files are untouched.
- `--job PATH` binds a bundle/profile, workspace, roots, state directory,
  session id and the first input into one reusable task. Every path resolves
  relative to the job file; explicit CLI flags override the job.
- `--tui --job` (or `--tui --input`) pre-fills the prompt with the job input;
  it is not submitted until the user presses Enter.
- The state root defaults to `KEEL_HOME`, then `~/.keel`
  (`%USERPROFILE%\.keel` on Windows). The store, trash and output spills live
  under it, so a plain run persists its session. `--state-dir` or a bundle's
  `state_dir` overrides it; `--store` overrides only the database. The eval
  harness isolates itself with a temporary `KEEL_HOME`.

### Performance

- Release-only gates for turn overhead, 10k-message context assembly, tool
  dispatch, memory recall and event enqueue, plus a 500-turn stress run.
- Event persistence adds about 0.37 ms at p50 to a turn with 50 events;
  serialization is 0.003 ms and enqueue is under a microsecond. The kernel emits
  no per-token events.
- Report: `evals/reports/2026-11-01-kernel-performance.md`.

### Examples

- `crates/keel-runtime/examples/minimal_turn.rs` runs one turn without the
  reference CLI: profile, two stages, one tool, a mock model and a driver.

### Evidence

- 50-chapter novel: 57/57 turns, 0 overflow, 78.9% cache hit.
- 20 curated coding cases: 20/20.
- Five companion personas, 20 turns each: 100/100.
- Kernel contract tests, including the full event order for a tool turn, and
  301 Rust tests (254 in the kernel crates).
