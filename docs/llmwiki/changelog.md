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

### Structured shell and terminal approvals

- Added `ShellTool` and a shared command runner: program plus argument list,
  allowlist, no shell or interpolation, bounded output and timeout.
- The shell confirmation prompt includes the command line; child processes
  drop environment names that look like keys, tokens, secrets, passwords or
  credentials.
- Added command and test verifiers on the same execution path.
- Added a terminal interaction port; non-terminal stdin defers, so headless
  runs deny permission-gated actions.
- Coding profiles now expose six tools; recorded the missing OS command sandbox
  in `backlog.md`.
- Verification: 122 tests; clippy, fmt, `cargo deny check` and a CLI mock turn
  pass.

### First coding eval case

- Added `evals/cases/coding/task-001`: fix `mean([])` to return `0.0`, with
  `setup/`, hidden unittest coverage, a reference fix, `verify.sh` and
  `meta.toml`.
- The case rejects the initial workspace (`exit 1`, one test error) and accepts
  the reference fix (`exit 0`, four tests pass).
- Added `evals/README.md` and `.gitattributes` to keep shell and Python files
  at LF.
- The real model run is still pending a local `.env`.

### CLI workspace override

- Added `--workspace PATH` so an eval can run against a temporary workspace
  while `.env` and the profile still load from the repository.
- Updated README and the eval instructions with the runnable command.
- Verification: 122 tests, clippy, fmt, `cargo deny check` and a CLI mock turn
  with `--workspace`.

### DeepSeek profile

- Added `variants/coding/profile.deepseek.toml` using the OpenAI-compatible
  endpoint `https://api.deepseek.com` and model `deepseek-flash`.
- `.env.example` and the local `.env` use `DEEPSEEK_API_KEY`; only the key is
  left blank. The profile carries the endpoint and model.
- The task-001 eval metadata now targets the DeepSeek profile.
- Verification: profile test loads and validates the new profile; the CLI
  reports an empty key clearly.

### Prompt conventions

- Corrected the earlier misreading: the rule is about prompt structure, not
  cross-turn context. Prohibitions come first, recommended behavior after.
- Instructions are written in English; entity names, identifiers, file paths
  and user-provided text keep their original language and spelling.
- Updated the coding and companion prompts to that structure and documented
  the convention in `docs/spec/02-profile.md` and `backlog.md`.

### Secret hygiene

- A key was pasted into the tracked `.env.example`. Moved it to the gitignored
  `.env`, restored the example to a placeholder, and advised rotation. No key
  is present in the tracked files.

### Real-model turn limits

- Raised the default turn budget to 12 model calls, 30 s per tool and 180 s per
  turn so a real provider can finish a coding round without a local timeout.
- Verification: 123 tests, clippy, fmt and `cargo deny check`.

### Reasoning effort and first real coding run

- Added an optional profile/provider `reasoning_effort` hint. The DeepSeek
  profile sets `none` because `deepseek-flash` thinking mode requires
  `reasoning_content` to be echoed back in tool loops.
- Ran `deepseek-flash` on `task-001`: the model fixed `calc.py`, and the
  external hidden tests passed 4/4.
- Shell attempts were denied in the non-terminal run; the model reported the
  fix as unverified instead of claiming a test run.
- Report: `evals/reports/2026-11-02-task-001-deepseek.md`.
- Verification: 124 tests; clippy, fmt and `cargo deny check`.

### Usage and latency reporting

- `TurnOutcome` now carries summed token usage and turn duration; the CLI
  prints `usage` and `duration_ms`.
- A second real `task-001` run passed 4/4 hidden tests with 7437 tokens and
  5916 ms.
- Recorded the M2 acceptance state: one interactive terminal run remains, and
  the `keel-policy` crate split is explicitly waived until a second consumer.
- Verification: 124 tests; clippy, fmt and `cargo deny check`.

### Adaptive reasoning ADR and reasoning content

- Recorded ADR 0005: kernel-owned `Low -> High -> Max`, no user switch,
  failure-driven escalation, and no context-overflow trigger.
- `Message` now carries `reasoning_content`; the driver accumulates thinking
  deltas and the OpenAI-compatible wire echoes it on assistant tool-call
  messages. This is required by DeepSeek thinking mode.
- Added `docs/llmwiki/tasks/adaptive-reasoning.md` for the remaining
  controller and verifier-signal work.
- Verification: 125 tests; clippy, fmt and `cargo deny check`.

### Adaptive reasoning controller

- Added kernel-owned `ReasoningLevel` (`Low -> High -> Max`) to the driver.
  It starts at Low, is monotonic within a turn and resets next turn.
- Removed fixed `reasoning_effort` from profiles. `supports_reasoning` is a
  provider capability; the adapter sends `reasoning_effort` only when true.
- Escalation signals: invalid tool arguments, repeated invalid calls,
  no-progress rounds and required verifier failures. Permission denied, not
  found, timeout, cancellation, I/O and context overflow do not escalate.
- Assistant messages now combine text and tool calls, and reasoning content is
  echoed (including an empty string) for thinking-mode providers.
- Real task-001 stayed at Low, passed 4/4, used 15361 tokens in 10702 ms.
- Verification: 129 tests; clippy, fmt and `cargo deny check`.
