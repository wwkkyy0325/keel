# 01 — Kernel contract

The kernel owns mechanisms and invariants. It never knows variant names.

## Invariants

1. A turn always reaches a terminal state: `Done`, `Error` or `Cancelled`.
2. Every tool call has a schema, a typed result and a stable error code.
3. Permissions are enforced in the kernel; prompts and plugins cannot bypass
   them.
4. Cancellation propagates to model streams, tools and background work.
5. The event log is append-only and sufficient to reconstruct a turn.
6. Context assembly is deterministic for the same inputs and profile.
7. Plugins can be added or removed without changing kernel code.
8. The kernel emits telemetry at fixed boundaries only; it never writes
   telemetry sinks synchronously on the hot path.

## Turn state machine

States:

`Init -> Observe -> Solve -> Act -> Verify -> Converge -> Done`

Terminal: `Done`, `Error`, `Cancelled`.

Auxiliary states: `Stalled`, `Escalating`, `WaitSubAgents`, `WaitInteraction`,
`TerminalCommit`.

Events include: `Ready`, `ToolCalls`, `DoneSignal`, `ThinkingOnly`,
`Continued`, `SubAgentsPending`, `UnverifiedHard`, `VerifyCapReached`,
`StreamFailure`, `Overflow`, `VerifiedStalled`, `BudgetConverge`,
`BudgetCommit`, `TurnsExhausted`, `WallClockLow`, `InteractionPending`,
`InteractionResolved`, `InteractionDeferred`, `InteractionEndTurn`, `Cancel`,
and `Fail`. `Fail` is a kernel-internal event accepted from any non-terminal
state; it guarantees that an unexpected error still reaches a terminal state.

Every transition has a test. New states require an ADR.

One turn finishes the task. The limits (500 model calls, the profile's tool
budget) are runaway protection, not a per-task allowance, and a host may lower
them for a batch job. Liveness is measured by activity, not by the clock: a
command is stopped after five minutes *without output*, not five minutes after
it starts, so a download or a build that keeps printing runs to completion; a
model stream is stopped after five minutes without a chunk. A 24-hour turn
deadline remains as a backstop. A per-turn input-token budget (one hundred
times the working set, never below one million) bounds cost: reaching it
forces the answer call. The number is host-side only — the model sees the same
generic wrap-up and final-call instructions, never a token count, so it does
not start optimising for a counter. When the model reports a partial result while
the `todo` list still has unfinished items, the driver keeps the turn alive: it
records `TurnContinued`, adds the partial text and a reminder to the message
list and runs another round. The continuation is bounded (six per turn) and
never happens after the wrap-up instruction or the reserved final call. A
model that needs a decision from the user asks and closes or leaves the list;
the kernel does not guess.

## Ports

```rust
pub trait ModelProvider {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError>;
}

pub trait Tool {
    fn schema(&self) -> ToolSchema;
    fn permission_requests(&self, args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError>;
    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult;
}

pub trait ContextStage {
    fn name(&self) -> &'static str;
    fn priority(&self) -> i32;
    async fn build(&self, ctx: &BuildCtx) -> Option<Fragment>;
}

pub trait MemoryStore {
    async fn recall(&self, query: &Query, limit: usize) -> Vec<MemoryHit>;
    async fn write(&self, record: MemoryRecord) -> Result<(), MemoryError>;
}

`MemoryStore` is a host port, not a kernel implementation. The kernel's
built-in memory is lexical and structured (segments, records, memo, scoped
facts; ADR 0012). Vector databases, temporal graphs, embedding services, LLM
extraction pipelines and MCP memory servers are adapters or plugins; they
return typed, bounded data and cannot bypass permissions, audit, cancellation
or the context budget. If an adapter fails, the kernel falls back to its own
recall and records.

pub trait Policy {
    fn visible_tools(&self, all: &[ToolSchema]) -> Vec<ToolSchema>;
    fn context_recipe(&self) -> ContextRecipe;
    fn thinking_budget(&self) -> u32;
    fn verification_mode(&self) -> VerificationMode;
    fn memory_policy(&self) -> MemoryPolicy;
    fn model_route(&self, role: ModelRole) -> ModelRoute;
    fn before_commit(&self, ctx: &CommitCtx) -> CommitDecision;
}
```

The kernel asks `Policy` at defined points and receives data. Policy
implementations do not receive mutable kernel state.

Interactions are a control-plane port, not a tool:

```rust
pub trait InteractionPort {
    async fn request(request: InteractionRequest, cancel: &CancelToken) -> InteractionResponse;
}
```

The kernel assigns interaction ids and turn ids. Model providers map a native
control primitive or structured output to `ModelChunk::Interaction`; the
domain tool list is unchanged.

Interjections are a separate host queue, not interactions: a user message that
arrives during a turn is delivered at a safe point (before a model call, after
a tool round, before the final call) and never during a stream or a tool call.
The kernel provides the bounded queue, safe-point delivery and audit events;
the host chooses `queue`, `interrupt` or `next_turn`. The reference TUI
implements `queue`; the line modes read between turns. Cancellation and
interjection stay distinct. See ADR 0010.

## Tool protocol

- Every tool has a stable name, JSON schema, risk level and description.
- Arguments and results are typed at the kernel boundary.
- Assistant tool-call messages carry structured calls; tool-result messages
  carry the originating call id.
- Read-only tool calls may run concurrently; mutating calls are serialized.
- A tool failure returns a typed error envelope; it never aborts the turn by
  itself.
- Every call is audited with name, duration, status and error code.
- Tool visibility is a policy decision, not a registry decision.
- A tool declares its permission requests; the dispatcher asks the permission
  gate before `call` runs. A tool never grants itself access.
- A tool declared `read_only` may only declare read access. The dispatcher
  rejects a write or execute request before the gate, because read-only calls
  run concurrently.
- In `plan_mode`, only read-only tools are visible; a hidden write tool is
  rejected even if the model calls it.

## Context assembly

- Stages are ordered by priority; equal priorities stable-sort by registration.
- A stage returns at most one fragment.
- The kernel enforces the token budget after assembly and re-checks the full
  message list before every model call. Overflow is a typed terminal error
  until the compaction ladder lands.
- Compaction is a ladder: trim -> rolling summary -> LLM compact -> task
  boundary.
- The profile selects which stages are active and the budget.
- Stage order and prompt text are policy, not kernel logic.

## Permissions

- Two levels: `workspace_auto` and `full_auto`.
- `workspace_auto` is the permission boundary: actions inside the workspace
  run automatically; outside actions require confirmation. It gates tool
  calls, not processes: an approved `shell` command or MCP server is not
  confined to the workspace.
- A host may add labeled roots outside the workspace. Each root is read-only
  or read-write; execution still requires confirmation, and the most specific
  root wins. A drive root or a system directory is refused as a root.
- `full_auto` removes the workspace boundary and every extra root, but never
  the structural deny list.
- The structural deny list is the agent's own state (`.keel`) and operating
  system areas (privileged paths, `/dev`, storage devices, `System32`). It is
  hardcoded plus host additions; profiles, plugins and approvals cannot remove
  an entry.
- Credential names (`.env` and `.env.*` except the templates, `.ssh`,
  `.gnupg`, `.aws`, `.azure`, `.kube`, `.docker`, `.git`, private-key
  suffixes, `credentials.json`, a `secrets` directory) are refused outside
  every root. Inside a declared root they return a confirmation that names the
  pattern; a session path grant opens them, a full-auto escalation does not.
  `.env.example`, `.env.sample`, `.env.template`, `.env.dist` and
  `.env.defaults` are ordinary files.
- `shell` runs a program without a shell. The standard developer list labels a
  program it does not contain ("not in the standard program list") but does
  not veto it: the execution confirmation is the decision point.
- `read_only` is tool visibility, not a level. `manual` review is a
  host-level confirmation wrapper, not a kernel level.
- Plan mode is a tool visibility filter: it hides write/execute tools and does
  not lower permissions.
- Confirmations and escalation are typed interactions. The user may allow one
  call, allow a path prefix, or escalate the session to `full_auto`. Path
  grants and escalation carry a TTL and can be revoked; only the user or host
  approves, and grants never bypass the structural list.
- Deletion is soft: the target is moved into a `TrashStore` when it is on the
  same volume, otherwise copied, then removed. Overwrite backs up the previous
  version first. A failed trash operation aborts; there is no hard delete
  fallback. The agent never empties the trash; a restore is itself backed up.
- Path scan covers absolute paths and traversal; `..` never climbs above the
  drive or filesystem root. Before the gate, the dispatcher canonicalizes the
  requested path (including a not-yet-existing write target, through its
  deepest existing ancestor), so a symlink inside the workspace cannot point
  the check outside it. A dangling symlink is refused.
- The configured outputs directory (default `<workspace>/.keel/outputs`) is
  readable so truncated tool output can be re-read; the rest of `.keel` stays
  denied, and the exception never permits a write. A host may move outputs
  outside the workspace, and the spill marker names the absolute path. The
  directory keeps the newest 200 spill files; older ones are pruned
  best-effort and unrelated files are not touched.
- This is a permission boundary, not an OS sandbox. Container/seatbelt/job
  confinement of commands and plugins is a host option, not a kernel claim.
- Audit is always on; it is not sampled and not optional.

## Background commands

- `shell` accepts `background = "true"`. The host starts the process, writes
  its output under the outputs directory and returns a handle immediately; the
  command does not hold the turn. A host may run at most four at once and kills
  the process tree when the session ends.
- At the top of the model loop the driver drains finished commands and adds
  `[background <id> finished] <program> exit code <n> after <ms>; output:
  <path>` plus the last forty lines. A result that arrives after the last call
  waits for the next turn; it is never injected into a stream.
- `BackgroundResult` is the audit event. The port is `BackgroundTasks`; a host
  that cannot run processes returns the error the tool surfaces.

## Sub-agents

- `spawn_agent { task, role? }` delegates one self-contained task. The kernel
  provides the tool and the `SubAgentRunner` port; the host provides the child
  driver, model choice, limits and result shaping (ADR 0017).
- The child gets a fresh context and never the parent's messages. Only the
  final answer, state and usage return, bounded; the child transcript stays in
  the child event log.
- The child shares the permission gate, the interaction broker and the parent
  cancellation token; it cannot widen permissions. A child that calls
  `spawn_agent` again is refused (depth one).
- The tool is `Mutating`, so one spawn runs at a time. The result is audited
  through the normal tool events; the usage is reported in the result and is
  not yet part of the parent's token budget.

## Secrets

- Hosts read credentials from `.env` or the OS keyring. Profiles reference env
  or keyring names, never inline values.
- The agent never asks the user to paste a key into chat; it points to
  `.env.example` or the keyring.
- A user may still hand over a credential to configure (an app id, a token, a
  key). That request is served, not refused: the agent writes it to the file
  the user named, does not repeat the value in the answer, and `.env` writes
  still pass the permission gate.
- Input detected as a secret is not blocked. The turn records a
  `SecretWarning` event and returns the detected kind with the turn result, so
  the host can show the notice for that turn. Detection is best-effort and
  false positives (test keys, hashes, config examples) must not make the host
  unusable.
- The event log redacts detected secrets before append.
- Secrets never appear in code, logs, tests, prompts or git.

## Model wire

- OpenAI-compatible and Anthropic-compatible providers.
- Streaming first; non-streaming is a compatibility path.
- Provider config: base URL, key, model, context window, max tokens, reasoning
  dialect.
- Thinking-mode providers may require `reasoning_content` to be echoed back on
  assistant tool-call messages; the kernel stores it and the adapter renders it.
- Reasoning effort is adaptive and kernel-owned (ADR 0005); it is not a model
  choice or a user-facing on/off switch.
- Role routing: main, compact, subagent, verifier, commit.
- Retries are bounded; circuit breaking is per provider.
- Token usage is recorded per call.
- Interactions arrive as `ModelChunk::Interaction`; they are never domain tool
  calls.
- A model response mixing tool calls and an interaction is rejected.
- The provider receives the cancellation token; dropping the stream is not the
  only cancellation signal.

## Plugin lifecycle

- A plugin declares contributions in a manifest: tools, context stages,
  channels, memory backends, model adapters, UI.
- The kernel validates the manifest, grants declared capabilities and registers
  contributions.
- Removing a plugin removes its contributions.
- Plugins cannot write audit, bypass permissions or mutate the event log.
- In-process plugins first; process isolation is a later milestone.

## The kernel does not

- Contain domain tools, prompts, personas or UI.
- Know variant names.
- Read files, call models or write telemetry sinks directly on the hot path.
- Own business logic.
- Hide failures behind retries without recording them.
- Contain vector stores, graph databases, embedding clients or memory-product
  extraction pipelines. Those are host ports and plugins (ADR 0012).

## Acceptance tests

- Every FSM transition, including cancellation and timeout.
- Tool argument validation and error envelopes.
- Concurrent read-only tools vs serialized writes.
- Context budget enforcement and each compaction step.
- Permission: traversal, denylist, admin, workspace boundary, audit.
- Dispatcher permission enforcement before the tool runs.
- Interaction request/resolve/defer/timeout/cancel/budget, and no self-approval.
- Soft delete: overwrite backup, trash failure abort, deny-list protection.
- Plugin register/unregister and denied capability escalation.
- Model 500, stream cut, overflow and cancellation.
- Kernel overhead benchmark within budget.
