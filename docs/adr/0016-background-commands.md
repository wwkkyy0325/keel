# ADR 0016 — A background command returns a handle and delivers its result at a safe point

## Status

Accepted and implemented. The activity-based timeouts in
`docs/llmwiki/tasks/activity-budget.md` were the prerequisite: without them a
long command is still killed by a clock.

Implementation notes: the tool passes `background = "true"` because the kernel
tool parameter model has no boolean kind yet. `LocalBackgroundTasks` holds at
most four commands, kills the process tree on drop and at the two-hour cap,
and caps a command at two hours. The result enters as a user-facing
`[background <id> finished]` message at the top of the model loop;
`BackgroundStarted` and `BackgroundResult` record the audit events.

## Context

A tool call blocks the turn until it returns. A ten-minute build, a test suite
or a download therefore holds the model and the user: the model cannot start
the next independent step, and the user cannot get an answer until the command
finishes. Every tool is foreground by construction.

The turn already has safe points: the top of the model loop, where queued
interjections are delivered. A result that arrives while a turn is running can
enter the same way. A result that arrives after the turn ended can wait for the
next turn.

## Decision

1. **`shell` gains `background`.** The default stays foreground. With
   `background = true` the tool starts the process, redirects both streams to a
   file under the outputs directory, records the pid, the process group and
   the start time, and returns
   `started bg-3 (pid 4242); output: <absolute path>`. It does not wait.
2. **The host owns the registry.** One `BackgroundTasks` implementation holds
   the child handle, the tree guard, the output path and the status. At most
   four commands run at once per session; the fifth request fails with a
   message that names the limit. Cancel and session exit kill a whole tree.
3. **The kernel gets a port, not a process.** `BackgroundTasks` exposes
   `drain(&self) -> Vec<BackgroundResult>`; the driver holds it in
   `TurnDriverConfig`. The driver drains at the same safe point as
   interjections and once before the turn ends, and injects each result as a
   message:
   `[background bg-3 finished] exit code 0 after 402s; output: <path>` plus the
   last forty lines. The full output stays readable through `read_file`.
4. **A late result waits for the next turn.** If no turn is running, the result
   is held and delivered when the next turn starts. A host may instead start a
   turn for it and surface it in its UI.
5. **Liveness is unchanged.** The process is subject to the same inactivity
   window and two-hour cap; a killed tree still produces a `killed` result.
6. **Snapshots record handles, not processes.** After a restart a recorded
   handle is reported as `unknown`; the kernel never pretends a process
   survived.
7. **Audit.** `BackgroundStarted` and `BackgroundResult` carry the handle, the
   program, the exit status and the duration. The output file path is recorded;
   the content is not copied into the event stream.

## Consequences

- The model starts a build and keeps reading or editing while it runs; the
  result arrives at the next safe point and the turn continues.
- The user can talk while the work runs; an interjection and a background
  result share one delivery mechanism and one order.
- A host must reap processes on exit and enforce the per-session limit.
- The turn is no longer one strict request/response chain: a result can enter
  between rounds. The message list stays valid because delivery happens before
  a request is built, never inside a stream.
- Tests: a background command returns immediately, the result is delivered
  before the next model call, a late result waits for the next turn, the limit
  is enforced, cancel kills the tree, and a restored session reports the
  handle as unknown.

## Alternatives rejected

- **A `check_task` tool the model polls.** Costs a round per check and the
  model forgets; the host already knows when the process exits.
- **Async tool ids in the provider protocol.** Every adapter would change and
  providers that require a result per tool call would break.
- **Daemonizing without delivery.** The model never learns the outcome, and
  the user sees a side effect with no explanation.
- **Unbounded background tasks.** A model can start work faster than the
  machine finishes it; four is a small, understandable ceiling.
