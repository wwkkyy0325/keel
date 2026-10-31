# Coding rules

These rules apply to every crate, test, script and document in Keel.

## 1. Rust style

- Edition 2021; MSRV is declared in the workspace `Cargo.toml`.
- `cargo fmt --check` is mandatory.
- `cargo clippy --workspace -- -D warnings` is mandatory.
- `unsafe` is forbidden by default. If it is ever required, it needs a
  `// SAFETY:` comment, a test and an ADR.
- Prefer small modules with one reason to change. A file over ~500 lines is a
  signal to split, not a hard limit.
- Public items have doc comments. Private items get comments only when the
  reasoning is non-obvious.
- Comments explain why; the code explains what.

## 2. Error handling

- No `unwrap`/`expect` on user input, filesystem, network, model output or
  environment variables.
- Kernel errors are typed (`thiserror`), with a stable code, a retryable flag
  and a user-facing message.
- Wire boundaries use the shared error envelope. Do not invent per-module JSON
  error shapes.
- Cancellation is a first-class outcome, not an error string.
- Every retry has a bound and a backoff. No unbounded retry loops.

## 3. Async and concurrency

- No blocking I/O inside an async task. Use `spawn_blocking` or an async API.
- Every network/model/tool call has a timeout.
- Channels are bounded. Producers handle backpressure; no unbounded queues.
- Shared state is explicit: `Arc` for immutable data, locks only around small
  critical sections, never held across `.await`.
- Background work is cancellable and observed. Detached tasks need a reason.

## 4. Performance

- Boundaries are coarse: turn, tool call, context build, model stream chunk.
- Hot paths use concrete types and static dispatch. `dyn` is for plugin
  boundaries, not per-token work.
- Share messages with `Arc`; avoid cloning the full history per turn.
- Parse JSON once at the boundary; the core uses typed structs.
- Cache compiled tool schemas, system fragments and token counts.
- Audit/telemetry writes are asynchronous and bounded.
- Performance claims need a benchmark and a budget from
  `docs/spec/04-evals.md`.

## 5. Architecture

- `kernel` must not depend on `middle` or `plugins`.
- `middle` resolves a profile into an immutable `RuntimeConfig`.
- `plugins` depend only on the plugin API, never on kernel internals.
- No `if variant == ...` in the kernel. Variant behavior is data.
- Plugins cannot bypass permission, audit, cancellation or redaction.
- A new extension point needs two consumers before it is generalized.
- `crates/` holds libraries; `hosts/` holds binaries. A host may depend on a
  library; a library never depends on a host.
- Do not create empty crates or directories. Split a module into a crate only
  when a second consumer appears; otherwise keep it as a module.

## 6. Testing

- Unit tests for pure logic.
- Integration tests for the turn loop, tool dispatch and context assembly.
- Contract tests for every trait a plugin implements.
- Fault-injection tests: model 500, stream cut, tool timeout, disk full,
  cancellation, context overflow.
- Permission tests: traversal, denylist, admin, workspace boundary, audit.
- Benchmarks for kernel overhead, context assembly and tool dispatch.
- A bug fix starts with a failing test.

## 7. Security

- Permissions are enforced in the kernel, not in prompts or plugins.
- Paths are resolved and checked before use; no string concatenation.
- Shell execution uses structured arguments and allowlists. Natural language
  never becomes a shell string.
- Secrets come from `.env` or the OS keyring. Never log, commit or serialize
  them.
- Redaction defaults to metadata-only telemetry. Content capture is opt-in.
- Subprocesses get an explicit environment; do not inherit everything by
  accident.

## 8. Telemetry and audit

- Kernel emits typed events at fixed boundaries only.
- Audit is always on, append-only and separate from telemetry.
- Telemetry is sampled; errors are always kept.
- No high-cardinality labels in metrics.
- Plugins emit through the provided telemetry handle; they never write audit.
- Default is metadata-only. Prompt/tool content is opt-in and redacted.

## 9. Documentation

- ADRs for decisions that are expensive to reverse.
- Specs for behavioral contracts; code does not define the contract alone.
- A contract change updates its spec in the same commit.
- README describes the problem and how to run the project, not the file tree.

## 10. Git

- Conventional commits in English: `feat:`, `fix:`, `chore:`, `refactor:`,
  `docs:`, `test:`.
- Zero AI traces: no `Co-Authored-By`, no "Generated with Claude Code", no AI
  mentions, no tool signatures, no emoji.
- Commit only when the user asks.
- Each commit compiles and passes tests.
- Never commit secrets, `.env`, keys, tokens, `target/`, or generated artifacts.

## 11. Definition of done

A change is done when:

- the behavior is covered by a test;
- `fmt`, `clippy`, `test` and `deny` pass;
- performance budgets are unchanged or explained;
- the spec/ADR is updated if a contract changed;
- no new TODO without an issue;
- no secrets, paths or internal names leaked.

## 12. Anti-patterns

- A "temporary" variant branch in the kernel.
- A plugin that reaches into kernel state.
- A trait introduced for a single implementation.
- `serde_json::Value` in the core loop.
- Unbounded channels, retries or context growth.
- Logging full prompts by default.
- Copying code from another repository, even "just this once".
