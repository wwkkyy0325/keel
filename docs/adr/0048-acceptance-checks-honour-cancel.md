# ADR 0048 — Acceptance checks honour the turn's cancel token

## Status

Accepted and implemented in `keel-runtime`. Released as 0.2.7.

## Context

A turn that has written its answer moves to the acceptance plan. The verifier
created its own `CancelToken`, so the host's cancel reached the driver but not
the checks: clicking cancel during "verifying" changed nothing until every
check finished. A long test command could keep the button inert for minutes.

The model loop, the tool round and the shell already observe the token; the
verification phase was the gap.

## Decision

1. `AcceptanceVerifier::verify` takes the turn's `CancelToken`. Before each
   item it checks the token; when it is set, the remaining items are reported
   `Unverified` with detail "turn was cancelled" and are not run.
2. A running command check is passed the same token, so `command::run` stops
   it through the existing select.
3. After the report, the driver returns `KernelError::Cancelled { stage:
   "verify" }`; a cancelled turn is still not written to the kernel history,
   which is unchanged.

## Consequences

- Cancel during verification stops the current check and skips the rest
  instead of waiting for the whole plan.
- The report for a cancelled turn is discarded with the turn; its evidence
  does not claim a check ran.
