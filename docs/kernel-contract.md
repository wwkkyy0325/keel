# Kernel contract

This repository is kernel-first. It contains the reusable kernel crates and one
reference host that exercises them. A derived project depends on the kernel and
brings its own host, UI and composition; it does not fork the kernel.

I cannot compare this layout with another project's internals (clean-room
rules), but the shape is deliberate: a small library core, a thin executable
that proves it, and products built on top.

## What is kernel and what is not

| Part | Role |
|---|---|
| `keel-core` | Stable data, ports and traits: messages, tools, model wire types, policy questions, events, errors, sessions and snapshots |
| `keel-policy` | Profile schema and the immutable `RuntimeConfig` |
| `keel-runtime` | Turn FSM, driver, dispatcher, context stages and governor, permissions, acceptance, event log, tools, summarizer |
| `keel-model-openai` | OpenAI-compatible wire mapping and streaming provider |
| `keel-model-anthropic` | Anthropic Messages wire mapping and streaming provider |
| `hosts/keel-cli` | **Reference host**, not kernel: composition, SQLite store, MCP adapter, frontend views, CLI |

The CLI is the executable contract test: it runs the kernel the way a product
does. A derived project may copy patterns from it or replace it entirely.

## Stability tiers

Stable within a major version:

- `keel-core` public types and traits: `Session`, `SessionSnapshot` (format
  versioned), `Tool`, `ToolSchema`, `ToolArgs`, `ToolResult`, `ModelProvider`,
  `ContextStage`, `Policy`, `PermissionGate`, `InteractionPort`, `Event`,
  `EventSink`, `KernelError`, `ErrorCode`.
- `keel-runtime` permission surface: `PermissionEngine`, `RootScope`,
  `PermissionEngine::with_root(path, access)` and
  `PermissionEngine::add_root(path, access)` (also
  `TurnDriver::add_root`), which returns the canonical scope for a host
  command such as `/add-dir`. A root added at runtime is a session scope: it
  survives `revoke_all` (which drops grants) and lives until the process
  exits. Extra roots are read-only or
  read-write; execution is always confirm-only; the most specific root wins;
  drive roots and system directories are refused. `PermissionEngine::with_outputs`
  names the readable spill directory and `TurnDriver::with_outputs_dir` writes
  spills there; both default to `<workspace>/.keel/outputs`. The directory
  keeps the newest 200 `turn-*-call-*.txt` files and prunes older ones; other
  files are untouched.
- Two deny classes. The structural list (`.keel` and operating-system areas)
  is an unconditional denial that no grant or escalation lifts. A credential
  name is refused outside every root and needs a confirmation inside a root;
  a session path grant opens it, a full-auto escalation does not. `.env`
  templates (`.example`, `.sample`, `.template`, `.dist`, `.defaults`) are
  ordinary files. `shell` labels a program outside its standard list in the
  permission request but does not veto it; the confirmation is the decision
  point.
- The event enum is the live UI/audit feed: turn and state changes, context
  builds, tool starts/finishes, `PermissionDecided` for every gate decision,
  interactions, memory and summary events, `SubAgentFinished` for a delegated
  run, and `BackgroundStarted`/`BackgroundResult` for a background command.
  `Event` serializes with snake_case
  keys so a host can persist the stream without a second schema. A host may
  call `EventLog::clear` between turns after persisting the stream; sequence
  numbers stay monotonic within the process. `ToolFinished` carries a bounded,
  redacted `output_preview` and `ToolStarted` a bounded, redacted
  `args_preview` for a live UI; both are skipped during serialization, so tool
  arguments and output never reach the persisted stream.
- Turn limits are runaway protection, not a task budget: 500 model calls, the
  profile's `tools.budget` and a 24-hour backstop. The timeouts are
  *inactivity* windows: a command with five minutes of silence is stopped, a
  command that keeps producing output is not, and a model stream is stopped
  after five minutes without a chunk. A host may lower them. A per-turn
  input-token budget (`max_turn_input_tokens`, one hundred times the working
  set with a one-million floor) bounds cost; reaching it forces the answer
  call and records `TurnBudgetExhausted`. The number never enters a prompt, so
  the model cannot drift toward optimising a counter. While the task list has
  unfinished items, the driver continues the turn instead of ending it
  (`TurnContinued`), at most six times, and never after the wrap-up
  instruction or the reserved final call. Read-only calls in one round run in
  parallel, capped at eight at a time; mutating calls stay serialized.
- The tool protocol: names, parameter kinds, risk levels and result envelopes.
  `Tool::concurrent()` lets a mutating tool join the parallel batch; the
  declared risk stays for the UI and audit. `ToolResult::with_usage` attaches
  tokens spent outside the main model loop (a sub-agent), and the driver adds
  them to the turn usage.
  A tool declared `read_only` may only declare read access; the dispatcher
  rejects a write or execute request before the permission gate runs, so the
  concurrent read batch cannot hide a mutation.
- Path confinement: before the gate, the dispatcher canonicalizes every
  declared path (for a write target that does not exist yet, through its
  deepest existing ancestor), so a symlink inside the workspace cannot point
  the check outside it. `..` never climbs above the drive or filesystem root.
  A dangling symlink is refused. The exception is the configured outputs
  directory (default `<workspace>/.keel/outputs`), which is readable so a
  truncated tool result can be re-read; the rest of `.keel` is denied and the
  exception never allows a write. This is a permission boundary, not an OS
  sandbox: an approved command or MCP server is not confined.
- Built-in tool resource bounds: command output is captured as a bounded head
  and tail (64 KiB per stream), `read_file` reads at most 8 MiB and appends a
  truncation marker instead of refusing a larger file, and a timed-out or
  cancelled command kills its process tree. A host may wrap the
  tools but should not raise these silently.
- Command launch: on Windows a bare program name that is not found falls back
  to a `.cmd`/`.bat` shim on PATH, so allowlisted package managers start
  without a shell.
- Trash recovery: `TrashStore::list`/`find` (defaults report that a backend
  cannot list) and the `list_trash`/`restore_file` tools. `restore_file` takes
  the target path from the tool argument, so the permission gate approves the
  real file; an entry is accepted only for its recorded path, and the current
  version is backed up first. `TrashStore::discard` removes a file while
  keeping it recoverable; the default backs up then removes, and a local
  backend may move it instead. `LocalTrashStore::new(workspace)` keeps
  `<workspace>/.keel/trash`; `LocalTrashStore::at(dir)` uses `dir` directly so
  a host can put undo data in a state directory.
- Permission mode: a host may call `TurnDriver::escalate_full_auto(ttl)` and
  `TurnDriver::revoke_permission_grants()` to offer a mode switch. The grants
  are the same ones a prompt would create, expire with the TTL, and every tool
  call still passes through the gate.
- The profile schema loaded by `keel-policy`; unknown fields are errors.
- The snapshot JSON written by `Session::snapshot`; `format_version` is
  `SNAPSHOT_VERSION`, and `Session::restore` rejects a newer format.
- `keel-runtime` entry points: `TurnDriver`, `TurnDriverConfig`,
  `KernelOptions`, `ContextBuilder` and the standard stages, `ToolDispatcher`,
  `ToolRegistry`, `PermissionEngine`, `AcceptanceVerifier`, `EventLog`,
  `ProviderSummarizer`, `TrashStore`.
- Sub-agents: `SpawnAgentTool`, `SubAgentRunner`, `SubAgentSpec` and
  `SubAgentOutcome`. The tool takes `{ task, role? }` and returns one bounded
  report with state and usage. The host runner builds a child driver with a
  fresh context, shared permissions/interactions and the parent cancellation
  token; sibling spawns overlap (the tool declares `concurrent`) while a
  child's spawn tool is bound to a deeper runner that refuses, so nesting is
  rejected. `ToolResult::with_usage` carries the child's tokens and the driver
  adds them to the turn usage, so the budget sees the delegation.
  `SubAgentFinished` (parent turn, role, state, usage, error and up to 64
  `SubAgentStep` entries: tool, ok, duration) is appended to the parent event
  log by the host runner.
- Background commands: `BackgroundTasks`, `BackgroundResult`,
  `LocalBackgroundTasks` and the `shell` `background = "true"` argument. The
  host registry runs at most four at once, records each start with
  `BackgroundStarted`, kills the process tree on drop and at the two-hour cap,
  and caps a command at two hours. Results enter at the model-loop safe point
  as a `[background <id> finished]` message and are audited with
  `BackgroundResult`; a result that misses the last call waits for the next
  turn.
- Interjections: `InterjectionQueue`, `InterjectionPush` and
  `TurnDriver::interject`. The queue is bounded and deduplicated; delivery
  happens only at model-call safe points, and late items are reported in
  `TurnOutcome::late_interjections` instead of being dropped silently.
  `InterjectionQueued`, `InterjectionDelivered` and `InterjectionDropped`
  carry the turn and id.
- `keel-model-openai`: `OpenAiConfig`, `OpenAiProvider`. Wire helper functions
  are public for testing and may change in a minor.
- `keel-model-anthropic`: `AnthropicConfig`, `AnthropicProvider`,
  `StreamDecoder`. The wire decoder is public for testing and may change in a
  minor.

Not part of the kernel contract:

- `keel-cli` modules (`store`, `mcp`, `view`, argument parsing). They are a
  reference implementation; a derived host owns its storage, plugin config and
  read models.
- The CLI flags, SQLite schema and view JSON. They are versioned by the host,
  not by the kernel.

## Versioning and updates

- The workspace version is shared by all kernel crates; bump them together.
- Before 1.0, a minor may break with an ADR and a `CHANGELOG.md` entry. From
  1.0, a breaking change requires a major.
- Deprecated fields stay readable for one minor cycle.
- A snapshot is the update boundary: `format_version` lets a new kernel refuse
  old data it cannot read. A host that upgrades migrates snapshots; the kernel
  never guesses.
- The profile schema is part of the contract; changing a field is an ADR and a
  spec update.

## How a derived project consumes it

Path dependency in a monorepo:

```toml
[dependencies]
keel-core = { path = "../Keel/crates/keel-core" }
keel-policy = { path = "../Keel/crates/keel-policy" }
keel-runtime = { path = "../Keel/crates/keel-runtime" }
keel-model-openai = { path = "../Keel/crates/keel-model-openai" }
keel-model-anthropic = { path = "../Keel/crates/keel-model-anthropic" }
```

Git dependency on a tag, until crates are published:

```toml
keel-runtime = { git = "https://example.invalid/keel", tag = "v0.1.0" }
```

Updating a derived project:

1. Bump the kernel tag/path and build.
2. Run the kernel contract tests in the derived host.
3. Check `SNAPSHOT_VERSION`; if it changed, migrate stored sessions before
   restoring them.
4. Run the host's acceptance and eval cases.

The kernel-only build proves the boundary:

```bash
cargo build -p keel-core -p keel-policy -p keel-runtime -p keel-model-openai -p keel-model-anthropic
cargo test -p keel-core -p keel-policy -p keel-runtime -p keel-model-openai -p keel-model-anthropic
```

CI runs this as a separate job, so a host change cannot hide a kernel
dependency.

### Minimal embedding

`crates/keel-runtime/examples/minimal_turn.rs` is a host in one file: a
profile, two context stages, one tool, a mock model and a driver. Run it with

```bash
cargo run -p keel-runtime --example minimal_turn
```

For personal projects a path dependency is enough; publishing is optional.
A product copies this shape, replaces the mock with a provider and the prints
with its UI, and does not need the reference CLI.

## Contract tests

| Extension point | Contract test |
|---|---|
| `Tool` | `crates/keel-core/tests/tool_protocol.rs` |
| `ModelProvider` | `crates/keel-model-openai/tests/`, `crates/keel-model-anthropic/tests/`, `keel-runtime` mock tests |
| `Policy` and profile resolution | `crates/keel-runtime/tests/profile.rs`, `keel-policy` unit tests |
| `ContextStage` | `crates/keel-runtime/tests/context.rs`, `window_matrix.rs` |
| `EventSink` / `EventLog` | `crates/keel-runtime/tests/event_log.rs` |
| Event order for a tool turn | `a_tool_turn_emits_the_host_contract_events_in_order` |
| `InteractionPort` | `crates/keel-runtime/tests/interactions.rs` |
| `PermissionGate` | `crates/keel-runtime` permission unit tests; `permission_decisions_are_recorded_as_events` |
| Path confinement | `a_symlink_cannot_read_outside_the_workspace`, `a_symlink_cannot_write_outside_the_workspace`, `parent_components_do_not_pop_the_filesystem_root` |
| Extra roots | `an_extra_read_root_allows_reads_and_confirms_writes`, `an_extra_write_root_allows_reads_and_writes`, `the_most_specific_root_wins`, `a_filesystem_root_is_rejected`, `a_root_added_after_construction_survives_grant_revocation`, `adding_the_same_root_twice_keeps_the_first_scope` |
| Tool output spill | `reading_a_spilled_output_is_allowed`, `the_rest_of_keel_stays_denied` |
| `Summarizer` | `crates/keel-runtime` summarizer unit tests |
| `TrashStore` | `crates/keel-runtime` trash unit tests |
| Background commands | `a_background_command_reports_its_output_and_exit`, `the_background_limit_is_enforced`, `a_background_start_is_audited`, `dropping_the_registry_kills_the_process_tree`, `a_background_result_enters_before_the_next_model_call` |
| Sub-agents | `a_spawn_returns_the_child_report_and_usage`, `a_failed_child_is_a_tool_failure`, `a_tool_that_declares_concurrency_overlaps_its_siblings` |
| Turn continuation and budget | `a_turn_continues_while_the_task_list_has_unfinished_items`, `the_turn_budget_forces_the_final_call_without_telling_the_model` |
| Session snapshot | `a_snapshot_restores_the_derived_state`, `a_snapshot_from_a_newer_format_is_rejected` |
| Snapshot persistence (host) | `hosts/keel-cli/src/store.rs` tests — host contract, not kernel |

Add a contract test with every new trait or when a trait's semantics change.

## Frontend adaptability

The kernel is UI-agnostic. A UI gets:

- a typed event stream (`EventSink`): state changes, context builds, tool
  activity, permission decisions, interactions, memory and summary updates;
- `InteractionPort` for questions, plan approval and permission confirmation;
- `CancelToken` for cancel;
- `TurnOutcome.answer` and, when enabled, `TurnOutcome.context` — the exact
  message list the model saw;
- `Session::snapshot` for session state.

Token-level streaming is deliberately not an event. The kernel hot path must
not allocate or serialize per token. A host that wants live tokens wraps its
`ModelProvider` and forwards `TextDelta`/`ThinkingDelta` to its UI before
returning them to the driver. The reference CLI prints only the final answer,
which is why token streaming does not appear in its output.

Read models and views are host concerns: the kernel does not know SQL, tables
or screen shapes.

## What a derived project supplies

- a profile (or a policy implementation) and prompts;
- tools or MCP plugins;
- a store adapter and host loop;
- its UI and read models;
- model provider configuration.

It does not need to change the kernel for a new variant. If it does, the
change belongs in the kernel behind a trait, with a contract test and an ADR.
