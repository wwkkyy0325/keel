# ADR 0040 — A closed task list resets at the next turn

## Status

Accepted and implemented in `keel-runtime`. Released as 0.2.2.

## Context

The session keeps one task list. `TodoList::push` rejects an add once the
list is closed (`Completed`, `Failed` or `Abandoned`), and `close` is
terminal; nothing reopens it. A session outlives a single task, so the next
user request could not use the todo tool at all.

A live session showed it: after a completed list, the model added four tasks
and every call failed with `invalid_arguments` (`todo list is closed`) in
under a millisecond. The task list was unusable for the rest of the session.

## Decision

1. At the start of every user turn, if the task list is closed and not
   locked by the host, reset it: clear the items, set the state to `Active`,
   and bump the revision. The id sequence stays monotonic, so an event
   consumer never sees two items with the same id.
2. Emit `TodoUpdated` for the reset with the new revision and an empty list,
   so a host that mirrors the list drops the finished one.
3. A host lock (`locked`) is never cleared by the reset, and a list that
   closes in the middle of a turn still rejects further adds: closing is
   still terminal within a turn.

## Consequences

- Every user task starts with a usable list; the previous list survives in
  the turn's events and its record.
- The host does not need a per-turn hook; any host that restores a session
  gets the same behavior.
- A list closed by plan mode while locked stays closed until the host
  unlocks it, which is the host's decision to make.
