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
`SubAgentsPending`, `UnverifiedHard`, `VerifyCapReached`, `StreamFailure`,
`Overflow`, `VerifiedStalled`, `BudgetConverge`, `BudgetCommit`,
`TurnsExhausted`, `WallClockLow`, `InteractionPending`, `InteractionResolved`,
`InteractionDeferred`, `InteractionEndTurn`, `Cancel`, and `Fail`. `Fail` is a
kernel-internal event accepted from any non-terminal state; it guarantees that
an unexpected error still reaches a terminal state.

Every transition has a test. New states require an ADR.

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
- `workspace_auto` is the logical sandbox: actions inside the workspace run
  automatically; outside actions require confirmation.
- `full_auto` removes the workspace boundary but never the hard deny list.
- The deny list covers system/privileged paths, credential stores, secret
  files, other users' homes, privilege escalation, destructive storage
  commands, security bypass, Keel audit/`.git` internals and
  send/payment/secret upload. It is hardcoded plus host additions; profiles
  and plugins cannot remove entries.
- `read_only` is tool visibility, not a level. `manual` review is a
  host-level confirmation wrapper, not a kernel level.
- Plan mode is a tool visibility filter: it hides write/execute tools and does
  not lower permissions.
- Confirmations and escalation are typed interactions. The user may allow one
  call, allow a path prefix, or escalate the session to `full_auto`. Path
  grants and escalation carry a TTL and can be revoked; only the user or host
  approves, and grants never bypass the deny list.
- Deletion is soft: the target moves to a `TrashStore`. Overwrite backs up the
  previous version first. A failed trash operation aborts; there is no hard
  delete fallback. The agent never empties or restores the trash.
- Path scan covers absolute paths, traversal, deny patterns and later symlink
  and admin hardening.
- Audit is always on; it is not sampled and not optional.

## Secrets

- Hosts read credentials from `.env` or the OS keyring. Profiles reference env
  or keyring names, never inline values.
- The agent never asks the user to paste a key into chat; it points to
  `.env.example` or the keyring.
- Input detected as a secret produces a warning and is not blocked. Detection
  is best-effort and false positives (test keys, hashes, config examples) must
  not make the host unusable.
- The event log redacts detected secrets before append.
- Secrets never appear in code, logs, tests, prompts or git.

## Model wire

- OpenAI-compatible and Anthropic-compatible providers.
- Streaming first; non-streaming is a compatibility path.
- Provider config: base URL, key, model, context window, max tokens, reasoning
  dialect.
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
