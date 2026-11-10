# ADR 0043 — A shell call may set its own wall-clock bound

## Status

Accepted and implemented in `keel-runtime`. Released as 0.2.2.

## Context

A command was bounded by two things: an inactivity window (300 s by default;
any output resets it) and a two-hour hard cap. Neither is a wall-clock limit a
model can choose. A live session showed the gap: the model tried
`timeout_ms=600000` on a long test run and got `invalid_arguments`, because
the shell tool has no such argument. It retried without it, but the intent —
"this build may take ten minutes, do not cut it off, and do not let it run
forever" — had no expression in the tool.

The inactivity window is deliberately not that: it keeps a long build alive
and only kills a silent hang.

## Decision

1. `CommandSpec` gains `max_duration: Option<Duration>`, a wall-clock bound
   enforced alongside the inactivity window: both the foreground runner and
   `LocalBackgroundTasks` stop the command when it is reached.
2. The shell tool accepts `timeout_ms` (integer milliseconds). `0` or an
   absent value means no per-call bound; a value is clamped to two hours, so
   a model cannot ask for an unbounded command.
3. The tool description names `background` and `timeout_ms`, and the
   background path honors the same bound.

## Consequences

- A model that expects a long build can say so; a silent hang is still killed
  by the inactivity window, and a printing-forever command by the hard cap.
- The bound is per call, not per profile: a profile cannot raise it, so an
  unexpected value is visible in the call's arguments.
- Background commands share the limit; a task that exceeds it finishes with
  the timeout outcome and its output file still holds what it printed.
