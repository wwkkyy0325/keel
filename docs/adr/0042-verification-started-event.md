# ADR 0042 — Verification announces that it started

## Status

Accepted and implemented in `keel-runtime`. Released as 0.2.2.

## Context

An acceptance plan runs after the model's final answer, inside the same turn.
While the checks run there is no model stream and no tool call, so a host
sees nothing between the last answer text and `verification_finished`. In a
live session the answer looked complete but the turn stayed in the running
state — the user described it as "stuck" — until verification ended.

Making verification asynchronous would change what a turn means: the
`Done`/`VerificationFailed` outcome is decided by the checks, and a host
would have to show a turn that later revokes itself.

## Decision

1. Emit `VerificationStarted { turn_id, checks }` immediately before the
   verifier runs, and only when the plan selected at least one item.
2. The event is additive: `EVENT_FORMAT_VERSION` stays 1, and a host that
   ignores it behaves as before. The `keel-testkit` event-name contract list
   gains the name.

## Consequences

- A host can show a waiting state (`验证中…` in Keelson) for the gap between
  the answer and the verification result.
- The turn remains synchronous; its outcome still includes verification.
- Telemetry can measure verification time separately from model time.
