# Interjections

ADR 0010 fixed the contract: a user message that arrives during a turn is
queued and delivered at a safe point, never injected into a stream. The
reference TUI now implements `queue` mode.

## Steps

- [x] `InterjectionQueue`: bounded (default four), deduplicated, ids and
      arrival time, `Queued`/`Duplicate`/`Full`/`Empty` push outcomes. Verify:
      `the_queue_is_bounded_and_drops_duplicates`.
- [x] `TurnDriver::interject` stamps the active turn and emits
      `InterjectionQueued`; the queue is drained at the top of the model-call
      loop, after any tool round and before the final call is built.
      `InterjectionDelivered` carries the turn and id, and the message enters
      the request as `[user interjection] …`. Verify:
      `an_interjection_waits_for_the_next_safe_point`.
- [x] An item that misses the last safe point is drained at turn end and
      reported through `InterjectionDropped` and
      `TurnOutcome::late_interjections`. Verify:
      `a_late_interjection_is_reported_instead_of_dropped_silently`.
- [x] Event-log redaction covers all three interjection texts; the TUI shows
      full/duplicate notices and late items after the turn; the line printer
      reports late items.
- [x] TUI: `Enter` while running returns `Command::Interject`; slash commands
      still run (`/cancel`). Verify: `enter_while_running_queues_an_interjection`,
      `a_slash_command_still_works_while_a_turn_runs`.
- [ ] `interrupt` (cancel and restart) and `next_turn` remain host policies;
      only `queue` is wired in the reference host.

## Boundaries

- Delivery happens at the top of the model loop only; a stream and a tool
  call are never split or interrupted.
- The queue is in memory per driver turn; it is not persisted across a
  process restart.
- The TUI cannot queue while a permission modal is open; the prompt must be
  answered first.
- The line `--session` mode has no mid-turn reader; its messages remain a
  between-turn queue.
