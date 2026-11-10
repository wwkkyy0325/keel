# ADR 0046 — An inferred close is not terminal

## Status

Accepted and implemented in `keel-core`. Released as 0.2.5.

## Context

ADR 0040 gave the list a fresh start at the next user turn, but kept closing
terminal within a turn. That rule assumes a close is a decision. It usually
is not: `TodoList::refresh_state` closes the list as soon as the last item
becomes terminal.

A live turn did exactly that. It tracked two items, completed the second and
then the first; the list closed itself. The model then tried to add the
deliverable document task and to re-mark the first item, and both calls were
rejected with `todo list is closed` in under a millisecond. The model never
asked to close anything; it lost the list because it finished its plan.

## Decision

1. `TodoList` records how it closed. `refresh_state` sets an
   `auto_closed` flag when the last item becomes terminal; the flag is
   private, serialized with the list and `#[serde(default)]`, so snapshots
   written before this change load as opened.
2. `add` (model or host) and `update_status` reopen a list whose close was
   inferred: the state returns to `Active`, the flag clears, and the call
   applies. An update may close the list again when every item is still
   terminal.
3. An explicit `close` clears the flag, including when it confirms the
   outcome the list already inferred; from then on the list is terminal until
   the next turn reset (ADR 0040).
4. A locked host plan is never reopened implicitly, and `remove` still
   refuses a closed list.

## Consequences

- A model that finishes its plan and then finds one more task keeps tracking
  it instead of dropping the list for the rest of the turn.
- Closing is terminal when someone chose it, which is what the rejection
  message should mean.
- The resume-at-next-turn behavior from ADR 0040 is unchanged.
