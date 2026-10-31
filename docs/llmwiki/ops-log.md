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
- `feat: refuse and redact secrets`
- `feat: select model provider from profile`

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

### Secret handling

- Added core secret detection and redaction for common provider keys, tokens
  and private keys.
- The CLI refuses secret-like input before the model call with a rotation
  warning and points to `.env.example`.
- The event log redacts `TurnStarted` input and `TurnFinished` text before
  append.
- Added `.env.example`; `.env` remains gitignored.

### M2 — Host provider selection

- Added `base_url` and `api_key_env` to the profile model section and
  `RuntimeConfig`.
- CLI loads `.env`, selects mock or `openai-compatible`, and reads the key from
  the named environment variable.
- Added `variants/coding/profile.openai.toml`; the mock profile remains the
  network-free default.
- Verification: profile tests, CLI mock smoke test, workspace tests, clippy and
  `cargo deny check`.

### Secret input policy

- Changed secret-like input handling from refusal to warning. False positives
  on test keys, hashes and config examples made refusal unusable.
- The event log still redacts detected secrets; `.env` remains the credential
  path.
- Verification: CLI test covers a provider-style key, ordinary text and a
  SHA-256 hash; workspace tests, clippy and `cargo deny check`.
- Commit: `fix: warn on secret-like input instead of blocking`.

### Secret input notice

- Removed the pre-turn CLI banner. The driver emits `Event::SecretWarning` and
  returns `TurnOutcome.secret_warning`; the CLI prints one notice with the
  turn result.
- The event log still redacts the input and final text.
- Verification: runtime test checks the event and outcome and confirms the
  turn still completes; CLI smoke test shows the notice after the turn output.
- Commit: `refactor: attach secret notice to the turn`.

### M2 — Acceptance verifier (compute and file hash)

- Added the acceptance checklist types to `keel-core` and
  `AcceptanceVerifier` to `keel-runtime`.
- Compute verification uses a bounded arithmetic parser; file hashes use
  SHA-256 over a workspace-relative path and refuse traversal.
- Command, test and human items return `Unverified` until their execution path
  exists.
- Recorded deferred context/profile/pipeline work with triggers in
  `docs/llmwiki/backlog.md`.
- Verification: `cargo test --workspace` (112 tests), clippy, fmt and
  `cargo deny check`.
- Commit: `feat: add acceptance verifier for compute and file hashes`.

### M2 — Read-only coding tools

- Added `list_dir` and `code_search` with read permission requests, symlink
  skipping and bounded entries, file size, depth and output.
- `code_search` skips build/VCS/cache directories and non-UTF-8 or oversized
  files.
- Registered both in the CLI and exposed them in the coding profiles.
- Verification: 114 tests; clippy, fmt, `cargo deny check`; CLI mock turn
  reaches `Done`.
- Commit: `feat: add read-only workspace tools`.

### M2 — Structured shell and terminal approvals

- Added a shared command runner and `ShellTool`: fixed program/argument list,
  allowlist, no shell interpolation, bounded output and 60 s timeout.
- Permission prompts show the command line; sensitive environment names are
  removed from child processes.
- Added command and test acceptance verifiers using the same runner.
- Added a terminal interaction port; piped stdin defers instead of blocking.
- Coding profiles expose six tools.
- OS-level command sandboxing remains open and is recorded in `backlog.md`.
- Verification: 122 tests; clippy, fmt, `cargo deny check`; CLI mock turn
  reaches `Done`.
- Commit: `feat: add structured shell and terminal approvals`.

### M2 — First coding eval case

- Added `evals/cases/coding/task-001` with `task.md`, `setup/`, `hidden/`,
  `reference/`, `verify.sh` and `meta.toml`.
- The case fixes `mean([])`; the initial workspace fails one hidden test and
  the reference fix passes all four.
- Added `evals/README.md` and `.gitattributes` with LF for `*.sh` and `*.py`.
- Real model execution remains step 5 and needs a local `.env`.
- Commit: `test: add first coding eval case`.

### M2 — CLI workspace override

- Added `--workspace PATH` so eval runs use a temporary workspace while
  `.env` and the profile load from the repository.
- Updated README and eval instructions.
- Verification: 122 tests, clippy, fmt, `cargo deny check`; mock turn with
  `--workspace` reaches `Done`.
- Commit: `feat: add workspace override to the CLI`.

### DeepSeek profile

- Added `variants/coding/profile.deepseek.toml` with OpenAI-compatible
  `base_url = "https://api.deepseek.com"` and `model = "deepseek-flash"`.
- `.env` and `.env.example` use `DEEPSEEK_API_KEY`; only the key is blank.
- Eval `task-001` metadata targets the DeepSeek profile.
- Verification: profile test passes; the CLI reports `DEEPSEEK_API_KEY is
  empty` before any network call.
- Commit: `feat: add DeepSeek coding profile`.

### Prompt conventions

- Corrected the earlier interpretation: the requirement concerns prompt
  structure, not cross-turn context.
- Prohibitions come first; recommendations follow. Instructions are English;
  entity names and user-provided text keep their original language and
  spelling.
- Updated both prompts and documented the convention in `02-profile.md` and
  `backlog.md`.
- Verification: 123 tests; clippy, fmt and `cargo deny check`.
- Commit: `docs: correct prompt ordering convention`.

### Secret hygiene

- A key was pasted into tracked `.env.example`. Moved it to gitignored `.env`,
  restored the example placeholder, and advised rotation.
- No key is present in tracked files; `.env` remains ignored.

### Real-model turn limits

- Raised defaults to 12 model calls, 30 s tool timeout and 180 s turn timeout.
- Verification: 123 tests; clippy, fmt and `cargo deny check`.
- Commit: `fix: raise turn limits for real model runs`.

### First real coding run

- Added `reasoning_effort` to the profile/provider path; DeepSeek uses `none`
  because its thinking mode rejects tool loops without echoed
  `reasoning_content`.
- Ran `deepseek-flash` on `task-001` in a temporary workspace. The model fixed
  `calc.py`; `verify.sh` passed 4/4 hidden tests.
- Shell calls were denied because stdin was not a terminal. The model stated
  the fix was unverified by execution rather than fabricating a test run.
- Report: `evals/reports/2026-11-02-task-001-deepseek.md`.
- Verification: 124 tests; clippy, fmt and `cargo deny check`.
- Commit: `feat: pass reasoning effort to the provider`.

### M2 acceptance — usage and latency

- Added summed token usage and turn duration to `TurnOutcome`; the CLI prints
  `usage` and `duration_ms`.
- Second real `task-001` run: 7437 tokens, 5916 ms, hidden tests 4/4.
- Recorded M2 acceptance: interactive terminal self-run still pending; the
  `keel-policy` crate split is waived until a second consumer.
- Verification: 124 tests; clippy, fmt and `cargo deny check`.
- Commit: `feat: report token usage and turn duration`.

### Adaptive reasoning — ADR and reasoning content

- Recorded ADR 0005 with the corrected trigger set: no context-overflow or
  compaction trigger, default `Low`, no user on/off switch.
- Added `Message::reasoning_content`; the driver collects thinking deltas and
  attaches them to assistant tool-call messages. The wire echoes
  `reasoning_content` when present.
- Added `docs/llmwiki/tasks/adaptive-reasoning.md` for the controller,
  profile removal and verifier signals.
- Verification: 125 tests; clippy, fmt and `cargo deny check`.
- Commit: `feat: preserve reasoning content for tool calls`.

### Adaptive reasoning controller

- Added `ReasoningLevel` (`Low -> High -> Max`) to the driver: monotonic per
  turn, reset next turn, no user or model switch.
- Removed profile `reasoning_effort`; `supports_reasoning` is a provider
  capability and controls whether the wire sends the level.
- Escalation only on invalid tool arguments, repeated invalid calls, no-progress
  rounds and required verifier failures. Permission, not-found, timeout,
  cancellation, I/O and context overflow do not escalate.
- Assistant text and tool calls now share one message; `reasoning_content` is
  echoed even when empty for thinking-mode providers.
- Real task-001 stayed Low and passed 4/4: 15361 tokens, 10702 ms.
- Verification: 129 tests; clippy, fmt and `cargo deny check`.
- Commit: `feat: add adaptive reasoning controller`.

### Acceptance verification and policy split

- Driver runs host acceptance items at `Verify`; required failures escalate
  reasoning, feed evidence back and reprompt up to the configured bound, then
  end in `verification_failed`.
- Added `VerificationFinished` events and `TurnOutcome.verification`.
- CLI `--acceptance FILE` loads a TOML checklist; `task-001` uses a real
  `python -c` command check against the workspace.
- Created `crates/keel-policy` for profile parsing, `RuntimeConfig` and
  `TurnLimits`; runtime validation stays in `keel-runtime`, which re-exports
  the policy API.
- Real task-001: kernel acceptance passed; hidden tests 4/4.
- Verification: 131 tests; clippy, fmt and `cargo deny check`.
- Commit: `feat: wire acceptance verification and split keel-policy`.

### Hardening harness and cases

- Added readiness/risk docs and a hardening task doc.
- Added the eval harness (`run_case.py`, `run_suite.py`), provenance entries for
  planned public datasets, and ten self-authored cases.
- Added `profile.eval.toml` without shell; kernel acceptance still executes.
- Fixed the `Done`-after-failed-verification budget path.
- Suite: 10/10 passed, 152,807 tokens, 103 s.
- Verification: 132 tests; clippy, fmt and `cargo deny check`.
- Commit: `feat: add hardening harness and eval cases`.

### Provider resilience

- Added retry for transient HTTP/connect/timeout failures before streaming,
  with cancellation-aware backoff. CLI uses two retries; tests use zero.
- Bounded each model call to 120 s; the turn deadline remains 180 s.
- Added a local-server test: two 500 responses then a healthy stream.
- Verification: 133 tests; clippy, fmt and `cargo deny check`; real DeepSeek
  smoke returned `pong`.
- Commit: `fix: retry transient provider failures and bound model calls`.
