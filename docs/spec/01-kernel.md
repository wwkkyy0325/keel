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

Auxiliary states: `Stalled`, `Escalating`, `WaitSubAgents`, `TerminalCommit`.

Events include: `Ready`, `ToolCalls`, `DoneSignal`, `ThinkingOnly`,
`SubAgentsPending`, `UnverifiedHard`, `VerifyCapReached`, `StreamFailure`,
`Overflow`, `VerifiedStalled`, `BudgetConverge`, `BudgetCommit`,
`TurnsExhausted`, `WallClockLow`, `Cancel`.

Every transition has a test. New states require an ADR.

## Ports

```rust
pub trait ModelProvider {
    async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError>;
}

pub trait Tool {
    fn schema(&self) -> ToolSchema;
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
    fn model_route(&self, role: Role) -> ModelRoute;
    fn before_commit(&self, ctx: &CommitCtx) -> CommitDecision;
}
```

The kernel asks `Policy` at defined points and receives data. Policy
implementations do not receive mutable kernel state.

## Tool protocol

- Every tool has a stable name, JSON schema, risk level and description.
- Arguments and results are typed at the kernel boundary.
- Read-only tool calls may run concurrently; mutating calls are serialized.
- A tool failure returns a typed error envelope; it never aborts the turn by
  itself.
- Every call is audited with name, duration, status and error code.
- Tool visibility is a policy decision, not a registry decision.

## Context assembly

- Stages are ordered by priority; equal priorities stable-sort by registration.
- A stage returns at most one fragment.
- The kernel enforces the token budget after assembly.
- Compaction is a ladder: trim -> rolling summary -> LLM compact -> task
  boundary.
- The profile selects which stages are active and the budget.
- Stage order and prompt text are policy, not kernel logic.

## Permissions

- Four levels: read-only, manual, semi-auto, full-auto.
- Path scan covers absolute paths, traversal, denylist and admin patterns.
- The session workspace root is a trusted free zone; everything outside still
  gates.
- Confirmations are typed requests with a reason and a scope.
- Audit is always on; it is not sampled and not optional.
- Plan mode is orthogonal: it filters write tools, it does not lower
  permissions.

## Model wire

- OpenAI-compatible and Anthropic-compatible providers.
- Streaming first; non-streaming is a compatibility path.
- Provider config: base URL, key, model, context window, max tokens, reasoning
  dialect.
- Role routing: main, compact, subagent, verifier, commit.
- Retries are bounded; circuit breaking is per provider.
- Token usage is recorded per call.

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
- Plugin register/unregister and denied capability escalation.
- Model 500, stream cut, overflow and cancellation.
- Kernel overhead benchmark within budget.
