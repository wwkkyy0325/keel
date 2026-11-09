# ADR 0032 — A check reports how long it took

## Status

Accepted and implemented in `AcceptanceVerifier`.

## Context

An acceptance report said whether a check passed but not what it cost. A host
deciding whether a full test suite is affordable on a workspace, or whether a
cheaper compile check would pay for itself, had no data; the only timing was
the whole turn, which mixes in model latency.

## Decision

1. A command or test item measures its own run.
2. The evidence detail ends with `[duration: 0.42 s]` on both the passing and
   the failing path.
3. Items that did not run (not allowlisted, spawn failure) carry no duration.
4. Compute and file-hash items are unchanged.

## Consequences

- The cost of each check is visible where the result is shown, including a
  failure.
- No event or schema change, so existing readers and stores are unaffected.
- The number is text, not a field: a host that needs machine-readable timing
  would have to parse it, and should add a field instead.
- Duration is wall clock, not CPU, and includes process startup.
