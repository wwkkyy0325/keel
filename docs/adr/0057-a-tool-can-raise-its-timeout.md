# ADR 0057 — A tool can raise its own timeout

## Status

Accepted.

## Context

The dispatcher applies the session's `tool_timeout` (300 seconds by default)
to every call. That is right for a read, a write or a command that should not
hang, but it is wrong for a tool that legitimately runs for minutes: a live
`render_scene` call rendering 413 frames of 1080p canvas wrote 225 frames and
was killed with `tool call exceeded 300000 ms`. Raising the global limit
would let every hung tool run for minutes, and giving one rig a larger limit
only moves the problem to the next rig.

The call knows what it needs; the session does not.

## Decision

1. `Tool` gains a default method `timeout() -> Option<Duration>`. The default
   is `None`, meaning the session's limit.
2. The dispatcher enforces `max(session_limit, tool.timeout())` for that
   call. A tool can extend its own ceiling, never shorten the session's.
3. A tool that declares a longer timeout is still bounded: the value is a
   ceiling for one call, not a promise. Cancellation and the turn's wall
   clock still end it.
4. Hosts that need a longer render override `timeout` in the tool itself;
   profiles and rigs do not carry per-tool limits.

## Consequences

- `render_scene` and `render_film` declare 40 minutes; their internal render
  budget (60 seconds plus one second per frame, capped at 30 minutes) fires
  first, so the outer timeout is only a backstop.
- A hung tool that does not declare a longer timeout keeps the session's
  limit. A tool that does declare one can hold its slot (and a write round)
  for that long; the host's cancel button and the turn deadline still end it.
- The dispatcher's concurrency batching is unchanged: a read-only tool with a
  long timeout still runs in the read batch.

## Alternatives rejected

- Raising `tool_timeout` globally: a hung `shell` or `read_file` would take
  the new limit to fail, and the limit is part of the kernel contract.
- Per-rig limits: the explainer rig would get long tools, but any rig that
  enables the render tools would need the same edit, and a rig is a policy,
  not a property of a tool.
- Rendering in the background like the `shell` background mode: the browser,
  the loopback sink and the frame files are one unit; making it a registry
  task is a much larger change for the same outcome.
