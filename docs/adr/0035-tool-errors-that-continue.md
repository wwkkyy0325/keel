# ADR 0035 — A tool error says how to continue

## Status

Accepted and implemented in `keel-core` and `keel-runtime`. Released as
0.1.1.

## Context

A live turn against a real project made 47 tool calls; four failed. Three
were model mistakes the model repaired on its own: an unknown argument on
`todo`, mismatched parallel lists on `apply_edits`, and `close` while one
item was still open. The fourth was not a model mistake: the list closes
itself when its last item becomes terminal, so the explicit `close` that
followed rejected with `todo list is closed` even though the requested
outcome was already true.

Every rejected call costs a full model round carrying the whole context. The
error text is the cheapest place to shorten the correction.

## Decision

1. `TodoList::close` is idempotent for the state the list already holds.
   The call succeeds and returns the rendered list. A conflicting close is
   rejected with the state in the message (`todo list is already completed`)
   through `TodoError::AlreadyClosed(TodoState)`.
2. A rejected `close ... completed` names the items that still block it
   (`not every item is completed: #2 <text>`), not only the rule.
3. `ToolSchema::validate` names the arguments the tool accepts when it
   rejects an unknown one (`unknown argument 'op2'; accepted: op, id, text,
   status, outcome`). Strictness is unchanged: a typo still never changes
   behavior.

## Consequences

- A finished plan no longer ends with a false failure, and the model can fix
  the other two classes in its next call without a listing round.
- Error strings grow by one short clause; the tool result budget is not
  affected in practice.
- `TodoError` gained a variant that carries the state, so an exhaustive match
  outside the kernel must handle it.
