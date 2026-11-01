# M5 persistence

Implements the storage layout in ADR 0008 without moving it into the kernel.
The kernel exposes a snapshot; the host decides where it lives.

## Steps

- [x] Add `SessionSnapshot` and `SegmentSnapshot`, with `Session::snapshot`
      and `Session::restore`. Verify: snapshot round-trip test in
      `keel-core`; entries, records, memo and facts survive.
- [x] Expose `TurnDriver::session_snapshot` and `with_session`.
      Verify: used by the CLI; no kernel knowledge of the backend.
- [x] Add the SQLite host store (`hosts/keel-cli/src/store.rs`): WAL, foreign
      keys, `schema_version`, `sessions`, `turns`. One transaction per turn
      writes the snapshot, the task list and the transcript row.
      Verify: store unit tests for round-trip, missing session and turn rows.
- [x] Add CLI `--store <file>` and `--session-id <id>`; load at startup, save
      after every turn. Stored tasks take precedence over `--todo-file`.
      Verify: two processes on one store: the second saw `history: detail=1`,
      `tasks: done=1 current=#2`, and answered a chapter-1 probe without
      reading files.
- [x] Project the snapshot into queryable tables: `segments`,
      `segment_entries`, `records`, `session_summaries`, `summary_covers`,
      `facts`, `todos`, and the `session_current` view. Verify: a live turn
      projected 1 segment, 8 entries, 2 tasks and a correct view row; a second
      save replaces rows without duplication.
- [x] `fact_history` and `todo_history`: facts are versioned with reason
      `added`/`superseded`/`forgotten`; each task revision that changes the
      list stores the full snapshot. Migrated databases backfill version 1 on
      the next save. Verify: unit tests for fact supersede/forget and for a
      task status change; the live memory DB backfilled its facts.
- [x] Context snapshots: opt-in capture of the turn-start message list,
      redacted, one row per turn. Verify: driver test and store test; a live
      turn wrote the row.
- [ ] Fault injection: a crash mid-turn leaves the previous turn intact; a
      corrupt snapshot fails with a typed error, not a panic.
- [ ] Multi-session memory evaluation on top of the store (LongMemEval-style
      extraction, update, temporal, abstention).

## State ownership

- Kernel: `sessions.snapshot_json`, used only to resume.
- Frontend: the normalized tables and `session_current`, never the snapshot.
- Both are written from one snapshot in one transaction. Derived rows are
  replaced per save; summary revisions are append-only.
- No stored session status. Latest turn, current segment, current summary,
  active facts and task progress are queries, so they cannot contradict each
  other or a stale column.

## Notes

- Fact candidates are transient extraction state and are not snapshot.
- The memo is stored with its revision and covers, so a resume continues the
  same numbering.
- The store is a host adapter; ADR 0012 keeps vector and graph stores out of
  the kernel.
