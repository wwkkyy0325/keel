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
- [ ] Queryable segment, record, summary and fact tables for a frontend.
      Today they are inside the snapshot JSON.
- [ ] Fault injection: a crash mid-turn leaves the previous turn intact; a
      corrupt snapshot fails with a typed error, not a panic.
- [ ] Multi-session memory evaluation on top of the store (LongMemEval-style
      extraction, update, temporal, abstention).

## Notes

- Fact candidates are transient extraction state and are not snapshot.
- The memo is stored with its revision and covers, so a resume continues the
  same numbering.
- The store is a host adapter; ADR 0012 keeps vector and graph stores out of
  the kernel.
