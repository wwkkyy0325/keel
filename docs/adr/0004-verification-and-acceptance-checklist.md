# ADR 0004 — Verification and the acceptance checklist

## Status

Accepted. Contract now; implementation follows with the coding profile.

## Context

A model can claim that a computation, test or file change succeeded without
doing it. Final text is not evidence. Verification must be executed by the
kernel or a verifier, not self-certified by the model.

## Decision

1. The plan carries an acceptance checklist:
   `AcceptanceItem { id, description, verify, required }` with `VerifySpec`
   variants `Compute`, `Command`, `Test`, `FileHash`, `Human` and
   `Unverifiable`.
2. The kernel runs verifiers and records typed `Evidence`. The model cannot set
   `verified`; only the verifier can. Missing evidence is `Unverified`.
3. Deterministic computations run through a safe arithmetic verifier, never a
   model guess and never shell interpolation.
4. Process verification runs after each plan step, plus a final turn report.
   Required failures end in `Error` after bounded reprompts; optional failures
   end in `Done` with warnings.
5. `PlanApproval` includes the checklist. Approval also pre-authorizes the
   declared verifier commands for that plan; the hard deny list still wins.
6. Deferring an interaction marks dependent acceptance items as `Deferred` and
   lets independent work continue.
7. The contract is defined now; M2 implements calculator, command, test and
   file-hash verifiers with the coding profile. M5 adds fault injection, human
   verification and eval reports.

## Consequences

- `TurnReport` carries the verification report; the model's narrative is shown
  separately from evidence.
- The verifier registry is a host capability; a plan referencing an
  unregistered verifier is rejected before approval.
- `Unverifiable` items require explicit user approval.
