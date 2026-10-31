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

### Acceptance verification and policy split

- The driver now runs host-provided acceptance items at `Verify`. Required
  failures escalate reasoning, feed evidence back, and reprompt up to a bound;
  exhausted retries end in `verification_failed`. `VerificationFinished` events
  and `TurnOutcome.verification` carry the evidence.
- The CLI accepts `--acceptance FILE` with a TOML checklist; `task-001` has a
  real command check that imports the workspace `calc.py`.
- Split profile parsing, `RuntimeConfig` and `TurnLimits` into the new
  `crates/keel-policy`. Host capability validation stays in `keel-runtime`,
  which re-exports the policy API.
- Real task-001: kernel acceptance passed; external hidden tests 4/4.
- Verification: 131 tests; clippy, fmt and `cargo deny check`.

### Hardening harness and self-authored cases

- Added `evals/harness/run_case.py` and `run_suite.py`; each run records state,
  exit codes, usage, duration, reasoning, verification and output tails.
- Added a readiness assessment and risk register, plus a hardening task doc.
- Added nine self-authored cases (ten total with task-001): JSON merge,
  pagination, CLI validation, retry, ranking, storage, multi-file greeting,
  stable de-duplication and JSON Lines totals.
- Added `profile.eval.toml` without the shell tool; the kernel verifier still
  runs acceptance commands.
- Fixed a budget-exhaustion path that could report `Done` after a failed or
  missing verification; it now ends in `verification_failed`.
- Suite result: 10/10 passed, 152,807 tokens, 103 s.
- Verification: 132 tests; clippy, fmt and `cargo deny check`.

### Provider resilience

- Retry transient provider failures before a stream starts: HTTP 429/5xx,
  connect, timeout and request errors. Backoff is cancellation-aware.
- The CLI allows two retries; tests default to zero for deterministic error
  assertions.
- The driver bounds each model call to 120 s, below the 180 s turn deadline.
- Added a local-server test where two 500 responses are followed by a healthy
  stream.
- Verification: 133 tests; clippy, fmt and `cargo deny check`.

### Public dataset importer

- Added `evals/harness/import_public.py` for local JSONL/JSON or the Hugging
  Face rows API, covering HumanEval+ and MBPP+. It creates `setup/`,
  `reference/`, hidden test runner, `verify.sh` and `meta.toml` under a
  gitignored `evals/cases/public/`.
- Validated both datasets with local fixtures: setup fails and reference
  passes for HumanEval-style `check(candidate)` and MBPP-style assertions.
- The datasets server was unreachable from this environment, so the download
  path remains untested here; the local-file path works.
- Added a GitHub Actions workflow for fmt, clippy, tests, `cargo deny`, Python
  compile checks and importer fixture validation.
- Verification: both importer fixtures; no Rust changes.

### Public suite

- Downloaded HumanEval+ and MBPP+ through the local proxy and imported 20
  cases from each.
- HumanEval+: 20/20; 115,847 tokens in 105 s.
- MBPP+: 19/20; 105,723 tokens in 119 s. The one failure (`Mbpp/16`) is an
  ambiguous public prompt about repeated underscore groups.
- Fixed MBPP entry-point selection (test-referenced function, not the first
  helper) and generated a setup skeleton from the canonical signature.
- Fixed suite filters to compare POSIX paths on Windows.
- Report: `evals/reports/2026-11-02-public-suite.md`.
- Added a 300,000-token suite stop; 20 + 20 is the regression baseline and a
  50 + 50 expansion was deferred for cost.

## 2026-11-02 — M3 history design

- Recorded ADR 0006: session, segment and entry model; kernel-owned topic
  segmentation; budget-driven assembly; `L0`–`L4` compression ladder with only
  two triggers; fact-driven segment retention; the sufficiency gate; tool
  output truncation by character count with a spill file; strict promotion of
  session facts to global scope.
- Added `tasks/m3-history.md` with nine verifiable steps and
  `tasks/m3-summarization-research.md` with the open summarization question,
  candidate approaches and the decision rule.
- `L2` and `L3` stay model-free until that research note has a result.
- Updated the design summary, the backlog and the profile spec pointer.
- No code changed.

### M3 context governor

- Replaced the fixed detail share with a governor: a per-call gauge over the
  effective window, watermarks (green, amber, orange, red, critical), a detail
  share that varies by level, and cheap-first actions.
- Added the wrap-up injection at red, a reserved model call for wrap-up and the
  final answer when the turn budget is nearly exhausted, hysteresis, a
  compaction circuit breaker, a boundary marker, re-injection after compaction
  and a cost guard.
- Added a provenance entry for the public context-editing documentation used as
  a design reference; no code was copied.

### M3 dynamic watermarks and window floor

- Replaced percentage watermarks with headroom measured in calls:
  `headroom_calls = (ceiling - used) / growth`, with a 90% guard for growth
  spikes. A fast coding turn now compacts earlier in absolute tokens; a slow
  chat runs longer.
- Split the two budgets: `budget_tokens` is the working-set target per call,
  `context_window` is the hard capacity. A 1M window no longer implies an
  850k trigger.
- Recorded the observed plateau near 200k as the reason for a working-set
  target rather than a percentage of the window.
- Set provisional floors at 16k for coding and 8k for no-tool profiles, with
  validation and a window-matrix test (8k to 1M) to replace them with measured
  values.

### M3 session and history pipeline

- Added `Session`, `Segment`, `SegmentState` and `Entry` to `keel-core`. A turn
  records the user input and the assistant text; an open segment is created on
  first use and `seal_current` starts a new one.
- `BuildCtx` now carries `budget_tokens` (remaining for the stage) and the
  history slice; a context stage may return several fragments.
- Added `HistoryStage`: it keeps whole turns, never splits a user/assistant
  pair, drops the oldest turns first, and always leaves room for the current
  input.
- The driver owns a session and records every non-cancelled turn, so later
  turns see earlier ones.
- The CLI has a `--session` loop that reads one input per line until EOF and
  keeps the driver across turns; all profiles list the `history` stage.
- Verification: 141 tests; a 20-turn mock companion session reaches `Done` on
  every turn; `cargo fmt --check`, clippy and `cargo test --workspace` pass.

### M3 budget assembly and context governor

- Split `context_window` (model hard capacity) from `context.budget_tokens`
  (working-set target). Profiles are rejected when the budget plus output
  reserve exceeds the window, or when the window is below 8k with no tools or
  16k with tools.
- Added `ContextGovernor`: pressure is headroom in calls at the observed
  growth rate, with a 90% guard for growth spikes. Levels map to detail shares
  of 80/60/40/20%.
- `HistoryStage` keeps whole turns: the newest two are a floor, the rest of the
  detail share stays verbatim, and older turns become one compressed digest
  block; turns that do not fit are dropped whole and counted.
- The driver measures pressure at turn start and before every call, emits
  `ContextPressure`, and injects one wrap-up instruction at red when the
  message fits. An over-budget turn still ends in `ContextOverflow` until
  mid-turn compaction lands.
- Added `HistoryAssembled` with detail/digest/dropped counts and `WrapUpInjected`.
- Verification: 152 tests; a 20-turn mock companion session still reaches
  `Done` on every turn.

### M3 reserved final call

- The last model call of a turn is reserved for the answer. The kernel adds one
  instruction message and sends zero tool schemas, so the model cannot start new
  work.
- If a model still requests a tool on that call, the call is not dispatched and
  the turn ends in `Error` instead of a silent `Done` with empty text.
- Added `FinalCallForced` and two tests: the final request has no tools and the
  instruction, and a tool request on the final call is never dispatched.
- Verification: 154 tests; fmt and clippy clean.

### M3 wrap-up report capture

- The first assistant text after the context wrap-up instruction is now kept
  separately as `TurnOutcome.wrap_up_report`, instead of being only anonymous
  text in the transcript.
- It is not substituted for the final answer. It exists so the session can seed
  a segment record when the segment is sealed, and so a host can audit what the
  model reported at the pressure point.
- Verification: the wrap-up test asserts the report is captured; 154 tests,
  fmt and clippy clean.

### M3 answer artifact (ADR 0007)

- Added `Answer` to `keel-core`: revision, body, changes, pending, verification,
  notes and a missing flag. It is separate from the transcript.
- The body is the latest assistant round without tool calls; narration is
  excluded. Changes come from kernel-observed `write_file` successes and are
  deduplicated by path. Deferred interactions become pending items; the
  verifier report is attached; the wrap-up report is kept as notes.
- An empty body falls back to notes and otherwise sets `missing`.
- `TurnOutcome.answer` carries the artifact; the CLI prints `answer:` and a
  `changes:` line.
- Verification: 161 tests; narration exclusion, change recording and the
  missing-answer case are covered.

### M3 tool summaries in history

- Added `summary`: tool arguments are rendered as one line clipped at 200
  characters; results over 3000 characters keep the first 2000 and last 1000,
  with a marker naming `.keel/outputs/turn-{turn}-call-{index}.txt`.
- The full output spills once to that path. Both the summary and the spill are
  redacted first.
- `Session::record_turn` now takes tool summaries and appends a `[tools]` block
  to the assistant entry, so the next turn sees what ran and what it returned.
- The turn records the answer body plus the tool block; the transcript keeps
  the full output.
- Verification: 167 tests; a 200 kB `read_file` result truncates, spills, and is
  readable back through `read_file`.

### M3 kernel segmentation

- `Session::begin_turn` decides boundaries before a turn runs: it seals the
  open segment when the previous turn passed an acceptance plan, when the
  segment reached 12 turns, or when the input's term overlap is below 0.2 and
  it names none of the segment's artifacts.
- Segments track their turn count, term signature and artifact paths.
- The driver emits `SegmentOpened` and `SegmentSealed` with the reason.
- `last_turn_tokens` now looks at the newest non-empty segment, because
  `begin_turn` may have opened an empty one.
- Verification: 173 tests; topic change, follow-up, artifact reference,
  verified completion and the cap are covered in unit and integration tests.

### M3 segment records

- Sealed segments now become `SegmentRecord`s: goal, result, changes, pending,
  verification flag and turn count. The result excludes the `[tools]` block.
- The sufficiency gate requires a goal and a result. A segment that fails it
  keeps its raw entries, gets no record, and reports `CompressionSkipped` with
  the missing fields.
- History now shows sealed records as one `[earlier segments]` block; their raw
  entries are no longer replayed. Records get at most a quarter of the history
  allowance; older records are dropped first.
- `HistoryAssembled` reports `record_segments`.
- Verification: 176 tests; a topic change replaces the old raw turn with a
  record line, and an unanswered segment keeps its entries and reports the
  gate failure.

### M3 fact sedimentation

- Sealing a segment now extracts facts from structure the kernel already has:
  user constraints (cue words), changed artifacts, and verified segments.
- Facts have a key, statement, scope, confirmation flag and segment id. A
  durable constraint (always, never, from now on, 以后, 永远, 总是) is promoted
  to global; artifacts and verified outcomes stay session-scoped.
- A newer fact with the same key replaces the older one in either store, so a
  contradiction demotes it.
- Added `MemoryStage`: global facts first, then session facts, as one bounded
  `[memory]` block. All profiles list `memory` ahead of `history`.
- `FactPromoted` reports each promotion.
- Verification: 182 tests; promotion, transient artifacts, non-durable
  constraints and contradiction replacement are covered.

### M3 conservative fact promotion

- Constraint extraction no longer accepts any sentence containing a cue. Only
  sentence-initial imperative sentences count; questions, first-person
  narration, mid-sentence cues and phrases scoped to the current task are
  rejected.
- A durable cue marks a candidate, not a global fact. A candidate is promoted
  only after the same statement appears in a second segment; a different
  statement for the same key resets the count and demotes it.
- This replaces the previous keyword-to-global rule, which could promote
  habits, questions or task-local instructions.
- Verification: 186 tests; the new rejection cases and the two-segment
  promotion and demotion paths are covered.

### M3 explicit and repeated fact promotion

- A single explicit request ("remember", "记住", "请记住", "from now on",
  "以后都") now promotes a constraint immediately.
- Without that request, a durable constraint must appear in at least two
  separate turns; it no longer needs two different topic segments. This
  removes the case where a single-topic session could never promote a genuine
  preference.
- Repetition is counted per user turn, not per sentence, and a different
  statement for the same key still resets the count and demotes the fact.
- Verification: 188 tests, including two mentions in one segment and immediate
  promotion through an explicit request.

### M3 three-tier memory with rollback

- Facts now live in `Session`, `Workspace` or `Global` scope. Automatic
  promotion stops at workspace: an explicit "remember" promotes there, a
  workspace phrase promotes there, and a durable constraint repeated in two
  turns promotes there.
- Global scope requires an explicit request that also names a global scope
  ("everywhere", "all projects", "全局", "所有项目").
- A newer statement for the same key replaces the fact in every store, so a
  correction demotes it. "Forget X" / "忘记 X" removes matching facts from all
  three stores and emits `FactForgotten`.
- `MemoryStage` labels facts as global, workspace or session and never takes
  more than a quarter of the context allowance.
- Verification: 191 tests; tier placement, global gating, contradiction
  demotion and explicit forgetting are covered.

### M3 window matrix

- Added `crates/keel-runtime/tests/window_matrix.rs`: 40 deterministic turns at
  8k, 16k, 32k, 64k, 128k and 1M for a no-tool and a tool-using profile, with
  digests, records, wrap-ups and overflow counted.
- Results: companion 40/40 at every window; coding 40/40 at 16k and above; 8k
  coding only 37/40 with three overflows and 35 wrap-ups.
- Recorded the run in `docs/llmwiki/window-matrix.md`; the ADR 0006 floors of
  16k coding and 8k no-tool are now measured, not provisional.

### M3 real-token calibration

- The 20-turn DeepSeek companion run at an 8k window finished 20/20 with no
  overflow, using 22.9k input tokens; the remembered preference was used
  several turns later.
- Fixed the token estimate for CJK: one token per character instead of four
  characters per token, which had underestimated Chinese by about 4x.
- The driver now reserves the measured gap between reported input tokens and
  the kernel estimate, covering tool schemas and provider template overhead
  (about 390 tokens for the DeepSeek wire).
- Chinese segmentation uses character bigrams plus follow-up markers, and the
  newest segment stays verbatim even after it seals; records cover older
  history only.
- The synthetic window matrix is now documented as a machinery regression, not
  as evidence about real token counts.

### M3 real coding session

- Ran 20 real coding turns with four tools at a 16k window: 20/20 `Done`,
  271.6k input and 20.6k output tokens, no overflow or wrap-up. The generated
  tests passed 15/15 when run externally.
- Verified the final workspace: renamed parameters, type hints, docstrings,
  `ValueError` on zero division, `pow2`, bitwise `is_even`, `clamp` and the
  README all match the tasks.
- The run found that `write_file` could not create a missing parent directory.
  It now creates parent directories inside the workspace, with a test.
- Added `variants/coding/profile.deepseek.session.toml` for headless sessions
  and `evals/reports/2026-11-03-coding-20-turn.md`.

### M3 model segment summaries

- The segment level now gets one model-written summary per sealed segment when
  the transcript is at least 800 estimated tokens. L2 turn digests stay
  structured and model-free.
- The summary runs through the same provider with 300 output tokens, a 30-second
  timeout and cancellation; failure falls back to the structured record. The
  prompt forbids new facts and requires paths, identifiers and numbers to stay
  exact.
- The summary is clipped to 800 characters, rendered in place of the one-line
  result, and its token usage is added to the turn's reported usage.
- A real three-turn smoke triggered one summary (63 characters) and showed the
  record in the next request; `summary:` is now printed by the CLI.
- Verification: 204 tests; large/short segment gating, fallback and usage
  accounting are covered.

### M3 L4 folding and storage design

- Records whose changes are all covered by artifact facts and that have no
  pending items are folded: they leave the context projection but stay in
  storage. The newest segment is never folded.
- Folded records without a summary are appended to the next segment summary,
  marked `[segment N]`. On success the new record lists them in `covers`, and
  each one records `absorbed_by`.
- Added ADR 0008: four storage planes (audit, transcript, derived context,
  facts), a SQLite schema, soft flags instead of deletes, context snapshots and
  provenance. Implementation is deferred to M5.
- Verification: 208 tests; folding rules, later-summary absorption and the
  driver marker are covered.

### M3 rolling session memo

- Replaced the per-segment model summary with one rolling session memo,
  rewritten at every seal. `SessionSummary` stores the text, revision,
  cumulative coverage and the token budget; absorbed records leave the
  context projection but stay in storage.
- The update input is the previous memo, the new structured record, the newest
  transcript clipped at both ends, and unabsorbed records that fit the input
  budget. The output budget scales with the working set (10%, 256-4096 tokens)
  and halves at orange or red pressure.
- Records with open items are never absorbed. A failed update keeps the
  previous memo and leaves the record block visible for the next attempt.
- ADR 0006 and ADR 0008 updated for memo revisions, `absorbed_in` and
  `summary_covers`.
- Real 5-turn companion smoke at 8k produced three memo revisions (1144, 2185
  and 2645 characters) under a 677-token budget, with no record block after
  absorption and correct constraint recall on the last turn.
- Verification: 216 tests; fmt and clippy clean.

### M3 targeted recall and closeout

- Added deterministic lexical recall for absorbed or folded segments: score by
  shared terms and artifact paths, return at most two, and attach a clipped,
  redacted excerpt from the best matching turn. No model and no index.
- `HistoryStage` emits `[recalled earlier segments]` under a fifth-of-remaining
  and 400-token cap, before the fallback records; `HistoryRecalled` reports the
  segment ids and cost.
- Real 24-turn companion run: 24/24 `Done`, 20 memo revisions (484-2679
  characters, budget 675-677), context 843-1479 tokens, and recall recovered
  `config/marlin.toml` and the pasta recipe on the final probes. Report:
  `evals/reports/2026-11-03-companion-24-turn-recall.md`.
- M3 is closed. Vector recall stays deferred.
- Verification: 220 tests; fmt and clippy clean.

### Novel 50-chapter pilot

- Added a novel variant (`profile.deepseek.novel.toml`), per-block allocation
  fields on `HistoryAssembled`, session `error:` printing, and a host
  `--max-model-calls` flag.
- The pilot ran 54 turns in one session but failed: 34 `Done`, 20 `Error`, 14
  files and 31,464 Chinese characters; 40 turns changed no file. One turn could
  not read, write a ~3,000-character chapter, update a growing state file and
  answer within the output/tool budgets, and the model started following its
  own gap list in the state instead of the prompt.
- Recall fired on 40 turns at an average of 620 tokens against a nominal 400
  cap. The cap was bypassed because the first hit was always admitted; that is
  fixed. The per-block allocation data shows the governor was not the
  bottleneck.
- Report: `evals/reports/2026-11-03-novel-50-turn-pilot.md`.
- Verification: 220 tests; fmt and clippy clean.

### Task state (ADR 0009)

- Added `TodoList` / `TodoItem` to `keel-core`: host and model sources, locked
  host lists, one `in_progress`, 100 items, 200 characters.
- Added the `todo` tool (list/add/update/remove) and a pinned `task_state`
  context stage between memory and history. Task state is never summarized,
  folded or recalled.
- The driver shares the list with the tool, snapshots it into `BuildCtx` and
  emits `TodoUpdated`; the CLI seeds host tasks with `--todo-file` and prints
  a task summary after every turn.
- The novel variant now uses `task_state` and `todo`; the project seeds 50
  chapter tasks and shortens chapters to 1,800-2,200 characters.
- List lifecycle: `completed` auto-closes on the last item; all-terminal lists
  with a failure close as `failed`; explicit `close` supports `completed`,
  `failed` and `abandoned`. Closed lists reject edits and collapse to one
  summary line; `TodoClosed` and the CLI report the outcome.
- The driver now bounds live tool results with the same head/tail summary used
  for history, so a large `read_file` cannot overflow the current turn.
- Verification: 226+ tests; fmt and clippy clean. The ten-chapter smoke is
  tracked in `docs/llmwiki/tasks/task-state.md`.

### Kernel ablation switches

- Added `KernelOptions { recall, memo, bounded_tools }` to the driver and CLI
  flags `--no-recall`, `--no-memo`, `--unbounded-tools`, `--no-todo`. The
  defaults keep every mechanism on; the switches exist for controlled
  ablations, not variant policy.
- Added a fixed ten-chapter ablation workload in
  `D:\workspace-new\wwkkyy0325\ascent-ablation`: 1,200-1,800 characters,
  chapter-only prompt, ten host tasks, one arm per mechanism removed.
- Verification: 230 tests; fmt and clippy clean. Arm results are tracked in
  `docs/llmwiki/tasks/ablation.md`.

### Kernel ablation results

- Ran five fifteen-chapter arms under one fixed protocol: full, no-todo,
  no-recall, no-memo and unbounded. Report:
  `evals/reports/2026-11-03-kernel-ablation.md`.
- Bounding live tool results is load-bearing: the-four bounded arms had zero
  overflow; unbounded needed three elisions and still overflowed once.
- The task list buys tracking and closure but costs the most observed overhead:
  full used 776k input tokens and closed 15/15; no-todo used 452k and wrote
  14/15 without closure. Two todo round-trips per chapter are the likely cost.
- Memo and recall are not load-bearing in this workload: the state file carries
  the story. No-recall still wrote 15/15 and used slightly less input; no-memo
  wrote 14/15 with similar input. Probes or a state-less workload are needed.
- Missing chapters in no-memo and no-todo are single-run model variance.

### Structured memo fallback

- When the model summary fails or times out, the kernel now writes a
  deterministic memo from the structured records, keeps the previous memo text
  and covers only the records that fit the budget. Uncovered records stay
  visible. `SummaryUpdated.fallback` reports the path.
- The full-15 pilot showed `summarizer returned no text`, which left no memo
  and therefore no recall candidates; the fallback keeps the memory pipeline
  alive when the model call fails.
- Verification: 232 tests; fmt and clippy clean.

### Recall budget and mid-turn compaction

- Recall now takes a quarter of the remaining history allowance (minimum 400
  tokens) instead of a fixed 400-token ceiling. The full-15 pilot showed a
  chapter-sized hit being dropped with `recall=0(0/1)`; the hit now fits.
  `HistoryAssembled` reports the candidate token count.
- Before a context overflow, the driver elides old tool-result bodies, keeping
  the latest tool-call round. `ContextElided` reports how many messages were
  shortened and the token delta. This covers growth inside a turn, which the
  turn-start governor cannot see.
- Verification: 232 tests; fmt and clippy clean.
