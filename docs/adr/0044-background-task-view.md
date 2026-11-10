# ADR 0044 — A host can read background tasks without draining them

## Status

Accepted and implemented in `keel-runtime`. Released as 0.2.2.

## Context

A background command is session-scoped, but its result is consumed by the
driver's `drain`, which injects it before the next model call. A host had no
way to show a task that outlived the turn that started it: the live view
cleared with the turn, and the result appeared only when a later turn drained
it. The task either vanished from the screen while still running, or a
finished result sat invisible until the user typed again.

Draining from the host would be wrong: the model would never see the result.

## Decision

1. `LocalBackgroundTasks::snapshot` reports every task in the registry —
   running, or finished and not yet drained — with id, program, pid, running
   flag, exit code, elapsed time and output path. It does not consume
   anything; `drain` remains the driver's.
2. The trait `BackgroundTasks` is unchanged, so other backends and test
   doubles are unaffected. A host that uses `LocalBackgroundTasks` can expose
   the view; Keelson adds a `background_tasks` command and polls it while a
   row is visible.
3. A drained task disappears from the snapshot, which is the signal that the
   model now owns the result.

## Consequences

- A host can keep a task visible across turns without stealing the result.
- Reading is cheap (a mutex and a `try_wait`); polling once a second while a
  row is shown is enough.
- The snapshot cannot show a result after the model has consumed it. That is
  intentional: the result is in the conversation from then on.
