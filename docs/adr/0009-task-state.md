# ADR 0009 — Task state is kernel-owned and tool-edited

## Status

Accepted.

## Context

The novel pilot put the work plan in `state/continuity.md`, a free-form file
written by the model. After a few chapters the model added its own "gap list",
then followed that list instead of the prompt's chapter number. Task state and
story state had no boundary, so the plan contaminated the facts and drifted.

A todo list can fail in two opposite ways:

- a pure tool that writes a file leaves the state outside the kernel; it is not
  rendered every turn and can be rewritten without limit;
- a purely internal plan that the model cannot edit is too rigid for open-ended
  work.

The kernel needs a typed task list that is always visible and has a narrow
editing surface.

## Decision

### Data

- `TodoList { items, revision, locked }` lives in kernel-owned session state.
- `TodoItem { id, text, status, source }`.
- `TodoStatus`: `pending`, `in_progress`, `completed`.
- `TodoSource`: `host` or `model`.
- Limits: 100 items, 200 characters per item, at most one `in_progress`.

### Host and model

- The host seeds items with `add_host`; they are immutable in text and cannot
  be removed.
- A host-seeded list may be locked. A locked list rejects `add` and `remove`
  from the model; only the current item may change status. Completing or
  starting a later item is rejected with `OutOfOrder`, so a fixed plan cannot
  be completed out of order. This is how a fifty-chapter plan survives a
  failed chapter instead of silently skipping it.
- An unlocked list lets the model add and remove its own items.

### Tool

- One kernel tool, `todo`, with four operations: `list`, `add` (text), `update`
  (id, status), `remove` (id).
- The tool returns the rendered list, so the model sees the current state after
  every edit.
- The tool changes no files and needs no permission request. It is
  `ReadOnly` for visibility, which keeps it available in plan mode.
- The kernel validates every mutation; a rejected edit is a typed tool error,
  not a silent change.
- The first pending item is the current task when no item is explicitly
  `in_progress`. The model therefore marks only completion, one tool round per
  task instead of two. The `[tasks]` block carries a `current: #id` line.

### List lifecycle

- `TodoState`: `active`, `completed`, `failed`, `abandoned`.
- Completing the last open item closes the list as `completed`. If every item
  is terminal and at least one failed, it closes as `failed`. An unfinished
  plan can be closed explicitly with `todo op=close` and an outcome.
- `completed` requires every item to be completed. `failed` and `abandoned`
  may close with open items, so a stuck plan has a terminal state too.
- A closed list rejects add, remove and status changes. It renders as one
  summary line, plus failed item lines, instead of the full list.
- `TodoClosed` reports the outcome and counts; the CLI prints
  `tasks: closed=… completed=… failed=… total=…`.

### Context

- A `task_state` stage, priority between `memory` and `history`, renders
  `[tasks]` with ids and status markers (`[ ]`, `[>]`, `[x]`, `[!]`).
- Task state is pinned: it is not summarized by the memo, not folded into
  facts and not recalled. It is small and directly relevant to the next action.
- The stage emits nothing when the list is empty.

### Observability

- `TodoUpdated { revision, items, in_progress }` is emitted after a tool call
  changes the list.
- The CLI prints `tasks: done=… current=…#id total=…` after every turn.
- Storage maps to a `todos` table in ADR 0008; persistence remains M5.

## Consequences

- A fixed host plan cannot be rewritten into a gap list by the model.
- Story state and task state have separate homes and separate lifetimes.
- The model can still create its own tasks when the host leaves the list
  unlocked; the kernel does not plan for it.
- The list is bounded, so it cannot become a second transcript.

## Alternatives rejected

- A free-form `plan.md` written by the model: the pilot already showed it
  drifts and is not guaranteed to be read.
- State only in the system prompt: no revision history and no typed limits.
- A full scheduler in the kernel: ordering and dependencies are policy, not a
  kernel invariant; this version only stores and renders tasks.
