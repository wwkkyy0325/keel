# Changelog

Kernel crates and the reference host share the workspace version. A derived
project pins a version and updates deliberately; see `docs/kernel-contract.md`.

## 0.2.12 — 2026-11-11

### Kernel

- A permission prompt labels only an `Execute` detail as a command; any other
  tool detail is shown as a sentence. A screen capture reads
  `window Notepad` instead of `command: window Notepad`, while the shell
  keeps its command wording.

### Evidence

- The confirmation for a capture named a folder and then a "command" that was
  not a command. Tests: a shell detail keeps `command:`, a write detail does
  not.

## 0.2.11 — 2026-11-10

### Kernel

- Tool images no longer split a group of tool results. When several calls run
  in one round and one result carries an image, the OpenAI-compatible adapter
  emits every `tool` message first and then one user message with the images;
  inserting the image message between tool results violates the rule that a
  `tool_calls` message is answered by its tool messages.

### Evidence

- A live turn failed with `400 Bad Request: An assistant message with
  'tool_calls' must be followed by tool messages responding to each
  'tool_call_id'` while a screenshot was in the same round as another call.
  Test: two tool results, the first with an image, produce `tool`, `tool`,
  `user(image)`.

## 0.2.10 — 2026-11-10

### Kernel

- Messages and tool results can carry images (ADR 0051). `ImagePart` travels
  in memory only; the session snapshot stores no image bytes, so an image is
  evidence for the turn that produced it. The driver attaches a result's
  images to its tool message.
- The OpenAI-compatible adapter sends user content as an array and follows a
  tool message carrying images with a user image message; the Anthropic
  adapter puts images inside `tool_result` blocks and user content blocks.
  Each image is charged 1024 tokens in the context estimate.

### Evidence

- Tests: a tool returning a PNG reaches the next model request with the image
  on its tool message; both adapters encode a tool image and a user image,
  including the `data:image/png;base64,...` URL and the Anthropic base64
  source block.

## 0.2.9 — 2026-11-10

### Kernel

- A response cut by the output limit is reported as such (ADR 0050): both
  adapters record the provider's finish reason (`length` / `max_tokens`) and
  return an `invalid_response` error naming the limit instead of a JSON
  parse error from a truncated tool call. A text-only answer cut the same
  way is no longer delivered as complete.
- Text streamed before a model error stays in the turn's answer body, as a
  cancelled turn already does.

### Evidence

- A live turn failed with `invalid tool arguments: EOF while parsing a string
  at line 1 column 7170` after 50 s; the profile's 8192-token output cap had
  cut the tool-call JSON mid-string, and the earlier JSON error hid that.
- Tests: an OpenAI `finish_reason: length` stream and an Anthropic
  `stop_reason: max_tokens` stream both report the output limit and still
  forward the text that was streamed.

## 0.2.8 — 2026-11-10

### Kernel

- A cancelled turn stays in the session history (ADR 0049): the input is
  recorded, a streamed partial answer is kept with an
  `[the user interrupted this turn; the answer is incomplete]` marker, and
  the outcome's answer body carries the partial text so a host can show it.
  The next message continues from the interrupted question.

### Evidence

- A user asked a question, cancelled the turn, and sent `继续`; the model
  continued the previous completed turn because the cancelled input had
  never been recorded, while the transcript still showed the question.
- Test: after a cancelled turn, the next model request contains the
  cancelled input.

## 0.2.7 — 2026-11-10

### Kernel

- Acceptance checks honour the turn's cancel token (ADR 0048): the verifier
  skips remaining items when the token is set, a running command check is
  stopped by `command::run`, and the driver returns `Cancelled` afterwards.
  Cancel during "verifying" no longer waits for the whole plan.

### Evidence

- A user clicked cancel during verification and nothing happened until every
  check finished; the verifier had created a fresh token.
- Test: a cancelled token leaves every item unverified and runs nothing.

## 0.2.6 — 2026-11-10

### Kernel

- `PermissionEngine::base_level` (and its dispatcher and driver forwards)
  reports the configured level without a session grant; `level` keeps
  returning the effective one (ADR 0047). No permission check changes.

### Evidence

- A live session's chip read "full auto" from an eight-hour grant while the
  persisted setting stayed `workspace_auto`; after a restart it read
  "workspace auto", and choosing full auto had been a no-op because the chip
  already showed it.
- Test: a grant leaves `base_level` at the configured level; switching the
  base level is visible after the grant is revoked.

## 0.2.5 — 2026-11-10

### Kernel

- A list that closed itself (every item became terminal) reopens when the
  model adds a task or changes a status (ADR 0046). An explicit `close` stays
  terminal, a locked host plan is never reopened, and `remove` still refuses
  a closed list. The list remembers how it closed; snapshots without the
  field load as opened.

### Evidence

- A live turn completed 2/2 items, then `todo add` and `todo update` were
  rejected with `todo list is closed` in under a millisecond; the model had
  not asked to close the list.
- Tests: an add after an inferred close reopens the list; an explicit close
  still rejects an add; a closed locked plan still rejects an update.

## 0.2.4 — 2026-11-10

### Kernel

- The OpenAI-compatible and Anthropic stream readers keep the trailing bytes
  of an incomplete UTF-8 sequence across network chunks instead of decoding
  each chunk lossily. A Chinese character cut by a chunk boundary no longer
  becomes three U+FFFD; genuinely invalid bytes still do.

### Evidence

- A live DeepSeek answer stored `让对���知道信息来源`; the session document held
  an incomplete lead byte and two continuation bytes where `方` belongs.
- New tests: a character split across two chunks decodes whole; an invalid
  byte still becomes a replacement character.

## 0.2.3 — 2026-11-10

### Kernel

- `write_file` writes in the existing file's line-ending style when the file
  contains a CRLF pair (ADR 0045), using the same `with_line_ending` the edit
  tools already use. An LF file and a new file are written as given, so a
  rewrite of a CRLF file no longer reads as a whole-file change.

### Evidence

- A live turn changed six lines of a CRLF file and the checkpoint reported
  +142/−139; the content was right, the endings were LF.
- New tests: a CRLF file keeps CRLF when the content is LF; an LF file is
  written unchanged.

## 0.2.2 — 2026-11-10

### Kernel

- A closed task list resets at the start of the next user turn (ADR 0040).
  The reset clears the items, sets the state to active, bumps the revision
  and emits `TodoUpdated`; ids stay monotonic and a host lock is left alone.
  A session can use the todo tool for a second task again.
- `workspace_auto` runs a command inside a write root without a prompt
  (ADR 0041). `Execute` now follows the root's access: the workspace root and
  write extra roots allow it, a read-only root and anything outside still
  confirm. `FullAuto`, the secret-path checks and the shell program allowlist
  are unchanged.
- The base level is runtime-mutable (`set_base_level`;
  `TurnDriver::set_permission_level` also drops session grants), so a host can
  switch level without reopening the session.
- `verification_started` is emitted before the acceptance checks run, so a
  host can show a waiting state while the answer is already written
  (ADR 0042). The event is additive; the event format version stays 1.
- The shell tool accepts `timeout_ms`, a wall-clock bound clamped to two
  hours (ADR 0043). `CommandSpec.max_duration` is honored by the foreground
  runner and by background tasks; without it the inactivity window and the
  hard cap remain the only limits.
- `LocalBackgroundTasks::snapshot` reports running and finished-but-undrained
  commands to a host without consuming them (ADR 0044); the driver's drain
  still owns the result.

### Evidence

- A live session's second task lost every `todo add` (`todo list is closed`,
  `invalid_arguments`, under a millisecond) because the previous list had
  completed.
- The same session asked once for the first `shell` call, and the approval
  escalated to `full_auto` for eight hours; the level the user selected
  (`workspace_auto`) should have run that command.
- New tests: a closed list resets on the next turn; execute inside a write
  root is allowed, and a read-only root still confirms.

## 0.2.1 — 2026-11-09

### Kernel

- `SEGMENT_TOKEN_CAP` (12,000 estimated text tokens) seals a segment on text
  size even while the topic continues, beside the existing twelve-turn cap.
  The seal reason is the existing `size`. The check runs before the follow-up
  marker so "继续" cannot hold a large segment open.
- The estimate counts assistant tool-summary blocks; a segment that seals
  without a complete goal/result keeps its text in history, so the cap cannot
  lose content.
- This bounds the active segment's contribution to history. A single turn with
  many tool calls can still raise the per-call context; tool-result elision is
  unchanged.

### Evidence

- The clean replay's active segment stayed open across ten same-topic turns;
  the segment token cap is the mechanism that turns that text into a record
  and a summary candidate earlier.
- New test: a segment whose text exceeds the cap seals with reason `size`
  before the turn cap.

## 0.2.0 — 2026-11-09

### Kernel

- The rolling summary is rewritten at most once every
  `KernelOptions::summary_every_seals` seals (default 3), or immediately at
  Amber pressure or worse. A deferred seal keeps its structured record in the
  context until the next rewrite. `TurnDriver::with_summary_cadence` tunes it.
- Segments already covered by the summary are rendered as a bounded index
  next to it (`#id goal -> result [files]`), so a topic the rewrite dropped
  stays discoverable and its paths stay exact.
- The summary prompt now asks for a leading `SEGMENT_MEMO:` line. The driver
  splits it off, stores it on the sealed segment's record (`memo`), and
  recall renders the memo instead of a raw excerpt. A model that ignores the
  line falls back to the old summary-only behavior.
- `SegmentRecord::memo` is optional and serde-defaulted, so 0.1.x snapshots
  load unchanged.

### Evidence

- A transcript replay showed the summary rewritten at every seal: 8 seals in
  16 turns, cache share on seal turns falling to 13–76% while non-seal turns
  stayed at 95–99%, and the summary shrinking from 11.1k to 3.6k characters
  as it dropped older topics.
- New tests: a seal streak rewrites on the third seal only; the covered index
  names a topic the summary dropped; the memo line splits from the rolling
  summary; a memo-less summary stays whole.

## 0.1.1 — 2026-11-09

### Kernel

- `todo close` is idempotent: asking for the outcome the list already holds
  succeeds and returns the rendered list. The list closes itself once every
  item is terminal, so the previous behavior reported a false failure for a
  finished plan. A conflicting close is rejected with the state in the
  message (`todo list is already completed`).
- A rejected `close ... completed` names the items that still block it
  (`not every item is completed: #2 ...`), so the model can fix them without
  a listing round.
- An unknown tool argument now names the arguments the tool accepts
  (`unknown argument 'op2'; accepted: op, id, text, status, outcome`).
  Strictness is unchanged; a typo still never changes tool behavior.

### Evidence

- New tests: `closing_into_the_current_outcome_is_a_no_op`,
  `an_explicit_close_is_also_idempotent`,
  `close_succeeds_when_the_list_closed_itself`,
  `close_names_the_items_that_are_not_done`, and the extended
  `unknown_argument_is_rejected` probe.
- Found in a live Sojourn turn: 47 tool calls, 4 failures. Three were model
  mistakes it repaired itself; the close after the list had closed itself
  was the one kernel defect.

## 0.1.0 — 2026-11-01

First complete kernel and reference host.

### Kernel

- Turn FSM, driver, tool dispatcher, cancellation and turn deadlines.
- Typed tool protocol and model wire types; OpenAI-compatible streaming
  provider.
- `keel-model-anthropic`: Anthropic Messages API streaming, text/thinking
  deltas, `tool_use` input accumulation, tool-result history, the control tool
  and usage. It sits beside `keel-model-openai`; no kernel crate depends on an
  adapter.
- One turn finishes a multi-step task. The defaults are runaway protection
  (500 model calls, a 24-hour backstop), not a per-task budget. When the model
  reports a partial result while the `todo` list still has unfinished items,
  the driver continues the turn with a reminder and records `TurnContinued`;
  six continuations bound a model that never closes the list, and a wrap-up or
  the reserved final call is never overridden. `TurnEvent::Continued` keeps the
  FSM explicit.
- Upgrade support: `KERNEL_VERSION`, `EVENT_FORMAT_VERSION` and the
  re-exported `SNAPSHOT_VERSION` are the version handshakes; the dev-only
  `keel-testkit` crate provides a mock-model `Probe`, event assertions and the
  event-name contract. `docs/kernel-upgrade.md` is the product-side checklist.
- The audit stream covers session grants (`PermissionGranted`/
  `PermissionRevoked`), sub-agent starts, background results with the program
  and output path, and model failures with their code and duration;
  `ModelFinished` records how long the call took.
- Sub-agents (ADR 0017): `spawn_agent { task, role? }` delegates one task to a
  child turn with its own context, shared permissions and the parent's cancel
  token. Only a bounded report with state and usage returns; sibling spawns
  run in parallel and a nested spawn is refused. The host runner reports a
  failed child with its error text instead of a silent success.
- Background commands (ADR 0016): `shell` with `background = "true"` starts a
  process, writes its output under the outputs directory and returns a handle
  immediately. `BackgroundTasks` drains finished commands at the model-loop
  safe point and injects the result; `BackgroundStarted` and `BackgroundResult`
  audit the handle and the outcome. The local registry runs at most four, kills
  the process tree on drop and on the two-hour cap, and caps a command at two
  hours. A result that misses the last call waits for the next turn. Commands
  are session-scoped: cross-session persistence is an explicit OS service
  installed with the user's confirmation, not a detached process.
- A per-turn input-token budget (`max_turn_input_tokens`, one hundred times
  the working set, one-million floor) bounds cost. Reaching it records
  `TurnBudgetExhausted` and forces the answer call. The model never sees the
  number or a counter in the prompt.
- Timeouts measure activity, not the clock. A command is stopped after five
  minutes without output, so a build or download that keeps printing finishes;
  a two-hour hard cap bounds a command that prints forever. A model stream is
  stopped after five minutes without a chunk. Read-only calls in one round run
  in parallel, capped at eight at a time; mutating calls stay serialized.
- A credential the user hands over to be configured is served: the turn still
  records `SecretWarning` and redacts the log, and the prompt tells the agent
  to write the value to the file the user named without repeating it.
- Interjections: a bounded, deduplicated per-turn queue (`TurnDriver::interject`).
  Items are delivered before the next model call as `[user interjection] …`,
  never during a stream or tool round. `InterjectionQueued`,
  `InterjectionDelivered` and `InterjectionDropped` audit the lifecycle; an
  item that misses the last safe point is returned in
  `TurnOutcome::late_interjections`.
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
- The deny list is split in two. A structural denial (`.keel`, operating-system
  areas, storage devices) cannot be approved away. A credential name is
  refused outside every root and asks for confirmation inside a root; a
  session path grant opens it, a full-auto escalation does not. `.env.example`
  and its template siblings are ordinary files, so the documented first-run
  flow works.
- `shell` no longer rejects a program outside its standard list before the
  gate; the permission request marks it as non-standard and the execution
  confirmation decides. The list labels, it does not veto.
- `Event` implements `Serialize` with snake_case keys, so a host can persist
  the kernel event stream as structured data.
- Versioned session snapshots (`SNAPSHOT_VERSION = 1`); a newer format is
  refused rather than misread.
- Bounded command execution: both pipes are streamed into a head and tail
  (64 KiB per stream) instead of being buffered in full; an inactivity window
  or cancellation kills the process tree; `read_file` reads at most 8 MiB and
  appends a size marker instead of refusing a larger file.
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

- `provider = "anthropic-compatible"` selects `keel-model-anthropic`;
  `variants/coding/profile.anthropic.toml` targets `claude-sonnet-5-5` and
  `ANTHROPIC_API_KEY`. `.env.example` lists the key.
- Credentials resolve from the profile's `api_key_env` first, then its
  `api_key_keyring` entry in the OS keyring. `--keyring-set service:account`
  reads a secret from stdin and `--keyring-delete` removes it; the secret is
  never printed. Windows uses Credential Manager, macOS the Keychain, Linux
  the kernel keyring.
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
  overflows. The collapsed summary reports the number of calls the gate
  blocked, so a denied write is not invisible after the turn.
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
- TUI: `Enter` while a turn runs queues an interjection instead of rejecting
  the input; `/cancel` still cancels, and a late interjection is shown as a
  notice after the turn. The line modes still read between turns.
- TUI: a finished turn collapses its tool lines into the aggregate line;
  `Ctrl-O` or `/details` reveals or hides the process for the whole
  conversation. Tool output stays bounded and redacted.
- TUI: Chinese and English chrome, chosen from the shell locale or the OS UI
  language at startup; `--lang auto|zh|en` overrides. The model channel and
  tool names stay English.
- TUI: a cancelled, failed or empty turn ends with one explicit line, and
  revealing details moves the viewport to the last turn's process instead of
  leaving it off-screen.
- TUI: the status line keeps the workspace base name and `+N` for extra
  scopes; `/roots` still prints the full paths and access levels.
- TUI: `/add-dir <path> [ro|rw]` adds a session scope through
  `TurnDriver::add_root`; relative paths resolve against the workspace and a
  scope survives a full-auto toggle, unlike a grant.
- TUI: the input box grows up to six content lines and scrolls around the
  cursor; a sent message longer than eight lines collapses to its head until
  `Ctrl-O`. Running tools are yellow, the status dot is green when idle.
- TUI: tool calls read as one block under a shared gutter (`✓`/`✗`/`…`) with
  nested `in` and `out` lines; assistant text renders fenced code blocks in a
  rounded box with the language on the border, aligned Markdown tables,
  quotes, rules, inline code, bold and bullets. `ToolStarted` carries a
  bounded, redacted `args_preview` that is never persisted.
- A finished turn with open task items shows the remaining count and
  pre-fills the prompt in the TUI; the host never restarts a turn by itself.
- `--help`/`-h` prints usage and `--version`/`-V` prints the host version
  before argument parsing.
- CI runs the format, lint and test gates on Ubuntu, Windows and macOS, and
  the kernel-only tests plus the `minimal_turn` example on each platform;
  `cargo deny` stays in a separate Linux job.

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
- `crates/keel-runtime/examples/long_task.rs` rehearses a sixty-file task in
  one turn with the mock: 62 model calls, 60 tool calls, one continuation.
- `crates/keel-runtime/examples/behaviors.rs` verifies eleven runtime
  behaviors against the mock provider with no network and prints a pass/fail
  summary: continuation, token budget, command activity windows, the
  permission matrix, file truncation, the program marker and the parallel-read
  cap.

### Evidence

- `consumer-app`: a working `keel-notes` consumer crate built by the agent in
  one turn inside an empty workspace — path dependencies on the three kernel
  crates, a `ask` command over the mock provider, two passing unit tests —
  25 model calls, 43 tool calls, 71 s, 94.5% cache hit.
- `novel-6`: a six-chapter Chinese novella (8,069 characters) written in one
  turn with `deepseek-flash`: 19 model calls, 36 tool calls, 156 s, 96% cache
  hit. A first run with a 16k working set overflowed while re-emitting the
  whole book; 24k and no full-book re-emission passed.
- `long-refactor`: twenty service modules updated in one turn. A real
  `deepseek-flash` run finished in 18-26 s with 11-13 model calls, 49-52 tool
  calls and up to ten tools in one round; both hidden tests passed. The case
  declares mechanism thresholds, and `run_case.py` fails it when the tool
  loop, parallelism, permission decisions or the turn budget deviate.
- 50-chapter novel: 57/57 turns, 0 overflow, 78.9% cache hit.
- 20 curated coding cases: 20/20.
- Five companion personas, 20 turns each: 100/100.
- Kernel contract tests, including the full event order for a tool turn, and
  301 Rust tests (254 in the kernel crates).
