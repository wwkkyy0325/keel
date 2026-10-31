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

### Public dataset importer

- Added `evals/harness/import_public.py` for local JSONL/JSON or the Hugging
  Face rows API, covering HumanEval+ and MBPP+; output is gitignored.
- Generated cases include a function skeleton, reference solution, hidden
  runner, `verify.sh` and `meta.toml`.
- Validated both styles with local fixtures: setup fails, reference passes.
- `--download` is implemented but the datasets server was unreachable here.
- Commit: `feat: support HumanEval+ and MBPP+ imports`.
- Added `.github/workflows/ci.yml` for Rust gates, Python compile checks and
  importer fixture validation.

### Public suite

- Imported 20 HumanEval+ and 20 MBPP+ cases through the local proxy; cases stay
  gitignored.
- HumanEval+: 20/20, 115,847 tokens, 105 s.
- MBPP+: 19/20, 105,723 tokens, 119 s; `Mbpp/16` is an ambiguous public prompt.
- Fixed MBPP entry-point selection and setup skeleton; fixed Windows suite
  filtering.
- Report: `evals/reports/2026-11-02-public-suite.md`.
- Commit: `fix: select MBPP entry points and add public suite report`.
- Added a 300,000-token suite stop; 20 + 20 is the regression baseline and a
  50 + 50 expansion was deferred for cost.
- Commit: `feat: cap eval suite spend`.

### M3 history and memory design

- Recorded ADR 0006 with the user's model: topic segmentation, recent-turn
  detail, budget-driven assembly, result-preserving compression, tool output
  truncation by character count, automatic stage triggers, and fact-driven
  segment retention.
- Segment boundaries are kernel-owned and deterministic; the model cannot open
  or seal a segment.
- Compression levels are `L0` raw, `L1` truncated tool output, `L2` turn
  digest, `L3` segment record and `L4` fact folding. Only segment seal and
  assembly pressure trigger compression.
- A sufficiency gate keeps raw entries when the candidate record is missing a
  required field; `CompressionSkipped` records the reason.
- Facts are extracted from structured signals and promoted to global scope only
  under strict conditions; everything else stays session-scoped.
- Summarization is deliberately unresolved: `tasks/m3-summarization-research.md`
  compares structured extraction, model summaries and a hybrid, with a probe
  experiment and a 200k-token budget.
- Added `tasks/m3-history.md` with nine implementation steps.
- No code changed.

### M3 context governor

- Replaced the static share with a governor consulted before every model call:
  gauge over the effective window, watermarks at 50/70/85%, cheap-first
  actions, and a detail share of 80/60/40/20% by level.
- Red injects a one-shot wrap-up instruction that produces the segment record;
  one model call is reserved for wrap-up and the final answer when the turn
  call limit is nearly reached.
- Coordination: safe points only at the top of the model-call loop, no
  compaction during streaming, interactions or verification, hysteresis,
  two compactions per turn, a three-failure circuit breaker, a boundary
  marker, re-injection of task state, and a savings floor.
- Added `ContextPressure` to the event list and a provenance row for the
  public context-editing documentation.
- No code changed.

### M3 dynamic watermarks and window floor

- Replaced fixed percentages with call headroom derived from observed growth:
  amber at four calls, orange at two, red at one or at 90% of the ceiling.
- Split `budget_tokens` (working-set target) from `context_window` (hard
  capacity); config validation requires budget plus output reserve to fit.
- Provisional floors: 16k for coding with six tools, 8k for a no-tool
  companion; recorded the per-call overhead arithmetic behind them.
- Added a window-matrix test step (8k, 16k, 32k, 64k, 128k, 1M) using a mock
  model with scripted growth; the measured knee replaces the provisional
  floors.
- No code changed.

### M3 session and history pipeline

- Added `keel-core::session`: `Session`, `Segment`, `SegmentState` and `Entry`.
  Turns are recorded as user input plus assistant text; `seal_current` closes a
  segment and the next turn opens a new one. Topic labels keep the original
  language and are bounded to 80 characters.
- Changed the context contract: `BuildCtx` carries `budget_tokens`; each stage
  receives the remaining budget; `ContextStage::build` returns zero or more
  fragments, so history can emit a sequence of messages.
- Added `HistoryStage` (priority 9, before `latest`). It groups messages into
  turns, keeps whole turns only, drops the oldest first, and reserves room for
  the current input so history can never crowd it out.
- The driver owns a `Session`, passes its history into the context, and records
  every non-cancelled turn after the terminal state is known.
- Added `--session` to the CLI: one input per line until EOF, one driver for
  the whole run. All coding profiles and the companion profile list the
  `history` stage.
- Verification: `cargo fmt --check`; `cargo clippy --workspace --all-targets --
  -D warnings`; `cargo test --workspace` (141 tests). A 20-turn mock companion
  session reached `Done` on all 20 turns, and per-turn input tokens grew
  (139, 145, 152 on the three-turn smoke), confirming history is assembled.

### M3 budget assembly and context governor

- Added `model.context_window` (default 128000) and validation: the budget plus
  the output reserve must fit the window, and the window must be at least 8k
  with no tools or 16k with tools.
- Added `ContextGovernor` with headroom in calls: `(ceiling - used) / growth`,
  a 90% guard, and levels green/amber/orange/red mapping to detail shares
  80/60/40/20%. The ceiling is `min(window - reserve, budget_tokens)`.
- `HistoryStage` now groups turns, keeps the newest two as a floor, fills the
  rest of the detail share with whole turns, and compresses everything older
  into one system digest block (`- user: ... / assistant: ...`, clipped at 240
  and 400 characters). Turns that do not fit are dropped whole and counted.
- The driver emits `ContextPressure` at turn start and before every model
  call, and `HistoryAssembled` with detail/digest/dropped counts. At red it
  injects one wrap-up system message when the extra message fits the budget.
- Mid-turn compaction is not implemented yet, so an over-budget turn still
  ends in `ContextOverflow`; tool-result clearing at amber moves to step 7.
- Verification: 152 tests; `cargo fmt --check`; clippy with `-D warnings`;
  20-turn mock companion session still reaches `Done` on all turns.

### M3 reserved final call

- The last model call in a turn is reserved for the answer: the request carries
  no tool schemas and one system instruction asking for completed work, pending
  work and changed artifacts.
- A tool call returned on that final call is ignored (no dispatch). If it also
  carried no text, the turn ends in `Error` rather than a `Done` with empty
  text.
- Added `Event::FinalCallForced` and two integration tests: the final request
  has zero tools and the instruction, and a tool request on the final call is
  never dispatched.
- Verification: 154 tests; `cargo fmt --check`; clippy with `-D warnings`.

### M3 wrap-up report capture

- Added `TurnStats.awaiting_wrap_up` and `wrap_up_report`. The first non-empty
  assistant text after the wrap-up instruction is stored on the turn.
- Exposed it as `TurnOutcome.wrap_up_report`; it is a record seed, not the
  final answer. `TurnOutcome.text` keeps its existing accumulation semantics.
- The red-pressure test now asserts the captured report.
- Verification: 154 tests; fmt and clippy clean.

### M3 answer artifact (ADR 0007)

- Added `keel_core::Answer` with revision, body, changes, pending, verification,
  notes and missing.
- The driver sets the body on every text-only round (replacement, not
  concatenation), records successful `write_file` paths from the tool-call
  audit, collects deferred interactions as pending items, attaches the
  verification report and the wrap-up notes, and marks a turn with no body as
  missing.
- `TurnOutcome.answer` is the host-facing result; `text` stays the transcript.
  The CLI prints `answer:` and `changes:`.
- Added `MockStep::TextAndToolCall` for a narration-plus-tool round.
- Verification: 161 tests; `cargo fmt --check`; clippy with `-D warnings`.

### M3 tool summaries in history

- Added `keel-runtime::summary` with `compact_args` and `summarize_output`.
  Head/tail are 2000/1000 characters; args are clipped at 200; long output is
  redacted, then written once to `.keel/outputs/turn-{turn}-call-{i}.txt`.
- The driver builds one summary per tool result, appends them to
  `TurnStats`, and records them into the session with the answer body.
- `Session::record_turn` appends a `[tools]` block to the assistant entry;
  history therefore carries the calls, statuses and bounded output.
- Added `MockStep::TextAndToolCall` for narration-plus-tool rounds.
- Verification: 167 tests; the 200 kB read spills and is re-readable through
  `read_file`; fmt and clippy clean.

### M3 kernel segmentation

- Added `SegmentChange` and `TurnRecord`; segments now carry turns, terms and
  artifacts. `begin_turn` seals on verified completion, the 12-turn cap, or a
  topic change, and opens a new segment.
- Term signature: alphanumeric tokens of at least three characters, lowercased,
  minus a small stopword list. Artifact reference matches the full path or the
  file name.
- The driver calls `begin_turn` before building context and emits
  `SegmentOpened` / `SegmentSealed`.
- Fixed `last_turn_tokens` to skip the empty segment opened by `begin_turn`.
- Verification: 173 tests; fmt and clippy clean.

### M3 segment records

- Added `SegmentRecord` and `CompressionSkip` to `keel-core`. `seal_and_record`
  builds the record when a segment seals; `build_record` requires goal and
  result, and the result is the last assistant text without the `[tools]`
  block, clipped to 400 characters.
- `Session::history` now returns entries only from segments without a record;
  `Session::records` returns sealed records in order.
- `BuildCtx` carries `records`; `HistoryStage` renders them into one
  `[earlier segments]` system block with a quarter-of-budget cap, then spends
  the rest on detail and digests.
- The driver emits `SegmentSealed`, `CompressionSkipped` and `HistoryAssembled`
  with the record count.
- Verification: 176 tests; fmt and clippy clean.

### M3 fact sedimentation

- Added `Fact` and `FactScope`; segments now feed facts into two stores at
  seal. Extraction covers user constraint sentences, `Segment.artifacts` and
  `Segment.verified`.
- Constraint keys drop the cue word, so "always use tabs" and "never use tabs"
  share `constraint:use tabs`; the newer statement replaces the older one and
  can move scope.
- Added `MemoryStage` (priority 5) rendering `[memory]` with global facts
  first; registered in the CLI and added to every profile's stage list.
- The driver passes `session.all_facts()` into `BuildCtx` and emits
  `FactPromoted`.
- Verification: 182 tests; fmt and clippy clean.

### M3 conservative fact promotion

- `parse_constraint` now requires the cue at the start of the sentence, rejects
  questions (`?`, `？`), first-person starts (`I`, `we`, `我`), and sentences
  containing scoping markers (`this task`, `here`, `这次`, `暂时`, ...).
- Added a candidate counter keyed by fact key and statement. A durable
  constraint is promoted only on its second distinct segment; a different
  statement resets the count to one and demotes the fact.
- Updated the fact tests: two-segment promotion, contradiction demotion, and
  four rejection cases. The driver integration now asserts a single statement
  stays session-scoped.
- Verification: 186 tests; fmt and clippy clean.
