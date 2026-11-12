# ADR 0056 — A background command can be stopped

## Status

Accepted.

## Context

A background command is session-scoped (ADR 0016) and a host can show it
(ADR 0044). Nothing could end it: a dev server or a watcher ran until the
whole registry was dropped at application exit, or until the two-hour hard
cap. A user who started a preview server had no way to stop it from the
session, and a stopped turn could leave a process behind deliberately.

Stopping must not hide the command from the model: the driver consumes every
background result at the next safe point, and a command that vanished from
the registry would be a silent gap in the turn's history.

## Decision

1. `LocalBackgroundTasks::stop("bg-N")` kills the process tree with the same
   `kill_tree` used at shutdown and returns whether a running task was found.
   The task stays registered, so `drain` still reports how it ended and the
   model still receives the result.
2. The `BackgroundTasks` trait is unchanged. A host that exposes its own
   registry keeps its own stop policy.
3. Keelson adds a `stop_background` command and a Stop button on each running
   row. The next poll marks the row finished; its exit code is whatever the
   killed process reported.

## Consequences

- A preview server, a watch loop or a stray daemon can be ended without
  closing the application.
- A stopped command is not a successful command: its exit code is not zero,
  and the model's background result says it terminated.
- Stopping is not undoable. Restarting is a new `background` tool call.

## Alternatives rejected

- A kill on turn cancellation: the point of a background command is to
  outlive the turn.
- Removing the task from the registry on stop: the model would never see the
  result, which contradicts ADR 0016.
- Exposing a generic process kill through the shell tool: it already exists,
  but it cannot address the registry handle and would hide a result the
  driver still owns.
