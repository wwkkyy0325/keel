# Changelog

## 2026-10-31

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

## 2026-10-31 — Permission and interaction refactor

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

## 2026-10-31 — M2 OpenAI-compatible provider

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
- Report: `evals/reports/2026-10-31-task-001-deepseek.md`.
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
- Report: `evals/reports/2026-10-31-public-suite.md`.
- Added a 300,000-token suite stop; 20 + 20 is the regression baseline and a
  50 + 50 expansion was deferred for cost.

## 2026-10-31 — M3 history design

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
  and `evals/reports/2026-11-01-coding-20-turn.md`.

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
  `evals/reports/2026-11-01-companion-24-turn-recall.md`.
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
- Report: `evals/reports/2026-11-01-novel-50-turn-pilot.md`.
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

### Interjection contract

- Added ADR 0010: interjections are queued user messages delivered at safe
  points, not interactions and not cancellations. The host chooses `queue`,
  `interrupt` or `next_turn`; delivery never splits a tool-call/result pair and
  never happens during a stream.
- The contract is fixed now; implementation waits for a host with a mid-turn
  reader. The line-based CLI already queues between turns through stdin.

### Derived current task

- `TodoList::current` returns the explicit `in_progress` item or the first
  pending item. The `[tasks]` block renders a `current: #id` line; the driver
  and CLI use it for events and progress.
- The model now marks only completion, saving one todo round-trip per task.
  The novel persona and the ablation prompts use the derived form.
- Verification: 233 tests; fmt and clippy clean.

### Kernel ablation results

- Ran five fifteen-chapter arms under one fixed protocol: full, no-todo,
  no-recall, no-memo and unbounded. Report:
  `evals/reports/2026-11-01-kernel-ablation.md`.
- Bounding live tool results is load-bearing: the-four bounded arms had zero
  overflow; unbounded needed three elisions and still overflowed once.
- The task list buys tracking and closure but costs the most observed overhead:
  full used 776k input tokens and closed 15/15; no-todo used 452k and wrote
  14/15 without closure. Two todo round-trips per chapter are the likely cost.
- Memo and recall are not load-bearing in this workload: the state file carries
  the story. No-recall still wrote 15/15 and used slightly less input; no-memo
  wrote 14/15 with similar input. Probes or a state-less workload are needed.
- Missing chapters in no-memo and no-todo are single-run model variance.

### Fusion fixes: task order, recall gate, acceptance detail, host guard

- Locked host plans are now sequential: only the current item may change
  status; a later `completed`/`in_progress` is rejected with `OutOfOrder`.
  Tests cover out-of-order completion and in-progress.
- Recall skips long task briefs (more than 24 query terms) unless the input has
  a question mark or a recall marker. The 50-chapter prompts no longer pull two
  old segments every turn; short probes still recall.
- Acceptance failure messages now include the verifier detail, so a reprompt
  tells the model what failed, not just the item id.
- The CLI has `--turn-input` (write the current input for an external checker)
  and `--stop-on-error` (a long-run host does not advance after an error).
  Added a 24k novel profile for long tool-heavy workloads.
- Verification: 242 tests; fmt and clippy clean.
- 15-turn slice (24k, per-turn checker, stop-on-error): 15/15 Done, 0
  overflow, 14/14 chapters, 14/14 tasks closed, 15/15 acceptance passes,
  context peak 13,305/23,846. Recall fired on 12 turns because `之前` matched
  the instruction `写之前`; the markers now require phrases such as
  `之前说`/`之前提` instead of bare `之前`/`前面`.
- 3-turn micro-slice after the marker fix: 3/3 Done, 0 overflow, 2/2 chapters,
  2/2 tasks closed, no recall on chapter turns, the probe answered correctly.
- Local 26B attempt at 20k/8k: the 15-turn slice stopped at turn 1 because the
  state file was not written (chapter 1 had 1,667 CJK); a micro-slice passed
  chapter 1 but missed `ch-002.md` while marking task 2 completed. Acceptance
  caught both and `--stop-on-error` stopped. The 26B used 13.6 GB VRAM and left
  about 0.4 GB free RAM; it was stopped after each attempt. The 26B can write
  one chapter when told to use `write_file`, but not reliably the next one.
- Task completion is not evidence-gated: the task list can close as
  `completed` while the expected file is missing. Acceptance remains the
  authority; a host-side revert or evidence rule is a follow-up.
- First full 50 attempt on DeepSeek: 10 turns, 9 Done, 1 model-stream timeout
  at turn 10; 10 chapters with no gaps, tasks 9/50, acceptance 9/9, 0 overflow,
  0 recall. `--stop-on-error` stopped the run. The chapter and state for turn
  10 were written before the timeout.
- Added CLI `--retry-errors N`: re-run the same input after a failed turn,
  bounded, so a transient stream timeout does not end a long run.
- The second full 50 showed the retry was not idempotent; the fix is recorded
  below.

### Fusion run: negative result at 16k

- The 57-turn fusion (50 chapters, 7 probes, tools, 50 locked tasks, memo,
  recall, acceptance; DeepSeek remote) failed to compose at 16k: 49 Done / 8
  `ContextOverflow`, 28/50 chapters, 17 missing chapter numbers, 28/50 tasks,
  current stuck at #10 then #42, 48 memo revisions with 16 failures, 50 recall
  events, and no acceptance failure despite 16 chapter turns returning Done
  without the requested file.
- Root causes: the host loop advanced after `Error` and after silent skips;
  acceptance checked only the latest chapter/state; the locked task list
  allowed out-of-order completion; 16k was too small for three bible reads plus
  a chapter write; recall over-fired on long chapter briefs (0.90–0.94 with
  143/180 hits); the CLI binary predated the summary floor.
- The kernel's failure paths worked (typed overflow, 17 elisions, recall
  answered the chapter-8 probe correctly), but the seams did not. Fixes are
  listed in `tasks/fusion-run.md`; no kernel code was changed by this run.
- Report: `evals/reports/2026-11-01-fusion-run.md`.

### Memory boundary and evaluation plan

- ADR 0012 fixes the boundary: the kernel owns working, episodic, semantic and
  static procedural memory mechanisms; vector stores, temporal graphs,
  embedding services, LLM extraction pipelines, MCP memory servers and
  cross-session backends are ports, adapters or plugins. No vendor memory was
  imported and no kernel code changed.
- `01-kernel.md` now says `MemoryStore` is a host port and lists vector, graph
  and embedding clients under "the kernel does not"; `03-modules.md` names the
  port layer.
- Added a memory evaluation task: the five LongMemEval-style abilities.
  Extraction, knowledge update, forget and recall abstention have kernel tests;
  multi-session and temporal reasoning wait for M5 persistence, and
  answer-level abstention needs a host probe.
- Validation matrix updated with the memory abilities and their gaps.
- Report/context: `docs/llmwiki/memory-comparison.md`; no code copied.

### Summary quality floor

- A model-written summary below `max_tokens / 4` tokens, clamped to 64..256,
  is now a failed summarizer call. The driver keeps the previous memo and
  writes the structured fallback instead of absorbing a segment into a
  near-empty summary.
- The observed case was a 126-character Chinese memo against a 756-token
  budget. Normal summaries of 1,000+ characters pass.
- Verification: unit tests for the 126-character case and the floor scaling;
  239 tests, fmt and clippy clean. ADR 0006 records the gate.

### IDF recall scoring and Chinese queries

- Recall now filters query-only question and meta words (English and Chinese
  bigrams), weights terms by IDF over the session, and ignores query terms that
  never occur in the session when normalizing. Segment terms are unchanged, so
  topic segmentation does not move.
- A recall marker (`remember`, `earlier`, `之前`, `记得`, ...) lowers the
  threshold from 0.34 to 0.20; a matched term present in every segment is not
  enough unless an artifact path is named. Artifact hits floor the score at
  0.9.
- `RecalledSegment` and `HistoryRecalled` now carry `score_bp`, `matched`,
  `query_terms` and `known_terms`; the CLI prints
  `recall: score=0.73 hits=3/4 known=4 segments=[0] tokens=257`.
- Validation on DeepSeek at the 8k companion profile: 24/24 `Done`; the Marlin
  probe scored 0.73 with 3/4 matches and answered `config/marlin.toml`; the
  pasta probe scored 1.00 with 1/2 known and answered `aglio e olio`. The 26B
  local rerun was deferred because the GPU was in use by another user.
- Verification: 237 tests; fmt and clippy clean. ADR 0011.

### Local model probe and control-tool fix

- Ran the 24-turn companion probe against a local llama.cpp endpoint
  (FableVibes-14B, 32k context) at 8k, 16k and 24k working sets: 24/24 `Done`,
  zero overflow, assembled context peaked at 2,432 tokens. Context is not the
  bottleneck for this workload.
- The endpoint exposed a kernel bug: the summarizer request advertised the
  `keel_interact` control tool, and the model answered with a plan-approval
  call, so every memo update failed with `summarizer returned no text`.
  `ModelRequest.control_tool` now gates that tool; the summarizer and the
  reserved final call clear it, and the wire builder omits an empty `tools`
  array. The same 8k arm had zero summary failures after the fix.
- The 14B still hallucinated the Marlin config path at every working-set size,
  even when recall supplied it. Model quality, not context, is the limit.
- Follow-up on Gemma 4 26B: 24/24 at 8k and 16k, zero overflow and zero summary
  failures. At 8k it answered `config/marlin.toml` from recall but missed the
  paraphrased pasta probe; at 16k both probes were correct because the memo
  retained `aglio e olio`. A novel chapter through the same endpoint used six
  model calls, wrote 1,184 CJK characters, updated the state and closed task 1,
  so domain tools work locally.
- Report: `evals/reports/2026-11-01-local-model-context.md`.
- Verification: 234 tests; fmt and clippy clean.

### Kernel affordances in personas

- The coding, novel and companion personas now state the kernel's affordances up front:
  bounded context, the rolling memo and `[recalled earlier segments]`, clipped
  tool results, mid-turn shortening, and the wrap-up/final-call reservation.
  The purpose is to stop the model from working around mechanisms or repeating
  instructions, not to add a second contract.
- No kernel code changed. The five-arm ablation numbers predate this prompt
  text; future arms should be compared within the same persona version.

### Recall A/B and memo sufficiency

- A 24-turn scripted companion run now has a `--no-recall` counterfactual:
  both arms reached 24/24 `Done` with no overflow and 21 memo revisions. On the
  turn 23 probe, the full arm named `config/marlin.toml` from recalled segment
  0; the no-recall arm said there was no Marlin project. Recall is load-bearing
  for an absorbed early detail.
- The turn 24 "pasta recipe" probe missed recall because the segment contains
  `aglio e olio` but not the word `recipe`; the score fell below the 0.34
  floor. This is the measured cost of lexical-only recall, not a kernel defect.
- A secondary 12-setting probe with the 1,170-token memo budget answered
  correctly with and without recall: the memo already carried the details.
  Memo budget relative to the sealed segment decides whether recall is needed.
- Report: `evals/reports/2026-11-01-recall-probe.md`.

### Derived current task and model-call telemetry

- `TodoList::current` derives the current task from the explicit in-progress
  item or the first pending item. The `[tasks]` block shows `current: #id`, and
  the model marks only completion, removing one todo round per task.
- The derived full-15 arm wrote 15/15, closed the list, had zero overflow and
  used 722,580 input tokens versus 776,150 for the explicit arm (-6.9%).
  Per-turn variance is larger than the delta, so the run alone does not
  attribute the saving.
- `TurnOutcome::model_calls` counts main and summary calls; the CLI usage line
  now prints `calls=N` so arm comparisons can use round-trips directly.
- Observation: the derived arm wrote a 126-character memo (budget 756 tokens)
  and the kernel still absorbed the covered segment. A failed summarizer call
  falls back, a degenerate short answer does not; this needs a probe before any
  guard.
- Verification: 233 tests; fmt and clippy clean.

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

### Retry recovery and host deadline

- The second full 50 exposed a non-idempotent retry: chapter 8 timed out twice,
  the third replay saw task #9 as current and wrote `ch-009.md` for a
  chapter-8 request. Acceptance caught the mismatch and stopped the run at
  turn 11 (7 Done / 4 Error, chapters 1–9, tasks 9/50, 0 overflow, 0 recall).
- The host now runs the acceptance checklist after a failed turn. If it passes,
  the turn counts as recovered and the session advances instead of replaying an
  input the world has moved past.
- `--turn-timeout-ms` overrides the host-owned 180 s turn deadline; the second
  run hit it three times during peak hours. A session that stops on an
  unrecovered failure now exits non-zero.
- `TokenUsage` now carries `cached_input_tokens`. The OpenAI-compatible
  provider reads DeepSeek's `prompt_cache_hit_tokens` and OpenAI's
  `prompt_tokens_details.cached_tokens`; the CLI prints `cached=` and the hit
  rate on the usage line. Cache hits are billed at a small fraction of misses,
  so the rate is a first-class cost metric for long runs.
- Report: `evals/reports/2026-11-01-fusion-run.md`.
- Verification: 246 tests; fmt and clippy clean.

### Cache observability and cost model

- The CLI now prints one `call:` line per main model call with `input`,
  `cached`, `hit` and `tools`, so cross-turn and within-turn reuse can be told
  apart. The turn aggregate line already carried `cached=` and `hit=`.
- Stage order is decided by priority, not by the profile list: system 0,
  memory 5, task_state 6, history 9, latest 10. A profile that lists stages in
  another order only changes tie-breaks, so an "inverted" profile cannot move
  the task block; that experiment was withdrawn.
- `docs/llmwiki/cache-economics.md` fixes the pricing arithmetic: at DeepSeek's
  50:1 hit-to-miss ratio the current order costs about one yuan more per
  50-turn run than the best alternative, while output tokens and window size
  dominate the real decision. A large context only wins around 100k at a 98%
  hit rate; from 200k up the extra tokens outweigh the discount. One uncached
  1M prefix costs as much as the whole bounded run, which is why a rebuild
  design must stay in a small working set.
- Verification: 247 tests; fmt and clippy clean.

### Summarizer reasoning and task evidence

- The 39% memo fallback rate was reasoning at the output cap. A DeepSeek probe
  with `reasoning_effort: low` spent 548 tokens on reasoning and 40 on content,
  hitting a 200-token limit; `reasoning_effort: none` spent 0 on reasoning and
  returned 90 content tokens. `ReasoningLevel::None` now maps to `"none"` and
  the summarizer uses it, so the output budget goes to the summary.
- The CLI snapshots the host task list before each turn. When a turn fails and
  its checklist does not pass, the snapshot is restored before a retry or a
  stop, so a failed turn cannot leave tasks completed without the artifact.
- Verification: 249 tests. A 3-turn live run with acceptance produced two
  model-written memo revisions (2,470 and 3,225 chars) with
  `fallback=false`; the first attempt had no summaries because a segment only
  seals after the previous turn passed acceptance, which ADR 0006 already
  specifies.

### Tool headroom in history assembly

- A 12-turn slice overflowed by 154 tokens at turn 2: the previous turn's
  detail filled the working set (20,958 / 23,677) and the first tool round
  added 2.9k. History assembly reserved the current input but not the turn's
  tool results, and mid-turn elision cannot drop the current round.
- History now reserves `tool_headroom = budget/6`, clamped to 512..4096 and
  zero below 2k tokens. Tight turns lose some detail instead of dying.
- An 11-turn rerun with both fixes: 11/11 Done, 11/11 acceptance, 0 overflow,
  context peak 9,784 / 23,772, and 10/10 memo revisions model-written with no
  fallback.
- The rerun also showed that a new process starts the host task list at #1:
  the slice wrote chapters 8–17 while the list stayed at `done=0`. Session and
  task persistence remain M5 work (ADR 0008).
- Verification: 250 tests; fmt and clippy clean.

### Session persistence, first slice

- `SessionSnapshot` and `SegmentSnapshot` carry segments, entries, records,
  the memo and scoped facts; `Session::restore` rebuilds the session. Fact
  candidates are transient and not carried.
- `TurnDriver::session_snapshot` and `with_session` expose the snapshot without
  teaching the kernel about a backend.
- The CLI host has a SQLite store (`--store`, `--session-id`): WAL, foreign
  keys, `schema_version`, `sessions`, `turns`. One transaction per turn writes
  the snapshot, the task list and the transcript row. Stored tasks take
  precedence over `--todo-file` on resume.
- Two processes on one store: the second process saw `history: detail=1` and
  `tasks: done=1 current=#2`, and answered a chapter-1 probe from the restored
  conversation without reading files.
- ADR 0008 is accepted; its queryable segment/record/summary/fact tables remain
  for the frontend. Task doc: `tasks/m5-persistence.md`.
- Verification: 254 tests; fmt and clippy clean.

### Frontend read model

- The store projects the snapshot into `segments`, `segment_entries`,
  `records`, `session_summaries`, `summary_covers`, `facts`, `todos` and the
  `session_current` view. Derived rows are replaced per save; summary
  revisions are append-only.
- One writer, two readers: the kernel resumes from `snapshot_json`, a frontend
  reads the tables and never the snapshot. Both are written from one snapshot
  in one transaction, so they cannot disagree.
- There is no stored session status. Latest turn, current segment, current
  summary, active facts and task progress are queries, so a stale column cannot
  contradict the rows.
- Live check: four turns projected one segment, eight entries, tasks 1/2 and
  `session_current.last_turn_seq = 4`; a second save replaced rows without
  duplication.
- `turns.answer_json` stores the structured `Answer` (revision, body, changes,
  pending, verification, notes, missing); `turns.answer` stays the body for
  listings. `records.turns` is projected too.
- Live seal check: two accepted turns sealed the first segment and produced a
  record plus memo revision 1 (1,447 chars); the next turn sealed again and
  appended revision 2 (1,802 chars); `session_current.summary_revision = 2`.
- `fact_history` and `todo_history` remain for a later slice.
- Multi-session memory eval: two processes, one store, companion 8k: 9/9 Done,
  6/6 answers (three stored facts, an update answered `海鸥`, and an explicit
  refusal for a car plate). The first run stored 16 facts, eight of them
  `verified: <question>`; verified segments now record a fact only when they
  produced artifacts. The rerun stored 8 facts with the same answers.
  Remaining: `不要` imperative noise and subject-level supersede. A temporal
  probe answered "before" as `蓝鲸` and refused to invent a change time
  (`fact_history.changed_at` is stored but not in the prompt). Report:
  `evals/reports/2026-11-01-memory-multi-session.md`.
- `docs/llmwiki/frontend-contract.md` fixes the read path: canonical queries,
  the current-state view, and the rule that a frontend never parses the resume
  snapshot or writes to the database. It names the three answer layers (turn
  answer, segment record, session memo) and the two overwritten axes (facts and
  task status; `fact_history` and `todo_history` are not projected yet).
- Verification: 255 tests; fmt and clippy clean.

### Fact and task history

- `fact_history` versions every fact with `added`, `superseded` or
  `forgotten` (and `active = 0` for the last one). `todo_history` stores the
  full task list for every revision that changed it.
- The projection diffs the previous rows before replacing them, so the history
  tables see the values that are about to be overwritten. A database migrated
  from an earlier slice backfills version 1 on its next save; the live memory
  database now has 9 facts and 9 history rows.
- `frontend-contract.md` documents the transition queries.
- Verification: 259 tests; fmt and clippy clean.

### Context snapshots

- `ContextSnapshot` (estimate, budget, stages, messages) is carried on
  `TurnOutcome` when `KernelOptions.capture_context` is set; the CLI flag is
  `--context-snapshots`. Off by default: the snapshot is large and only a host
  that persists it needs the copy. Message content is redacted.
- The store writes one row per turn in `context_snapshots`: turn id, session,
  estimate, budget, stages JSON, messages JSON. It is the turn-start assembly,
  not the transcript and not each tool round.
- `frontend-contract.md` and ADR 0008 describe the table; per-stage counts are
  deliberately not carried.
- Verification: 261 tests; fmt and clippy clean.

### Fusion run completed at 24k

- The full 57-turn novel finished on DeepSeek with the 24k profile, per-turn
  acceptance, a 300 s turn deadline and two retries available: 57/57 Done,
  0 Error, 0 retries, 50/50 chapters with no gaps, 50/50 tasks closed in order,
  57/57 acceptance passes, 0 overflow, context peak 15,853 / 23,739.
- Recall fired once in 57 turns, on the chapter-8 memory probe, with 2 hits and
  907 tokens; probes 55 and 56 came from the memo without recall. The narrowed
  gate kept 50 chapter briefs from pulling old segments.
- The memo fallback fired on 22 of 56 revisions (20 empty, 2 short) and kept
  the pipeline alive; probe answers still passed. A 39% fallback rate is the
  next quality target.
- 4,485,612 input and 337,820 output tokens over 43 minutes; 78.9% weighted
  cache hit (per-turn median 78.0%); about ¥2.37 at off-peak prices.
- Report: `evals/reports/2026-11-01-fusion-run.md`.
- Verification: 247 tests; fmt and clippy clean.

### MCP plugins

- ADR 0013 fixes the plugin shape: a plugin is an external process, not a
  linked library and not a reviewed manifest. `plugins.toml` lists MCP stdio
  servers; the host spawns each, performs the handshake, lists its tools and
  registers them as ordinary kernel tools (`mcp_<server>_<tool>`). Any
  allowlisted CLI stays reachable through the shell tool.
- Flat schema properties become typed parameters. Nested objects and object
  arrays switch the whole tool to a single JSON `arguments` string; a partly
  mapped flat schema would silently drop fields. The memory server exposed
  this: its `entities` array of objects was first mapped to a string list and
  the kernel rejected the call as `invalid_response`.
- Live: the official filesystem server listed a directory, and the memory
  server stored and found an entity, both with no kernel change. Windows runs
  `npx` through `cmd /c`.
- Remote transport: `url = "https://.../mcp"` speaks streamable HTTP, accepts
  a JSON or SSE response body and echoes `Mcp-Session-Id`. Verified against a
  local mock HTTP MCP server. A pure SSE endpoint (`GET /sse`) is not
  implemented.
- Ecosystem check: DSH (DeepSeek Harness) plugins are Cordis TypeScript modules
  bound to that runtime, not MCP; Dify and Coze plugins target their own
  runtimes. Domestic MCP markets (ModelScope, Bailian, Tencent) are hosted
  remote MCP; a live test needs a streamable-HTTP deployment URL and, for most
  services, a key. A DSH bridge or Dify/Coze adapter is a separate integration.
- Remaining: hot reload, server-initiated requests, deeper schema mapping.
  Report: `evals/reports/2026-11-01-mcp-plugins.md`; task doc:
  `tasks/m4-plugins.md`.
- Verification: 268 tests; fmt and clippy clean.

### Store fault injection

- A save that fails after the session and turn rows were written rolls back
  the whole transaction; the previous snapshot, the latest turn and the
  `session_current` row stay unchanged. The test injects the failure with a
  SQL trigger that rejects the fact insert, which runs near the end of
  `project`, so the rollback covers both the transcript and the read model.
- A hand-corrupted `sessions.snapshot_json` now has a test: `load` returns
  `KernelError::InvalidConfig` with the decode error, and startup propagates
  it instead of panicking. A corrupt task list takes the same path.
- Validation matrix updated: the M4 row reflects the MCP tests and the two
  servers; the persistence row lists fault injection. The domestic hosted MCP
  test and a DSH bridge are marked deferred.
- Verification: 270 tests; fmt and clippy clean.

### Fact updates and reply instructions

- An explicit update cue ("改成", "改为", "换成", "changed to", "renamed to",
  "is now", ...) now names a subject: the normalized text before the cue. Every
  fact whose key starts with that subject is removed before the new statement
  is stored, so `项目代号是蓝鲸` followed by `项目代号改成海鸥` leaves one
  current fact. The host still sees both versions through the snapshot diff and
  `fact_history`; the memo keeps the narrative order for temporal questions.
- Sentences that ask for an answer now ("直接回答", "just answer", "告诉我",
  ...) are no longer constraints even when they open with `不要`. The
  multi-session evaluation recorded four of these as workspace facts; the
  question, not the user's preference, was being stored.
- Both rules are kernel-side heuristics over the existing lexical extractor:
  no prompt change, no model call. Verify: two new unit tests; 272 tests, fmt,
  clippy and deny clean.
- Rerun of the multi-session evaluation (11 turns, three processes on one
  store): 11/11 Done, 8/8 answers, 0 overflow; 3 current facts, all real, and
  0 reply instructions; "before" answered 蓝鲸 from the memo, "when" refused.
  The store records the replaced fact as `forgotten` because it diffs keys;
  both versions remain in `fact_history`.
- Validation matrix: knowledge updates now list subject supersede; the
  extraction gap is narrowed to other imperative phrasing. Report:
  `evals/reports/2026-11-01-memory-multi-session.md`.

### Branching and session deletion

- Every saved turn now writes a `session_checkpoints` row with the post-turn
  snapshot and task list, in the same transaction as the turn. A branch restores
  that state instead of replaying the transcript, which would rewrite a memo the
  model wrote and change when facts were extracted.
- `--fork-from <session>:<turn>` creates the branch under `--session-id`, copies
  turns `1..turn`, rebuilds the projection, and records `parent_session_id` and
  `fork_turn_seq` on the new session. A branch is an ordinary session; there is
  no branch table.
- `--delete-session <id>` removes every row of one session in one transaction.
  Branches are independent and survive the parent. Deleting the
  `session_current` view would delete nothing; it is derived.
- Schema version 8. Verify: checkpoint-per-turn, fork restore, fork errors,
  delete rows, branch survival, plus a two-process mock smoke and a live
  branch delete. 278 tests; report:
  `evals/reports/2026-11-01-branching-and-plugin-check.md`.

### Plugin probe and restart

- `--plugins-check --plugins <file>` starts every server, runs `initialize` and
  `tools/list`, prints one line per server and exits non-zero when any fails.
  This is the gate a configuration page calls before saving; a changed file
  still needs a host restart.
- In-process hot reload is rejected: the tool registry is fixed for a turn, and
  a mid-turn swap has no correctness story. Restart is cheap because session
  state is in SQLite.
- MCP servers are killed as a process tree (`taskkill /T /F` on Windows, the
  process group on Unix). The direct child is often `cmd /c npx`, so killing it
  alone left the runtime alive and holding files and ports, which is the usual
  cause of "restart fails for no reason".
- Third server family: a Python stdlib SQLite MCP server
  (`evals/harness/fixtures/mcp_sqlite_server.py`) passed probe, initialize,
  tools/list and create/insert/select. The test skips when Python is absent.
- Verify: wrapped (`cmd /c python`) and direct servers reported ready, a broken
  command reported failed with exit 1, and both server PIDs were gone after the
  probe. 278 tests; fmt, clippy and deny clean.

### Plugin protocol depth

- The JSON fallback description for nested MCP schemas no longer clips the raw
  schema. It now lists each field with its type and required mark, recurses
  into object arrays (`entities[].name`), and appends a minimal example,
  bounded at 900 characters. The model still passes one `arguments` string;
  only the description changed.
- A stdio server may now send requests to the client. `ping` is acknowledged,
  `roots/list` returns the workspace as a `file://` URI, and other methods
  (including sampling) get a JSON-RPC `-32601` error instead of silence. The
  roots capability is advertised during initialize with `listChanged: false`.
- Streamable-HTTP responses still drop server-initiated requests; only stdio
  answers them. Sampling remains refused by design: it spends tokens and sends
  context outside the kernel's permission and audit path.
- Verify: `a_nested_schema_is_described_with_fields_and_an_example`,
  `server_requests_are_answered_by_method`, and the Python fixture end to end
  (it asks for roots during initialize and `roots_seen` returns the same URI).

### Checkpoint retention

- `--keep-checkpoints N` prunes a session's checkpoints to the newest N inside
  the turn transaction; the default `0` keeps every turn. Turns referenced by
  an existing branch are pinned: `fork_turn_seq` protects them from pruning.
- Verify: `old_checkpoints_are_pruned_but_fork_points_are_kept` keeps
  `[1, 4, 5]` (newest two plus the fork point) and a fork from a pruned turn
  fails with a typed error; a three-turn CLI smoke with `--keep-checkpoints 2`
  left `[2, 3]` and refused a fork from turn 1.

### Plugin status as JSON

- `--plugins-check --json` prints the probes as
  `[{name, tools, error, duration_ms}]` for a configuration page, with the same
  exit-code rule: non-zero when any server failed. `--json` without
  `--plugins-check` is rejected.
- Verify: `a_server_status_serializes_for_a_config_page` and a live two-server
  probe that printed one ready entry and one failed entry.

### Plugin configuration contract

- ADR 0013 now records the implementation: one JSON argument for nested
  schemas with a generated description, stdio answers for `ping`/`roots/list`
  and refusal for sampling, the restart-based configuration flow with the
  `--plugins-check` gate, and process-tree cleanup on drop.
- Live probe: the official memory server listed 9 tools in 2.2 s, and a mock
  turn loaded all of them through `mcp::load` without a kernel change.

### HTTP server requests and resumed turn numbers

- Streamable HTTP now answers server-initiated requests. A response body (JSON,
  JSON array or SSE) is split into the reply matching the client's id and any
  `method`+`id` messages; each server request is answered with a POST to the
  same endpoint. Sampling is still refused.
- The mock HTTP server emits `roots/list` beside the `tools/list` response and
  records the client's roots reply; the test asserts the workspace URI comes
  back. Streamable HTTP therefore covers the same request set as stdio.
- The session loop prints the persisted session turn (`next_turn_seq`) instead
  of the driver's per-process id. A resumed session continues at `turn: 3`, and
  the first new turn on a branch forked at turn 1 prints `turn: 2`.
- Verify: 282 tests; fmt, clippy and deny clean.

### Coding suite run

- Full `deepseek-flash` suite: 43/46 cases, 302,068 tokens, 82.4% weighted
  cache hit, about ¥0.36 at peak prices. Curated coding 10/10, HumanEval+ 20/20,
  MBPP+ 13/16; four MBPP cases skipped at the 300k budget. Report:
  `evals/reports/2026-11-01-coding-suite.md`.
- The two reproducible failures are benchmark boundaries: one lowercase word
  counts as valid in MBPP 16, and `True` counts as `1` in MBPP 20. MBPP 3 failed
  once with a test-runner error and passed on rerun.
- `run_case.py` was still matching the pre-cache usage line, so reports carried
  `usage: null`. It now parses calls/input/output/cached/hit/total and stores
  `cached_tokens`. Verified with mock runs and the suite.

### Composed frontend views

- `--view sessions` and `--view session --session-id <id>` print the assembled
  screens as JSON: session summary, memo with covers, facts, tasks, branches and
  decoded turns (`answer_model`, `usage`). The host performs joins, ordering and
  JSON decoding, so a frontend renders instead of deriving.
- Ordering is explicit everywhere (updated_at/id, scope/key, position, seq), and
  the same store serializes identically twice. Missing sessions exit non-zero.
- Tests in `hosts/keel-cli/src/view.rs` cover the list, both the parent and the
  branch view, every panel, repeat output, and the missing-session case; a CLI
  smoke ran both views against the memory store.
- Task doc: `docs/llmwiki/tasks/frontend-views.md` tracks the remaining
  composed panels (turn detail with decoded context snapshots, typed memory
  timeline, long-session limits, golden files) and the rule that a screen
  field lands with its view and test.

### Frontend views: turn, memory, records

- `--view session` now includes `records`: every sealed segment with goal,
  result, decoded changes/pending, verification and fold/absorption state,
  ordered by segment sequence.
- `--view turn --session-id <id> --turn <n>` returns one turn with the decoded
  structured answer, usage and the captured context snapshot (stages and
  messages). A missing turn exits non-zero.
- `--view memory --session-id <id>` returns the typed timeline: every
  `fact_history` row with `added`/`superseded`/`forgotten` and `active`, and
  every `todo_history` revision with decoded items. A missing session exits
  non-zero.
- The session view has a golden file (`hosts/keel-cli/tests/fixtures/
  session-view.golden.json`, timestamps normalized), so a field rename or an
  ordering change fails a test rather than reaching the UI.
- Verify: 289 tests; CLI smoke of turn and memory views against the memory
  store. fmt, clippy and deny clean.

### Turn pagination for long sessions

- `--view session` accepts `--limit N` (0 = all) and `--before <seq>`
  (exclusive cursor). It reads the newest N turns and returns them ascending,
  so the newest page still reads top to bottom; the next page passes the
  smallest `seq` received. The session summary keeps the total turn count.
- The cursor is only valid with `--view session`; using it elsewhere is an
  error. The golden file is unaffected because its test uses the default
  window.
- Verify: `a_turn_window_pages_backward_in_ascending_order` (all, newest one,
  older page, empty page) and a CLI smoke of pages 9-11 then 7-8 on the memory
  store.

### View contract through the real binary

- `hosts/keel-cli/tests/cli_views.rs` runs the compiled binary against a
  temporary store: it seeds two turns with the mock profile, then asserts
  `sessions`, `session`, two paged `session` calls, `turn`, `memory`, fork,
  branch metadata, delete, and the non-zero exits for a missing session and a
  cursor on the wrong view. It also parses `--plugins-check --json`.
- This is the frontend contract at the same boundary a UI uses, so a frontend
  test can reuse the commands without starting the app.
- Verify: 294 tests; fmt, clippy and deny clean.

### Five more curated coding cases

- Added `task-011-csv`, `task-012-lru`, `task-013-toc`, `task-014-semver` and
  `task-015-wrap` through `evals/harness/scaffold_cases.py`. Each was validated
  as setup-fails and reference-passes before any model run.
- Curated suite: 14/15 passed, 189,069 tokens, 84.9% cache hit, about ¥0.26.
  `task-013-toc` needed one prompt clarification (empty heading ignored) and
  passed on rerun; `task-008-greeting` failed once with `state=Error` and no
  final answer while its workspace tests passed, matching the model-variance
  category from the first suite. Report: `2026-11-01-coding-suite.md`.
- The curated set is now 15 of the M6 target of 20.

### Curated set reaches twenty cases

- Added `task-016-query`, `task-017-ranges`, `task-018-fsm`, `task-019-words`
  and `task-020-duration`, each validated as setup-fails and reference-passes
  before the model ran.
- Full curated suite: **20/20 passed**, 286,042 tokens, 85.6% cache hit, about
  ¥0.42; the slowest case took 37 s. `task-008-greeting` passed in this run,
  confirming the earlier failure as model variance.
- M6 now has the twenty coding cases; the companion personas, reproducible
  report and license audit remain. Report: `2026-11-01-coding-suite.md`.

### Companion persona evaluation

- Five personas with dedicated prompts and profiles: archivist, coach, pirate,
  poet and engineer (`variants/companion/personas/`, `profile.<name>.toml`).
  Each prompt states measurable surface rules and the base prohibitions.
- `evals/harness/run_companions.py` runs one process per persona for twenty
  turns on one store with per-turn acceptance, then reads `turns.answer_json`
  from SQLite. It checks persona rules every turn and three durable facts in
  turns 17-19, prints per-persona failures and writes a JSON report.
- Results: 5/5 personas, 100/100 turns `Done`, all rule counters 20/20, 15/15
  memory probes, 231,519 tokens. Report: `2026-11-01-companion-personas.md`.
- The first run flagged an engineer memory miss for `周二 20:00`; the checker
  required the literal “八点”. It now accepts equivalent time spellings and
  the rerun passed. This normalised the checker, not the memory bar.
- Verify: `python -m py_compile evals/harness/*.py`; the companion run above.

### Architecture, case study and license audit

- Added `docs/architecture.md`: layers, one turn end to end, bounded context,
  memory, tools/permissions, storage and branching, plugins, and the list of
  concerns deliberately kept outside the kernel.
- Added `docs/case-study.md`: the 50-turn fusion, 20-case coding suite,
  companion runs, memory and plugin evidence in one place, with the cost model
  and a failure analysis (16k negative result, empty summaries, fact noise,
  tool headroom, benchmark boundaries, turn closure).
- Added `docs/license-audit.md`: Apache-2.0 project license, `cargo deny`
  policy and direct dependency licenses, dataset provenance, external plugins,
  asset inventory and a pre-release checklist.
- README refreshed to the current state: implemented features, store/branch/
  view commands, plugin configuration, evals and the new documents.

### Kernel distribution and snapshot versioning

- ADR 0014 fixes the repo shape: `crates/` is the distributable kernel;
  `hosts/keel-cli` is a reference host and executable contract test, not the
  product. Derived projects depend on the crates and bring their own host.
- `docs/kernel-contract.md` lists the stable surface, extension points,
  contract tests, versioning/deprecation rules, snapshot migration and how a
  derived project consumes and updates the kernel. `CHANGELOG.md` records the
  0.1.0 kernel and host.
- `SessionSnapshot` now carries `format_version` (`SNAPSHOT_VERSION = 1`);
  `Session::restore` returns a typed `Unsupported` error for a newer format
  instead of reading it as if the fields matched. The test
  `a_snapshot_from_a_newer_format_is_rejected` covers it.
- CI gains a kernel-only job (`cargo test -p keel-core -p keel-policy
  -p keel-runtime -p keel-model-openai`), so a host change cannot hide a
  kernel dependency.
- Verify: kernel-only build/tests (251 tests) and the full workspace
  (295 tests); fmt and clippy clean.

### Permission decisions as events

- `Event::PermissionDecided` records every gate decision (`allow`, `deny`,
  `confirm`) with the tool, access kind, path and reason. The dispatcher emits
  one per permission request, so a UI or audit log no longer has to infer a
  decision from the tool outcome.
- `docs/kernel-contract.md` now describes the event feed as the UI/audit
  contract and states the streaming rule: the kernel does not emit per-token
  events; a host that wants live tokens wraps its `ModelProvider`.
- Verify: `permission_decisions_are_recorded_as_events` covers allow, confirm
  and deny; 296 workspace tests, 252 kernel-only.

### Event-order contract and minimal embedding

- `a_tool_turn_emits_the_host_contract_events_in_order` pins the event sequence
  a host may rely on: `turn_started` first, `turn_report` last, with
  `model_started` → `permission_decided` → `tool_started` → `tool_finished` →
  `turn_finished` in between.
- `crates/keel-runtime/examples/minimal_turn.rs` is a host in one file: a
  profile string, two context stages, one tool, a mock model, a driver. It runs
  a turn and prints the state, answer and event count; no CLI involved.
- `docs/kernel-contract.md` and README point to the example and state that a
  path dependency is enough for personal use; publishing the kernel is
  optional.
- Verify: 297 workspace tests, 253 kernel-only, `cargo run -p keel-runtime
  --example minimal_turn`.

### Persisted kernel events and turn errors

- `Event` and the field types it carries implement `Serialize`; variant names
  are snake_case, so the persisted payload and the event name agree. The hosts
  no longer need their own event schema.
- The SQLite store (schema 9) adds `events` (one row per event, in emission
  order, in the turn transaction) and `turns.error` (kernel error text and
  code). Re-saving a turn replaces its event stream; a branch copies the events
  of the turns it copies; deleting a session removes them.
- `--view events --session-id <id>` pages the stream by turns with
  `--limit`/`--before`; `--view turn` now includes `events` and `turn.error`.
- Verify: `events_serialize_for_persistence`,
  `events_and_errors_are_stored_with_the_turn`,
  `a_failed_turn_exposes_its_error_text`,
  `the_events_view_pages_with_the_turn_window`, the CLI events assertion, and
  301 workspace tests (254 kernel-only).

### Performance gates

- Added release-only ignored tests in
  `crates/keel-runtime/tests/performance.rs`: turn overhead (p50 < 5 ms,
  p99 < 20 ms), 10k-message context assembly (< 20 ms), tool dispatch (< 1 ms),
  memory recall over 200 segments (< 50 ms), event enqueue (< 1 ms) and a
  500-turn stress run.
- Measured the cost of event recording: serialize 50 events 0.003 ms; save a
  turn with 50 events 1.602 ms p50 vs 1.232 ms without (about 0.37 ms); the
  kernel never emits per-token events, so the stream is per boundary.
- Results: turn 0.015/0.060 ms, context 1.835/3.197 ms, dispatch
  0.004/0.020 ms, recall 0.090/0.169 ms; 500 turns in 13.8 ms. Report:
  `evals/reports/2026-11-01-kernel-performance.md`; task doc:
  `docs/llmwiki/tasks/performance.md`.

### Resource bounds for tool execution

- `command::run` now drains stdout and stderr while the process runs and keeps
  at most 64 KiB per stream: a 48 KiB head and a 16 KiB tail, with the omitted
  byte count in between. A command that prints without end can no longer fill
  memory before the timeout.
- `TreeGuard` kills the whole process tree when the command is cancelled or
  times out. Windows uses `taskkill /T /F`; Unix runs the child in its own
  process group and signals the group. A completed command is not killed, so a
  program that detaches on purpose still leaves its descendants.
- `read_file` checks the file size first and refuses anything over 8 MiB with
  `InvalidArguments`, instead of reading it and returning a tool error later.
- `ShellTool` takes its timeout from the host instead of a hard-coded 60 s that
  the 30 s dispatcher timeout always beat. `--tool-timeout-ms` overrides the
  dispatcher limit for long builds; `--turn-timeout-ms` still caps the turn.
- Verify: `a_capture_keeps_a_bounded_head_and_tail`,
  `a_large_output_is_truncated_and_bounded`,
  `a_timeout_takes_the_descendants_with_it`,
  `read_file_refuses_a_file_over_the_limit`, `shell_stops_at_its_own_timeout`,
  and 309 workspace tests (262 kernel-only). Task doc:
  `docs/llmwiki/tasks/resource-safety.md`.

### Command shims and explicit undo

- Windows: when a bare program name is not found, `command::run` retries the
  same arguments through a `.cmd`/`.bat` shim on PATH. `npm`, `npx`, `pnpm`
  and `yarn` are batch files, so the allowlisted names previously failed with
  "program not found"; `git`, `cargo` and other `.exe` tools were unaffected.
- The trash records the original path in `.keel/trash/meta/<id>.toml`.
  `TrashStore` gains `list` and `find` with defaults, so a backend written
  before this change keeps compiling and reports that it cannot list.
- New tools: `list_trash` (newest first, id and original path) and
  `restore_file` (`path` for the newest backup, `entry` for one id). The
  target comes from the tool argument, so the permission gate approves the
  file that is actually written; an entry is accepted only for its recorded
  path. The current version is backed up before the copy, so undo is
  reversible.
- Coding profiles expose both tools; `profile.eval.toml` and the novel profiles
  keep their tool lists for comparability with earlier runs.
- No shell is added. Pipes stay unavailable; `python`/`node` scripts remain the
  audited path, and a `bash` tool would be a host-level, opt-in decision.
- Verify: `a_windows_command_shim_is_resolved`,
  `backups_are_listed_newest_first_and_found_by_id`,
  `a_trash_id_cannot_escape_the_trash_directory`,
  `restore_file_restores_the_newest_backup_and_keeps_the_current_version`,
  `restore_file_rejects_an_entry_for_another_path`,
  `restore_file_reports_a_missing_backup`,
  `list_trash_names_the_backups_and_their_paths`, profile validation, and a
  CLI smoke run. Task doc: `docs/llmwiki/tasks/command-and-undo.md`.

### Long-session retention and MCP message caps

- `EventLog` keeps a monotonic sequence counter and gains `clear()`; the
  reference host clears the in-memory stream after each turn is printed and
  saved. A five-turn mock session now prints `events: 14` every turn instead
  of a growing count, and the SQLite `events` table still holds both turns.
- The store keeps the 20 most recent checkpoints per session by default;
  `--keep-checkpoints 0` keeps all, and a checkpoint referenced by a fork is
  never pruned. Twenty full snapshots per session bound the write cost that
  previously grew with the square of the session length.
- MCP stdio messages are read with a bounded `fill_buf`/`consume` loop instead
  of `Lines`, and HTTP bodies are read in chunks under the request timeout;
  either over 8 MiB is reported as an error rather than buffered.
- Verify: `clear_drops_records_and_keeps_sequence_numbers`,
  `a_session_persists_every_turn_after_the_event_log_is_cleared`,
  `the_default_checkpoint_cap_bounds_the_history`,
  `a_stdio_message_is_read_up_to_its_newline`,
  `an_oversized_stdio_message_is_refused`,
  `an_oversized_http_body_is_refused`, and 322 workspace tests (270
  kernel-only). Task doc: `docs/llmwiki/tasks/resource-safety.md`.

### Interactive terminal host

- Added `--tui`: a ratatui/crossterm front end for the reference host with a
  streaming transcript, tool status lines, a permission modal, slash commands,
  Ctrl-C cancellation and session persistence. The kernel is untouched; the UI
  lives under `hosts/keel-cli` and is not a kernel dependency.
- Streaming uses a `HostModel::Tui` decorator that forwards `TextDelta` to the
  UI and passes the same chunk to the driver, so the kernel never learns about
  the TUI. Tool progress drains `EventLog` by sequence; interactions cross a
  channel-backed `InteractionPort` and reuse the line UI's `parse_response`.
- The frontend is tested headlessly: the reducer has unit tests (CJK
  backspace, history, slash parsing, modal scopes, cancel/quit), and the
  renderer is exercised through `TestBackend` with goldens for the empty
  screen, streaming and the permission modal. A piped stdin exits with a
  clear error instead of entering raw mode.
- Slash commands are `/help`, `/clear`, `/cost`, `/sessions`, `/cancel` and
  `/quit`. Session switching is by `--store` + `--session-id`; `/resume` is
  not part of the simplified UI.
- Verify: `the_tui_requires_an_interactive_terminal` and the 11 `tui::tests`
  reducer/render tests; 334 workspace tests (270 kernel-only). Task doc:
  `docs/llmwiki/tasks/tui.md`. A real-terminal smoke run is still manual.

### Result-only turn view

- A finished turn now hides its process: the streamed assistant text, tool
  lines and in-turn notices are dropped, and the transcript keeps the user
  message, the final answer and one aggregate line
  (`1 tool · 1 file · 15 in / 5 out · 12 ms`). The running turn still shows
  live text and tool status; the collapse happens when the outcome arrives.
- The permission prompt moved inline above the input instead of a centered
  modal, matching the conversation flow.
- New goldens: `tui-result.txt`, `tui-prompt.txt`; `tui-empty.txt` and
  `tui-streaming.txt` updated for the card prefixes (`›`, `⏺`, `⎿`) and the
  usage segment in the status line.
- Verify: `a_finished_turn_hides_its_process`,
  `a_finished_turn_renders_only_the_result_and_its_fields`,
  `the_permission_prompt_is_inline`, and 336 workspace tests (270
  kernel-only). Task doc: `docs/llmwiki/tasks/tui.md`.

### Paced mock demo

- `--demo` (mock provider only) makes the TUI demonstrable without a model
  key: a preamble is streamed, a `shell` call asks for permission, runs
  `python -c "import time; time.sleep(2)"`, and the answer is streamed in
  six-character pieces before the turn collapses to its result.
- `DemoModel` alternates a tool-call response and an answer response, so
  every turn shows both phases. A host-side `Paced` wrapper delays chunks and
  is only used with `--demo`; it forwards the same chunks to the driver, so
  the kernel is untouched.
- Verify: `text_chunks_split_on_char_boundaries`,
  `the_demo_script_runs_a_sleeping_shell_tool`, and 338 workspace tests (270
  kernel-only). The demo needs a terminal, so the end-to-end check is manual.
  Task doc: `docs/llmwiki/tasks/tui.md`.

### Selectable permission prompt and mode switch

- The inline permission prompt is now a vertical option list: `↑`/`↓` moves,
  `Enter` confirms, the default selection is `Allow once`, and `d`/`a`/`s`/`f`
  stay as shortcuts. Prompt text is split on its newlines and the panel height
  accounts for wrapped lines, so the command no longer runs into the reason.
- `Tab` and `Shift+Tab` toggle the session between `workspace_auto` and
  `full_auto`; the header is the single mode indicator. The driver gained
  `escalate_full_auto(ttl)` and `revoke_permission_grants()` for hosts that
  offer the switch; the same grant path and expiry still apply.
- Verify: `arrows_select_a_scope_and_enter_confirms_it`,
  `tab_requests_a_mode_toggle`, `the_permission_prompt_is_inline`, updated
  goldens, and 340 workspace tests (270 kernel-only). Task doc:
  `docs/llmwiki/tasks/tui.md`.

### Scrollable history, expandable process and session mode

- The transcript keeps every turn. A finished turn's process (streamed text,
  tool lines, notices) is stored as hidden blocks instead of being deleted;
  `Ctrl-O` shows or hides those blocks for the whole history, and the footer
  shows the hint only when something is hidden.
- Scrolling no longer jumps to the top on the first `PgUp`: it starts from the
  current bottom, and a scrollbar appears when the transcript overflows.
- `Tab` is the single mode key; `Shift+Tab` is not bound. The mode uses a
  session-long TTL (30 days, cleared on exit) and is shown only in the header,
  with no notice and no duplicate interaction line. The permission prompt's
  Full auto option uses the same TTL.
- The footer is two columns (status/usage left, keys right) so it fits an
  80-column terminal without truncation.
- Verify: `a_finished_turn_hides_its_process`,
  `ctrl_o_toggles_process_details`,
  `page_up_starts_at_the_bottom_and_page_down_returns_to_it`,
  `tab_requests_a_mode_toggle`, updated goldens, and 342 workspace tests (270
  kernel-only). Task doc: `docs/llmwiki/tasks/tui.md`.

### Claude-style layout and Shift+Tab

- `Shift+Tab` is the mode key again, matching Claude Code; `Tab` is unbound.
- Removed the top header: the workspace, model and permission mode now live in
  the bottom status line, with key hints right-aligned on the same row. The
  transcript starts at the top of the screen, the input box uses rounded
  borders and no title, and the permission panel is rounded too.
- This is an observable-behavior match only. The clean-room rule forbids
  reading or copying another repository, so no opencode or Claude Code source
  was used; the layout follows the documented interaction and the user's
  screenshots.
- Verify: `shift_tab_requests_a_mode_toggle`, updated goldens, and 342
  workspace tests (270 kernel-only). Task doc: `docs/llmwiki/tasks/tui.md`.

## 2026-11-01 — Tool output preview and first-run workspace

- `ToolFinished` carries a bounded, redacted `output_preview` for a live UI.
  Serde skips the field, so tool output never reaches the persisted event
  stream; the event log's excluded-output invariant still holds on disk.
- The TUI shows the head and tail of a running tool's output and the whole
  preview under `Ctrl-O`; a finished turn hides the preview with the rest of
  its process.
- `examples/demo-workspace` is a three-test Python module with one failing test
  and a README, used by the first-run walkthrough in the repository README.
- Verify: `a_preview_is_redacted_and_bounded`,
  `a_tool_output_preview_is_not_persisted`,
  `a_running_tool_shows_a_short_output_excerpt`, and the existing suite.

### Tool contract hardening

- MCP `number` properties now switch the whole tool to the single JSON
  `arguments` mode instead of being flattened to an integer. The kernel models
  integers only, so a fractional value would have been silently dropped (or
  rejected at the wire).
- The dispatcher rejects a tool declared `read_only` that requests write or
  execute access, before the permission gate and before `call`. Read-only
  calls run concurrently, so the dispatcher cannot trust the label alone.
- Verify: `a_fractional_number_switches_to_json_mode`,
  `a_read_only_tool_may_not_request_write_access`, and the full suite.
  Task doc: `docs/llmwiki/tasks/tool-contract-hardening.md`.

### Sandbox boundary and file tools

- The dispatcher canonicalizes every declared path before the permission gate.
  A symlink inside the workspace can no longer redirect a read or write
  outside it; a missing write target is resolved through its deepest existing
  ancestor, and a dangling symlink is refused.
- `..` no longer climbs above the drive or filesystem root. The old
  normalization could turn an escape into a relative path that later resolved
  against the process cwd.
- `.keel/outputs` is readable so truncated tool output can be re-read. The
  rest of `.keel` stays denied and the exception is read-only.
- MCP servers spawn with the workspace as their working directory.
- New built-in tools `make_dir`, `move_file` and `delete_file`; delete copies
  to the trash first, move backs up an existing destination.
- Verify: the symlink, normalization and spill tests, `file_tool_tests`, the
  full workspace suite and a terminal smoke run.

### Extra roots

- `PermissionEngine::with_root(path, access)` adds a `RootScope` outside the
  workspace. Read-only scopes allow reads and confirm writes; read-write
  scopes allow both. The most specific root wins, execution stays
  confirm-only, and a drive/filesystem root or system directory is refused.
- `--root PATH[:ro|:rw]` is repeatable; `/roots` lists every scope in the
  terminal UI.
- MCP `roots/list` advertises every scope and the server still starts in the
  primary workspace.
- Real two-root smoke: read from a `:ro` root, wrote in the workspace, four
  calls, 4.2 s, 83.7% cache hit. Task doc:
  `docs/llmwiki/tasks/multi-root.md`.

### Bundle and state directory

- `--bundle PATH` loads a composition file with `profile`, optional `plugins`
  and optional `state_dir`; paths resolve relative to the bundle. It is
  mutually exclusive with `--profile` and `--plugins`.
- `--state-dir PATH` (or a bundle's `state_dir`) moves the SQLite store to
  `<state>/keel.db` and the trash to `<state>/trash`. `--store` still wins.
  `LocalTrashStore::at` takes the trash directory directly.
- Reference bundles exist for coding and companion. A mock bundle run from a
  scratch directory loaded the profile and created the state database; the
  workspace stayed empty.
- Truncated output still spills to `<workspace>/.keel/outputs`; moving it is
  future work. Task doc: `docs/llmwiki/tasks/bundle-and-state.md`.

### Job file

- `--job PATH` binds a bundle/profile, workspace, extra roots, state
  directory, session id and the first input into one reusable task. Every
  path resolves relative to the job file; unknown fields and access values are
  rejected.
- Explicit CLI composition wins as a whole; `--workspace`, `--input`,
  `--session-id`, `--state-dir` and `--store` override the job's fields, and
  CLI `--root` entries are added to the job's roots.
- Real smoke: a job referenced a bundle, set a workspace, session and a
  read-only root; the run loaded the profile, created `state/keel.db`, and
  `--view sessions` showed the job's session. Task doc:
  `docs/llmwiki/tasks/job-file.md`.

### Trash moves on the same volume

- `TrashStore::discard` removes a file while keeping it recoverable. The
  default backs up and removes; `LocalTrashStore` renames the file into the
  trash when it is on the same volume and falls back to copy plus remove
  across volumes.
- `delete_file` uses `discard`, so deleting a large file no longer doubles
  disk usage. The message names the trash entry instead of a "backup".
- Verify: `discard_moves_the_file_and_keeps_it_restorable`,
  `delete_file_moves_the_file_into_the_trash`, and the full suite.

### Outputs in the state directory

- `summarize_output` takes the outputs directory and writes an absolute path
  into the truncation marker. `TurnDriver::with_outputs_dir` and
  `PermissionEngine::with_outputs` let the host place and allow that directory.
- With `--state-dir` (or a bundle's `state_dir`), spills go to
  `<state>/outputs`; the workspace keeps no `.keel` directory. Without a state
  directory the default stays `<workspace>/.keel/outputs`.
- The read exception is limited to the configured outputs directory and only
  permits reads.
- Smoke: a 5000-character echo produced a 5000-byte spill in the state outputs
  directory, the store lived in the state directory, and the workspace stayed
  empty. Task doc: `docs/llmwiki/tasks/state-outputs.md`.

### Spill retention

- The outputs directory now keeps the newest 200 `turn-*-call-*.txt` files;
  older ones are pruned after each write, best effort. Unrelated files are
  left alone.
- A marker that points at a pruned spill returns `NotFound`; the model can
  re-run the tool instead of reading a vanished file.
- Verify: `old_spills_are_pruned_and_other_files_are_kept` and the full suite.

### TUI job prefill

- `--tui --job` (and `--tui --input`) place the input in the prompt without
  starting a turn; the user still presses Enter. The default "hello from keel"
  is never prefilled because it is not an explicit input.
- Verify: `prefill_fills_the_prompt_without_starting_a_turn` and
  `a_prefilled_prompt_is_visible_in_the_input_box`.

### Default state directory

- With no `--state-dir` and no bundle `state_dir`, the state root is
  `KEEL_HOME`, then `~/.keel` (`%USERPROFILE%\.keel` on Windows). The store,
  trash and output spills live under it; a plain run now persists its session.
- An explicit `--state-dir` or bundle `state_dir` overrides the default, and
  `--store` overrides only the database. The reference bundles dropped their
  repo-local state path and use the default.
- The eval harness sets `KEEL_HOME` to a per-case temporary directory, so
  eval runs stay out of the developer's home.
- Verify: the three `state_home` tests plus two smoke runs (KEEL_HOME store;
  explicit state dir overriding it). Task doc:
  `docs/llmwiki/tasks/default-state-dir.md`.

### Release basics

- `--help`/`-h` prints a usage reference and `--version`/`-V` prints the host
  version before parsing, so both work without a profile or valid flags.
- CI now runs format, lint and the full test suite on Ubuntu, Windows and
  macOS; a separate Linux job keeps `cargo deny`. The kernel job runs the
  kernel-only tests and the `minimal_turn` example on every platform.
- Verify: `help_and_version_are_detected_before_parsing`, direct
  `--help`/`--version` runs, and the full workspace suite. Task doc:
  `docs/llmwiki/tasks/release-basics.md`.

### Anthropic provider

- New `keel-model-anthropic` crate: Messages API request mapping, streaming
  text/thinking/tool-use decoding, usage, control-tool interactions, retries
  and cancellation.
- The host selects it with `provider = "anthropic-compatible"`;
  `variants/coding/profile.anthropic.toml` targets `claude-sonnet-5-5` and
  `ANTHROPIC_API_KEY`. The summarizer uses the same adapter.
- Verified with six wire tests, a local HTTP streaming test (path and
  headers), a profile validation test, and a host smoke against a local SSE
  server. A live API call was not run because no key is present.
- Task doc: `docs/llmwiki/tasks/anthropic-provider.md`.

### Keyring credentials

- Profiles gain `api_key_keyring` (`service:account`); the host resolves the
  environment variable first, then the profile entry, then the provider
  default (`keel:openai`, `keel:anthropic`).
- `--keyring-set service:account` reads a secret from stdin and stores it in
  the OS keyring; `--keyring-delete` removes it. The secret is never printed.
- Windows uses Credential Manager, macOS the Keychain, Linux kernel keyutils.
- Smoke: stored `keel-smoke:test`, ran a keyring-only profile against a local
  Anthropic SSE server, saw `x-api-key: smoke-secret`, then deleted the entry
  and confirmed a second delete reports "does not exist".
- Task doc: `docs/llmwiki/tasks/keyring.md`.

### Interjections

- Kernel: bounded, deduplicated `InterjectionQueue`; `TurnDriver::interject`
  stamps the active turn and emits `InterjectionQueued`. The driver drains at
  the top of the model loop, so delivery happens before the next request and
  before the final call, never inside a stream or tool round.
- Delivered text enters the request as `[user interjection] …`; late items are
  reported in `TurnOutcome::late_interjections` and `InterjectionDropped`.
  Event-log redaction covers all three interjection events.
- TUI: `Enter` while a turn runs queues an interjection; slash commands still
  work; full/duplicate and late items are shown. The line printer reports late
  items.
- Verify: queue unit test, two driver tests (delivery after a stream/tool
  round; late reporting), two TUI reducer tests, and the full suite.
- Task doc: `docs/llmwiki/tasks/interjections.md`.

### OS sandbox proposal

- ADR 0015 (Proposed): confinement is a backstop for what the permission gate
  cannot see — the implicit effects of an already approved process. It covers
  two things only: writes outside the derived roots and reads of a fixed
  credential set. Ordinary reads, the network and every explicitly granted
  path stay open.
- The user surface is one boolean (`[sandbox] enabled`) plus `/allow <path>`;
  a blocked command names the path and the command that fixes it. A denial in
  ordinary work is treated as a bug in the derived tables, not a user setting.
- Port in `keel-core`, backends in a new `keel-sandbox` crate; the kernel keeps
  no platform syscall code. Roots come from the permission surface, the state
  root, temp, a built-in toolchain cache table and the home entries minus the
  secret set.
- Planned backends: Landlock on Linux, `sandbox-exec` on macOS, a job object on
  Windows. AppContainer is deferred because it mutates the user's directory
  ACLs; Windows reports the filesystem layers absent and the startup line says
  so.
- Network confinement is out of scope in v1: an always-on deny breaks package
  managers, and the README states the gap instead of implying isolation.
- Plan and verification matrix: `docs/llmwiki/tasks/os-sandbox.md`; open
  questions 17 and 18; provenance rows for the platform documentation.

### Permission frictions

- Split the deny list. A structural denial (`.keel`, operating-system areas,
  storage devices) stays unconditional; a credential name is refused outside
  every root and asks for confirmation inside one. A session path grant opens
  it, a full-auto escalation does not. `.env.example` and its template
  siblings are ordinary files.
- `shell` no longer rejects a non-standard program before the gate; the
  permission request marks it and the execution confirmation decides.
- `read_file` reads at most 8 MiB and appends a size marker instead of
  refusing the file.
- The collapsed turn line reports `N blocked`.
- Verify: 412 passed, 0 failed, 7 ignored; fmt and clippy clean.
- Task doc: `docs/llmwiki/tasks/permission-frictions.md`.

### Long turns and pasted credentials

- Turn limits are runaway protection again: 500 model calls, a 24-hour
  deadline, a five-minute tool timeout; the coding profiles move from 12 to
  120 tool calls and expose the `todo` tool.
- The driver continues a turn whose model stopped while the `todo` list still
  had unfinished items, up to six times, with a `TurnContinued` event and a
  `TurnEvent::Continued` transition; wrap-up and the reserved final call are
  never overridden.
- A credential the user pastes to have it configured is no longer described as
  a leak: the notice says it was not blocked and is redacted, the TUI shows the
  same notice, and the coding prompt says to write it to the named file
  without repeating it.
- Verify: 414 passed, 0 failed, 7 ignored; fmt and clippy clean.
- Task doc: `docs/llmwiki/tasks/autonomy.md`.

### Activity-based timeouts

- A command's timeout is now an inactivity window: output resets it, so a
  download or build that keeps printing is not cut; a silent command still
  dies, and a two-hour hard cap bounds a command that never stops printing.
- A model stream resets its window on every chunk; only silence fails.
- The turn deadline is a 24-hour backstop; the call budget ends a turn first.
- Read-only calls stay parallel but are capped at eight concurrent; mutations
  remain serialized.
- ADR 0016 (Proposed) designs background commands: start, return a handle, and
  deliver the result at a safe point.
- Verify: 416 passed, 0 failed, 7 ignored; fmt and clippy clean.
- Task doc: `docs/llmwiki/tasks/activity-budget.md`.

### Turn cost budget

- `TurnLimits::max_turn_input_tokens` bounds a turn's spend: one hundred times
  the working set, at least one million tokens. It is derived, not configured.
- Reaching it records `TurnBudgetExhausted` and forces the answer call; the
  model sees the ordinary final-call instruction, never a number, so it does
  not drift toward optimising a counter.
- Call limits are now 500 model calls and a 1,000-call coding tool budget,
  left as runaway protection only.
- Verify: 417 passed, 0 failed, 7 ignored; fmt and clippy clean.

### Runnable behavior harness

- `crates/keel-runtime/examples/behaviors.rs` runs eleven checks against the
  mock provider with no network: todo continuation, token budget forcing the
  answer call (and no token number in the request), an active command
  outliving the inactivity window, a silent command stopped by it, the
  permission matrix (`.env.example`, `.env`, outside-root secret, `.keel`),
  8 MiB truncation, the non-standard program marker, and twenty reads bounded
  at eight concurrent.
- End-to-end mock turn through the reference host: `state: Done`, 21 events,
  the SQLite store written, `--view sessions` reports the turn.
- Verify: the harness prints `11 passed, 0 failed`.

### Long task, mock and real

- `examples/long_task.rs`: sixty writes in one turn with the mock — 62 model
  calls, 60 tool calls, one continuation, all files written, no budget
  exhaustion (66 ms).
- `evals/cases/coding/long-refactor`: twenty modules in one turn, two hidden
  tests; the case rejects the setup and accepts the reference.
- `run_case.py` now reads the persisted event stream and the per-call tool
  counts, records `mechanisms`, and fails a case that misses its declared
  thresholds (tool calls, model calls, permission denied, budget exhausted).
- Real deepseek-flash run through the harness: 11 model calls, 49 tool calls,
  up to ten tools in one round, 0.0% permission denials, 18.4 s, verify OK.

### Real long runs

- `novel-6` (deepseek-flash, one turn): 8,069 Chinese characters in six
  chapters, 19 model calls, 36 tool calls, 156 s, 96% cache hit, 8/8 todo
  items, no wrap-up and no budget exhaustion. The first attempt at a 16k
  working set overflowed on the final book assembly; 24k and no full-book
  re-emission passed.
- `long-refactor` (deepseek-flash): 20 modules, 11-13 model calls, 49-52 tool
  calls, up to ten tools in one round, 18-26 s.

- `consumer-app` (deepseek-flash, one turn): a working `keel-notes` crate
  inside an empty workspace — path deps, an `ask` command, two unit tests —
  25 model calls, 43 tool calls, 71 s, 94.5% cache hit. The first attempt
  stopped and asked for permissions; the second wrote outside the workspace
  because the task gave absolute paths; workspace-relative wording passed.

### Auto-continue across turns

- Host policy: when a turn ends `Done`/`Converge` with open task items, start
  another turn with a fixed nudge, up to `--auto-continues` (default 3, 0
  off). At the limit, print the remaining count and return control.
- `--input` and `--session` both honor it; `--todo-file` seeds a deterministic
  list for a test.
- Real deepseek verification on the twenty-module case: 3 calls/turn needed
  three turns (`auto-continue 1/2`, `2/2`, then `19 task(s) remain`); 10
  calls/turn finished in three turns and the case verifier passed.

### Cross-turn continuation is opt-in

- `--auto-continues` now defaults to 0. A new turn spends a new token
  budget; restarting one silently would spend money without consent, so batch
  runs opt in and see `auto-continue k/N`.
- The TUI never restarts a turn by itself: when a finished turn leaves tasks
  open it prints `N task(s) remain; press Enter to continue` and pre-fills
  `Continue the task list`.
- `run_case.py` accepts `max_model_calls` in `meta.toml` so a case can force
  the multi-turn path.

### Background commands (ADR 0016)

- `LocalBackgroundTasks`: up to four processes, output under the outputs
  directory, kill on drop, two-hour cap, `bg-N` handles.
- `shell` takes `background = "true"` (string, the parameter model has no
  boolean kind) and returns immediately.
- The driver drains finished commands at the model-loop safe point and injects
  `[background bg-N finished] ...`; `BackgroundResult` audits it. Results that
  miss the last call wait for the next turn.
- Tests: background output/exit, the four-command limit, the shell flag with
  and without a registry, and delivery before the next model call.

### Sub-agents (ADR 0017)

- `SpawnAgentTool` + `SubAgentRunner` in the runtime; `HostAgentRunner` in the
  reference host builds a child driver with a fresh context, the same registry,
  permissions and interactions, its own event log and lowered limits.
- Only a bounded report with state and usage returns; nesting is refused at
  depth one; a failed child reports its error text.
- Real deepseek run: parent wrote notes.txt, the child rewrote it as alpha.txt
  (`ALPHA BETA`) and reported, the parent verified; 5 model calls, 54 events,
  10.9 s. The first run exposed a child-context bug (stage mismatch) that the
  new error reporting made visible.
- Serialized because the tool is `Mutating`; parallel spawns need a
  concurrency class.

### Parallel sub-agent spawns

- `Tool::concurrent()` lets a mutating tool join the parallel batch; the risk
  class stays as declared for the UI and audit.
- Each child registry binds `spawn_agent` to a deeper runner that refuses, so
  siblings are free and a grandchild is rejected.
- Real deepseek run: two spawns in one round wrote a.txt and b.txt; the parent
  read both and reported each child's usage (3 model calls, 2 tool calls).

### Delegated usage is counted

- `ToolResult` gains an optional `usage`; `spawn_agent` attaches the child's
  tokens and the driver adds them to the turn usage, so the token budget sees
  delegated work.
- Real deepseek check: the same two-spawn task now reports the parent's input
  as 14,458 tokens (own ~6k plus the children's), where it was 6,082 before.

### Sub-agent eval case

- `evals/cases/coding/subagent-delegation`: the parent must delegate to
  `spawn_agent`, the child writes `alpha.txt`, and the verifier checks the
  artifact (setup rejected, reference accepted).
- Real deepseek run through the harness: PASS, 5 model calls, 6 tool calls,
  21,627 input tokens (including the child), 17.5 s.

### Coding suite regression

- 20 real deepseek cases under the new limits: 18/20 on the first run. The two
  failures were `verification did not run`: both tasks used exactly the eval
  profile's 12-call tool budget, hit `TurnsExhausted` and never reached the
  acceptance phase, although the hidden tests passed.
- Fix: `variants/coding/profile.eval.toml` tool budget 12 -> 60. Both cases
  rerun to PASS (10 and 6 tool calls), so the corrected set is 20/20.

### Real-trigger eval cases

- `background-command`: the agent starts a background python command, writes
  another file, waits, and reads the background artifact. PASS in 9.1 s.
- `tool-timeout`: a silent five-second command is stopped by a one-second
  inactivity window and the agent records the error. PASS in 7.8 s.
- `continuation`: a seeded two-item task list, the model stops after item one,
  the driver continues the same turn (`continuations: 1`); both files exist.
  PASS in 5.7 s.
- `run_case.py` passes `tool_timeout_ms` and `todo_file` from `meta.toml`.

### Real budget and interjection triggers

- `--max-turn-tokens N` overrides the derived per-turn budget for a test; the
  `token-budget` case uses 1,000 and the harness requires
  `budget_exhausted: true`. PASS: 2 model calls, the artifact written, the
  forced final call.
- `--interject TEXT --interject-after-ms N` injects a user message while the
  turn runs; the `interjection` case delivers it during a three-second shell
  call, and the model writes `injected.txt`. PASS: 3 model calls.

### Kernel cleanup and sub-agent audit

- `SubAgentFinished` (parent turn, role, state, usage, error) is appended to the
  parent event log; a real run shows the event in `--view events`.
- `profile.deepseek.agent.toml` folded into the consumer profile (one variant
  fewer); the sandbox ADR is marked Deferred.

### Sub-agent audit trace

- `SubAgentFinished` now carries up to 64 `SubAgentStep` entries (tool, ok,
  duration). Real run payload: role=proofreader, state=done, 5 model calls,
  7 tool calls, steps = list_dir, read_file, shell x3, read_file, shell.
- Verified by reading `--view events` after a real delegation.

### Background lifecycle fixes (2026-11-05)

- Background tasks now kill the whole process tree on drop and at the
  two-hour cap, matching the foreground path (`kill_tree`: process group on
  Unix, `taskkill /T /F` on Windows). The old `child.kill()` left grandchildren
  alive.
- `BackgroundStarted` records the handle, program, pid and turn at start.
  Previously a persisted log showed a `background_result` for a handle that
  was never introduced.
- `BackgroundTasks::start` takes the turn id; the host wires the event log
  into `LocalBackgroundTasks`.
- Tests: `a_background_start_is_audited`, and
  `dropping_the_registry_kills_the_process_tree` (a Python grandchild that
  would write a marker after the registry is dropped). Real eval run
  `background-command` PASSes; `--view events` shows `background_started`
  then `background_result` for `bg-1`.

### Persistence guidance (2026-11-05)

- ADR 0016 gains decision 8: background commands are session-scoped, and
  cross-session persistence is an explicit OS service (Task Scheduler,
  `systemctl --user`, LaunchAgent) installed with user confirmation. The shell
  tool never detaches processes implicitly.
- `variants/coding/prompts/coding.md` states the same for the model, including
  the ban on `nohup`, `setsid`, `start /b` and trailing `&`.
- This is guidance, not enforcement: the foreground tree guard still disarms
  after the direct child exits, so the kernel does not physically stop a
  detached descendant.

### Real persistence-guidance runs (2026-11-05)

- Session scenario: "start a local HTTP server and confirm it responds".
  `deepseek-flash` used `shell background="true"` (`background_started` bg-1,
  pid 25952), verified with a real request (200). After the CLI exited the pid
  was gone and nothing was listening on 8765 — the session-scoped kill works.
  The answer volunteered the Task Scheduler option and said it needs
  confirmation.
- Autostart scenario: "prepare login autostart, do not install or run it".
  The model wrote `heartbeat.py`, `run_heartbeat.vbs` and a README, proposed
  the Startup folder as plan A and `schtasks /Create ... /SC ONLOGON` as plan
  B, both with uninstall steps. It installed nothing, created no task, and
  left no process. It asked the user before any registration.
- The persona now names "Task Scheduler or the Startup folder" because the
  real run chose the Startup folder.

### TUI process details (2026-11-05)

- A finished turn collapses its tool lines into the aggregate line; `Ctrl-O`
  or the new `/details` command reveals them for the whole conversation. The
  toggle only filters hidden blocks and never re-runs or reorders.
- `/details` is the discoverable form of the key; `/help` lists it.
- `docs/llmwiki/tasks/tui-daily.md` records the behavior and why it is a
  global toggle rather than a per-turn selection: no cursor model, one key.

### Bilingual TUI chrome (2026-11-05)

- New `hosts/keel-cli/src/ui_text.rs`: `Lang` (zh/en) with resolution
  `--lang` → `LC_ALL`/`LC_MESSAGES`/`LANG` → `GetUserDefaultUILanguage` (no
  new dependency) → English, and a `Text` table for every chrome string.
- The TUI renders the empty hint, status line, notices, summary fields, slash
  replies, continuation hint and permission option labels through the table.
  English permission labels still come from the kernel; Chinese translates the
  known ids. The permission sentence itself is still kernel English; fixing it
  needs structured fields on `InteractionRequest`.
- `--lang auto|zh|en` added and listed in `--help`; `auto` detects.
- Tests: resolution order, both arms, Chinese screen render. 433 pass.

### Minimal status line (2026-11-05)

- The status line is now `● state · tasks open/total · elapsed` on the left
  (tasks only while some are open, elapsed only while a turn runs) and the
  permission mode plus one contextual key on the right (`Shift+Tab`, `Ctrl-O`
  when process blocks are collapsed, `Ctrl-C` while running).
- Removed the workspace and model name from the status line; `/roots` still
  lists scopes, and the model is a profile decision. `TuiHost` no longer
  carries a workspace string.
- Task counts refresh from the shared `TodoList` before every draw.

### Correct permission mode labels (2026-11-05)

- The status line said `Confirm` / `Auto`, which implied a third
  always-confirm level. The kernel has exactly two levels (ADR 0002):
  `Workspace` (inside the roots auto, outside asks) and `Full auto` (no
  boundary, hard deny remains). The labels are now `工作区` / `全自动` and
  `Workspace` / `Full auto`. There is no `manual` level; it is a host-level
  wrapper the reference host does not implement.
- Goldens regenerated; 433 tests pass. Built in `target/verify` because a
  running `--tui --demo` session held the default binary.

### TUI state coverage and detail reveal (2026-11-05)

- `Ctrl-O` and `/details` share one toggle. Revealing now scrolls to the last
  turn's first hidden block; before, the view stayed at the bottom, so the
  process reappeared off-screen and the toggle looked broken.
- Every terminal outcome gets a line: cancelled, failed, missing answer (using
  `Answer::missing`, which the kernel already set), and any other state is
  named. Previously a cancelled or empty turn showed only the aggregate
  summary.
- Tests: `ctrl_o_reveals_the_last_turn_process`,
  `a_cancelled_turn_says_so`, `a_missing_answer_is_explained`.
- Verified in `target/verify` because a running `--tui --demo` holds the
  default binary.

### Workspace identity in the TUI (2026-11-05)

- The status line shows the workspace base name and `+N` when extra scopes
  exist (`--root`), matching the mainstream pattern of keeping the project
  identity visible. Full paths stay in `/roots`.
- `TuiHost` carries the workspace path again; `App` stores the base name and
  the extra-root count.
- Test: `the_status_line_names_the_workspace_and_extra_roots`.

### Session scopes and /add-dir (2026-11-05)

- `PermissionEngine::add_root(path, access)` and `TurnDriver::add_root` add a
  canonical scope after construction. `extra_roots` is now behind a mutex;
  `check`, `roots` and `with_root` take the lock.
- A runtime root is a session scope, not a grant: `revoke_all` (the full-auto
  toggle) does not remove it. Adding the same path twice returns the first
  scope.
- TUI `/add-dir <path> [ro|rw]`: default rw, relative paths resolve against
  the workspace, `/roots` and the status line's `+N` update on success.
- Tests: parse/defaults, recorded root updates the status, runtime root
  survives revocation, duplicate add keeps the first scope.

### Input growth and display styles (2026-11-05)

- The input box wraps the message itself, grows to six content lines and
  scrolls around the cursor; the layout caps it on short terminals.
- A sent user message longer than eight wrapped lines collapses to its first
  six plus `… +N lines (Ctrl-O)`; the same toggle reveals the turn process.
- Palette: running tool yellow, completed gray, failed red; idle status dot
  green, working yellow; answer marker green, summary/notice unchanged.
  Documented in `tui-daily.md`.
- Tests: `the_input_box_grows_with_a_pasted_message`,
  `a_long_sent_message_collapses_until_details_are_shown`. 443 pass.

### Transcript hierarchy and code blocks (2026-11-05)

- Removed the bullet glyphs that broke the left margin. Process lines share a
  column-2 gutter (`…` yellow, `✓` dim, `✗` red); `in` and `out` are nested at
  column 4, so a tool call reads as one block. The answer is plain text at
  column 0 with the summary underneath; notices and errors align at column 2.
- `Event::ToolStarted` gained `args_preview` (bounded, redacted, skipped by
  serialization); the dispatcher fills it, so the UI can show inputs without
  touching the persisted stream.
- Assistant/result text renders fenced code blocks with a dim `│` bar, inline
  code in cyan (backticks removed), bold, headings and bullets.
- Blank lines only around user messages and the answer, not between every
  block.
- Tests: input/output separation, code block, inline code. 446 pass.

### Code boxes and more Markdown (2026-11-05)

- Fenced code blocks render as a rounded box with the language on the top
  border; long lines wrap inside the box.
- Markdown tables render as aligned columns with a rule under the header;
  quotes use a dim bar, rules become a horizontal line, headings are bold.
- Inline code stays cyan; bold, bullets and fences are removed from the text.
- Tests: code box borders/language, aligned table. 447 pass.

### Telemetry gaps closed (2026-11-05)

- `PermissionGranted`/`PermissionRevoked`: every session grant is audited
  (path grant from a prompt, full-auto escalation, revocation). Host-level
  actions use turn 0 and are persisted with the next turn.
- `SubAgentStarted` with the role and cap, before the child runs.
- `BackgroundResult` now carries the program and output path; dropping the
  registry records a terminated result.
- `ModelFinished` carries `duration_ms`; `ModelFailed` records code, message
  and duration at every failure exit (setup error, timeout, cut stream).
- Tests: model failure event, host grant audit, background result fields.

### Kernel upgrade support (2026-11-05)

- `keel_core::KERNEL_VERSION` and `EVENT_FORMAT_VERSION`; `SNAPSHOT_VERSION`
  re-exported from `keel-core`.
- New dev-only crate `keel-testkit`: a mock-model `Probe` plus assertions and
  the event-name contract; six conformance tests (text turn, tool turn, model
  failure, cancellation, snapshot refusal, version constants).
- `docs/kernel-upgrade.md`: pinning, the upgrade checklist, what is breaking,
  the event policy and the `cargo-semver-checks` command.
- Kernel-only CI includes `keel-testkit`.

### Tool-result share measured; no code-execution sandbox (2026-11-07)

- `run_case.py --keep` now keeps the eval state home, so the event stream in
  `keel.db` survives and per-call usage can be analyzed.
- `evals/harness/tool_share.py`: computes, per turn, the input a tool round
  injects into the next model call (`input(n) - input(n-1) - output(n-1)`),
  its share of input, the round-trip count and the longest read chain.
- Three coding cases on `deepseek-flash`: tool results are 2.2%, 9.7% and 5.0%
  of model input; a turn takes 8-12 model calls; context never exceeded 7k of
  a 24k budget. Cost ceiling for removing it entirely is 5-25%, before the
  output tokens PTC would spend writing the orchestration code.
- Decision: do not build a code-execution sandbox. The dispatcher stays the
  only path to a tool, which is what keeps the permission gate, path
  confinement and the read-only/parallel policy.
- Report: `evals/reports/2026-11-07-tool-result-share.md`.

### `edit_file` exact-match tool (2026-11-08)

- `keel-runtime::tools::EditFileTool`: `edit_file { path, old, new,
  replace_all? }`. Literal match; exactly one occurrence by default; zero or
  multiple matches fail without writing; `replace_all` is explicit.
- Existing UTF-8 regular files only, capped at the read limit; the path goes
  through the write permission gate; the previous version is backed up to the
  trash before the write.
- Three contract tests: single replacement with backup, ambiguous match
  refusal plus `replace_all`, and missing/empty inputs. ADR 0018 records the
  contract.

### Acceptance after workspace changes (2026-11-08)

- `TurnDriver::with_acceptance_after_changes`: the checklist runs only when a
  workspace-changing tool (`write_file`, `edit_file`, `move_file`,
  `delete_file`, `make_dir`, `restore_file`) succeeded in the turn. A
  conversational turn skips it; `shell` is excluded so a test command cannot
  trigger a second verification run.
- Two turn tests: a text-only turn with a failing checklist stays `Done` with
  no verification event; an `edit_file` turn runs the checklist and reaches
  `Done` with evidence. ADR 0004 records the mode.

### OpenAI thinking turns keep `reasoning_content` (2026-11-08)

- `keel-model-openai::wire::build_request` now includes the assistant
  `reasoning_content` field when the request has `thinking_budget > 0`, even
  if `reasoning_effort` is not advertised. DeepSeek-style thinking providers
  reject a follow-up tool round that drops it.
- A wire test pins the follow-up request. Found by Keelson's live demo eval.

### Workspace read and search options (2026-11-08)

- `read_file` accepts optional `offset` and `limit` in lines. The range is
  returned as its exact bytes, CRLF preserved and without line numbers, so it
  can be handed to `edit_file`. An offset past the end reports the line count;
  a value below 1 is `InvalidArguments`.
- `code_search` accepts `case_sensitive` and a file-name `glob`. A pattern
  with a path separator matches the workspace-relative path; `*` and `?` are
  the only wildcards. No regex and no ripgrep dependency.
- The skipped-directory list now covers `out`, `coverage`, `vendor`, the
  virtualenv and tool caches, and `.next`/`.svelte-kit` build output.
- Four `read_file` contract tests and one search filter test. ADR 0019 records
  the contract.

### Read before write, line-ending fallback and glob (2026-11-08)

- `read_ledger::ReadLedger`: a per-engine SHA-256 record of the files the
  model has read. `write_file` on an existing file and every `edit_file`
  require a current read; a new file needs none. The refusal is a retryable
  `InvalidArguments` that tells the model to read again.
- `ReadFileTool`, `WriteFileTool` and `EditFileTool` gain `with_ledger`.
  The `new()` constructors keep the old behavior for hosts and tests that do
  not wire one.
- `edit_file` falls back to a line-ending-normalized match when the exact
  snippet is absent. A CRLF file edits with an LF snippet and stays CRLF; an
  LF file edits with a CRLF snippet and stays LF. The fallback is still
  literal: zero matches fail and multiple matches stay ambiguous.
- `GlobTool`: `glob { pattern, path?, limit? }`. `*` within a segment, `**`
  across segments, a pattern without a separator matches the file name at any
  depth. Files only, newest first, bounded. ADR 0021 records the contract.
- Five tool tests and three ledger tests; ADR 0020 records read-before-write
  and ADR 0018 gains the line-ending amendment.

### Atomic multi-edit (2026-11-08)

- `MultiEditTool`: `multi_edit { path, old: string_list, new: string_list,
  replace_all? }`. The parallel lists must have the same length. Each pair is
  applied in order to an in-memory copy; a missing or ambiguous snippet fails
  the whole call before any backup or write.
- Matching is the `edit_file` contract: literal, exact first, line-ending
  normalized fallback, read ledger checked once. One trash backup and one
  write for the batch.
- Six unit tests: success, atomic failure with no backup, unequal lists, the
  read guard, line-ending fallback and `replace_all`. ADR 0022 records the
  contract.

### Regex content search (2026-11-08)

- `CodeSearchTool` accepts `regex: boolean`, default false. The literal
  substring path is unchanged. In regex mode `case_sensitive` sets the
  builder's `case_insensitive(!case_sensitive)`, so the default stays
  case-insensitive.
- The pattern is compiled once per call; an invalid pattern is
  `InvalidArguments` before the walk starts. Matching is line-oriented, with
  the existing file-size, depth, result and line-length limits.
- New `regex` 1.13 dependency (MIT OR Apache-2.0); `cargo deny check
  licenses` passes and the audit table records it. ADR 0023 records the
  contract.
- Three contract tests: a pattern match, the case flag in regex mode and an
  invalid pattern. The literal and glob tests are unchanged.

### Denied calls keep their arguments (2026-11-08)

- `ToolDispatcher::reject` takes the redacted args preview. Invisible,
  unknown, invalid-argument and permission-denied calls now emit a
  `ToolStarted` event with that preview instead of `None`, so the transcript
  shows the command a user denied (or the arguments that were invalid).
- The preview is the same `compact_args` + `redact_secrets` value used for a
  prepared call. A dispatch test asserts `text=hi` survives a denial.
- Found by Keelson's real-task run on a copied GameVault core: every denied
  `shell` row had an empty argument column.

### Workspace interpreter paths run (2026-11-09)

- A check may name its program by an absolute path inside the workspace when
  the file name alone is allowlisted, with no `..` component. A project venv's
  `.venv/bin/python` now runs; a path outside the workspace, a relative path
  and an unlisted file name are still refused.
- Found while auditing Keelson's coding rig: the Python check preferred the
  project venv, `is_allowed` rejected every path, and every turn in a project
  with a venv failed a required check with `not allowlisted` before the code
  was considered.
- Three contract tests: a workspace path whose name is allowlisted runs, a
  path outside the workspace is refused, a workspace path with another name is
  refused. ADR 0030 records the contract.
- The shell allowlist is unchanged; only the acceptance verifier accepts a
  workspace path.

### Read paging and check timing (2026-11-09)

- `read_file` without `offset` returns at most 1000 lines and names the rest
  (`[showing lines 1-1000 of N; use offset=1001 to continue]`). A ranged read
  starts with a display-only `[file: N lines; showing a-b]` header.
- The read ledger records 1-based line ranges; ranges that cover every line
  count as a full read, so a long file can be paged and then rewritten.
  Overlapping ranges merge, an edit drops them unless the read was full, and a
  byte-truncated read never contributes. ADR 0031.
- A command or test item reports its own run time as
  `[duration: 0.42 s]` in the evidence detail, on pass and on failure. ADR
  0032.
- The schema description and the tool tests cover both; the ledger gained four
  range tests and the tools two paging tests.

### Search reaches the files a read can reach (2026-11-09)

- `code_search` skipped every file above 1 MiB without saying so. A 40,000-line
  module is about 1.2 MB, so the model could read and edit it but could not
  search it, and a search that found nothing there looked like an empty file.
- The limit is now the same 8 MiB as `read_file`, and a skipped file is named
  in the output: `... N files above 8388608 bytes were not searched: a.txt,
  b.txt`.
- Two tests: a 1.2 MB file is searched, and a 9 MiB file is named while a
  small file still matches.

### The read cap covers every read (2026-11-09)

- The 1000-line guard only applied to a read without `offset`. `read_file`
  with `offset: 1` and no `limit` returned the whole file, so the guard was
  one parameter away from being bypassed.
- Every read now stops at 1000 lines. A range that runs past the cap ends with
  `[stopped after 1000 lines; use offset=N to continue]`, and a `limit` above
  1000 is capped the same way.
- One test covers both an absent and an oversized `limit`.

### A choice can name its recommended option (2026-11-09)

- `InteractionRequest` carried a `default` field that no provider parsed and no
  host read. It is now `recommended: Option<String>`, the id of the option the
  model would pick.
- The control tool accepts `recommended`; both providers parse it and advertise
  it in the schema. A typo'd id marks nothing rather than failing the call.
- Keelson's desktop marks the option with a badge; the answer is one click
  instead of a read of every label. ADR 0033.
- Tests: the OpenAI and Anthropic parse tests assert the field.

### Empty thinking rounds keep the reasoning field (2026-11-09)

- DeepSeek V4 thinks by default and returns `reasoning_content` on tool rounds,
  often as an empty string. The driver attached reasoning to the tool-call
  message only when the text was non-empty, and the wire echoed the field only
  when the profile advertised `thinking_budget > 0` or `reasoning_effort`.
  Keelson's live profile sets neither, so the OpenAI-compatible DeepSeek path
  dropped the field and the follow-up returned 400: "The `reasoning_content`
  in the thinking mode must be passed back to the API."
- The driver now remembers that a round thought, even when the reasoning text
  is empty, and attaches it to the tool-call message; the wire includes the
  field whenever an assistant message already carries it, not only when a
  budget or an effort level is advertised.
- Tests: an empty thinking round is echoed on the tool follow-up (it fails
  without the driver change), and a captured reasoning echo is sent without a
  budget or `reasoning_effort`.

### A context budget ladder (2026-11-09)

- `[context] budget_tokens` stays the base working set. A new optional
  `max_budget_tokens` gives the ceiling; absent keeps the old fixed behaviour.
- The driver doubles the working set when a real signal says the content does
  not fit — Red pressure or an over-budget per-call estimate — up to
  `min(profile max, model window − output reserve)`. It is monotonic for the
  engine's lifetime so the history prefix stays cache-friendly.
- Escalation is emitted as `context_escalated { from, to, reason }`. The
  wrap-up instruction and tool-result elision now run only at the cap.
- New tests: a tool result that exceeds a tiny budget escalates and finishes
  instead of overflowing; the same setup with `max_budget_tokens` pinned keeps
  the old overflow error.
- ADR 0034 records the policy. Verification: full workspace tests, clippy and
  fmt pass.

### Tool errors that say what to do next (2026-11-09)

- A live Sojourn turn (47 tool calls, 4 failures) showed three self-corrected
  model mistakes and one false failure: the todo list closes itself when its
  last item becomes terminal, so the explicit `close` that followed was
  rejected as "todo list is closed" although the requested outcome was
  already true.
- `TodoList::close` is idempotent for the state the list already holds; a
  conflicting close returns `TodoError::AlreadyClosed(state)` and its message
  names that state.
- A rejected `close completed` names the open items
  (`not every item is completed: #2 ...`).
- `ToolSchema::validate` names the accepted arguments when it rejects an
  unknown one (`unknown argument 'op2'; accepted: op, id, text, status,
  outcome`).
- Tests: idempotent close on the current and an explicit outcome, close after
  auto-close through the tool, the open-item message, and the unknown-argument
  hint. ADR 0035 records the policy; released as 0.1.1.

### Summary cadence, covered index and memos (2026-11-09)

- The replay showed the summary rewritten at every seal: 8 seals in 16 turns,
  seal-turn cache share as low as 13%, and the summary shrinking from 11.1k to
  3.6k characters as it dropped older topics.
- The driver now rewrites at most every `summary_every_seals` seals (default
  3) or at Amber+; a deferred seal keeps its record in context.
  `TurnDriver::with_summary_cadence` tunes it.
- Covered segments render as a bounded index beside the summary
  (`render_covered_index`), so dropped topics stay discoverable and paths stay
  exact.
- The summary prompt asks for a leading `SEGMENT_MEMO:` line; the driver
  splits it, stores it on `SegmentRecord::memo`, and recall renders it instead
  of a raw excerpt. Snapshots without the field load unchanged.
- Tests: third-seal rewrite only, index names the dropped topic, memo split
  and no-marker fallback. ADR 0037; released as 0.2.0.

### A segment seals on text size (2026-11-09)

- The clean replay's active segment stayed open across ten same-topic turns,
  keeping its text verbatim/digested in the history while it grew. The twelve
  turn cap was too coarse for large answers and tool summaries.
- `SEGMENT_TOKEN_CAP` (12,000 estimated text tokens) now seals with reason
  `size` beside the turn cap, before the follow-up marker. Sealed text becomes
  a record and a later summary candidate; a segment without a complete
  goal/result keeps its text in history, so nothing is lost.
- Does not address a single turn's tool-output peak; tool-result elision is
  unchanged.
- Test: a large same-topic segment seals before the turn cap. ADR 0038;
  released as 0.2.1.

### Session control, background views and acceptance start (2026-11-10)

- A closed task list resets at the next user turn instead of rejecting the
  session's second task (ADR 0040); execute inside a write root runs without
  a prompt under `workspace_auto` (ADR 0041); the base permission level is
  runtime-mutable and a switch drops session grants.
- `verification_started` precedes the acceptance plan (ADR 0042) so a host can
  show a waiting state after the answer is written.
- The shell takes a per-call `timeout_ms` clamped to two hours (ADR 0043), and
  `LocalBackgroundTasks::snapshot` gives a host a non-consuming view of
  running and unreported background commands (ADR 0044).
- Aggregate tool-result elision was measured and rejected; ADR 0039 records
  the replay data (higher failure rate, 2.4x wall time).
- Verification: full workspace tests, clippy and fmt pass; released as 0.2.2.

### A rewrite keeps line endings (2026-11-10)

- `write_file` now writes in the existing file's line-ending style when the
  file contains a CRLF pair, reusing `with_line_ending` from the edit tools
  (ADR 0045). A live turn's six-line change had shown as +142/-139 because the
  model wrote LF into a CRLF file.
- An LF file and a new file are written as given.
- Tests: a CRLF file keeps CRLF for LF content; an LF file stays LF.
  Released as 0.2.3.
