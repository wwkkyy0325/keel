# ADR 0017 — Sub-agent dispatch is a host runner behind a kernel tool

## Status

Accepted and implemented. First implementation: `keel-runtime::subagent` and
`hosts/keel-cli/src/main.rs` (`HostAgentRunner`).

## Context

A side investigation should not fill the parent's context: reading twenty
files to answer one question costs the parent the whole transcript. The
kernel already has everything a child needs — a driver, a registry, a
permission gate, a cancellation token — but nothing decides when to delegate,
which model to use, how much the child may spend, or how its result returns.

The question was whether dispatch belongs in the kernel. It does not: depth
policy, model choice, result shaping and parallelism are product decisions.
The kernel should only guarantee that a child cannot escape the parent's
permissions, cancellation or audit.

## Decision

1. **The contract is a kernel tool.** `spawn_agent { task, role? }` is a
   `Tool` in `keel-runtime`. It takes a `SubAgentRunner` and returns one
   bounded report with the child's usage and state.
2. **Execution is a host runner.** The reference host builds a child driver
   with the same registry, permission engine and interaction broker, a fresh
   context (`system/task_state/memory/history/latest`), its own event log and
   lowered limits. The child never inherits the parent's messages.
3. **Isolation by default.** Only the final answer, the state and the usage
   return. The child transcript stays in the child event log and is discarded;
   the parent records the call in its own `ToolStarted`/`ToolFinished` events.
4. **No nesting.** A per-runner depth counter refuses a child that calls
   `spawn_agent` again. A future host may raise this deliberately.
5. **Serialized for now.** The tool is `Mutating`, so the dispatcher runs one
   spawn at a time. Parallel spawns need a concurrency class the dispatcher
   does not have yet.
6. **Permissions are shared, never widened.** The child's tool calls pass the
   same gate and the same interaction broker; a prompt goes to the same user.
   Cancellation is the parent's `CancelToken`, so cancelling the parent cancels
   the child's model call.
7. **Usage is reported, not hidden.** The tool result carries model calls,
   tool calls and input/output tokens. The parent's turn budget does not yet
   include them; the report is the accounting, and a later change may add the
   child usage to the parent's counter.
8. **Errors are surfaced.** A failed child reports its state and the error
   text; the tool does not pretend success.

## Consequences

- The parent sees one compact report instead of a transcript, and can ask a
  follow-up or continue its own work.
- The child pays for its own context: its tokens are outside the parent's
  budget until the counter is extended.
- Parallel sub-agents are not available; the dispatcher's risk classes decide
  that. A `Concurrent` class (side effects, safe to overlap) is the next step.
- The TUI does not render sub-agent progress; the tool line shows the call and
  the report. A host may add a per-agent view.
- A model can waste tokens delegating a trivial task; the tool description
  says to use it for a side investigation, not for work that needs the parent
  conversation.

## Alternatives rejected

- **Dispatch inside the driver.** It would put recursion, model choice and
  result shaping in the kernel, and every variant would need an edit.
- **A second process per sub-agent.** Isolation is stronger, but startup and
  packaging cost dominate for a personal tool; the same port can host a
  process backend later.
- **Sharing the parent context.** It defeats the purpose: the child would cost
  the same tokens as doing the work inline.
- **Letting the child spawn freely.** Unbounded cost and recursion; a depth
  counter is cheap and explicit.
