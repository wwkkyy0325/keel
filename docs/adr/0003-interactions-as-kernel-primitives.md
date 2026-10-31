# ADR 0003 — Interactions as kernel primitives

## Status

Accepted.

## Context

Questions, plan approval, permission confirmation and escalation are all
control-plane actions. Modelling them as domain tools pollutes the model's
tool list, scatters UI logic across tools and makes timeout/deferral behavior
inconsistent. They also need a single audit path.

## Decision

1. Interactions are typed kernel primitives:
   `Clarify`, `Choice`, `PlanApproval`, `PermissionApproval` and `Escalation`.
2. Model providers map their native mechanism to
   `ModelChunk::Interaction(InteractionRequest)`. The wire adapter may use a
   reserved control primitive or structured output; it is not a domain tool.
   The visible tool list stays domain-only.
3. An `InteractionPort` is host-provided and returns an
   `InteractionResponse`. The kernel's `InteractionBroker` applies the
   `InteractionPolicy`, enforces the deadline and writes audit events.
4. One interaction may be pending per turn. The kernel assigns the interaction
   id and turn id; the model cannot forge either. The model can only request,
   never approve or escalate.
5. The turn FSM gains `WaitInteraction`. Every interaction has a deadline and
   respects cancellation.
6. Timeout policies are `block`, `defer_and_continue`, `end_turn` and `fail`.
   Interactive hosts default to `block`; automation defaults to
   `defer_and_continue`.
7. On deferral the kernel records the id, injects a "do not assume an answer;
   continue independent work" notice, and reports deferred items in
   `TurnReport`. The host persists deferred items and feeds answers back later.
8. The host composes the frontend from `InteractionRequested` events and sends
   typed responses. The kernel never knows the UI.

## Consequences

- Permission confirmation and plan approval share one path.
- Plan mode becomes tool visibility plus a plan interaction, not a permission
  level.
- M1 implements in-memory pause/resume and a deferring default port. Persistent
  turn snapshots and multi-device routing are later work.
- Interaction ids, response validation, size limits and one-pending-per-turn
  are part of the security boundary.
