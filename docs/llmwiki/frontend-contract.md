# Frontend read contract

The CLI host writes one SQLite file per deployment (`--store`). A frontend
opens it **read-only** and reads the normalized tables and `session_current`.
It never reads `sessions.snapshot_json`; that column is the kernel's resume
format and may change shape.

## Guarantees

- One writer: the CLI. Readers see the state after the last committed turn.
- A turn is one transaction: snapshot, transcript row and projection commit
  together, so a reader never sees a half-written turn.
- There is **no session status column**. Every current value is a query, so a
  stale field cannot contradict the rows.
- Derived rows (segments, entries, records, facts, tasks) are replaced on every
  save. They never accumulate stale copies.
- Summary revisions are append-only. The current memo is the highest revision.

## Canonical queries

Sessions:

```sql
SELECT session_id, updated_at, last_turn_seq, last_turn_state, turns,
       summary_revision, current_segment_id, active_facts,
       tasks_completed, tasks_total
FROM session_current
ORDER BY updated_at DESC;
```

Transcript for one session:

```sql
SELECT seq, state, input, answer, usage_json, duration_ms, created_at
FROM turns
WHERE session_id = ?1
ORDER BY seq;
```

Segments and their messages:

```sql
SELECT id, seq, topic, state, turns, verified
FROM segments
WHERE session_id = ?1
ORDER BY seq;

SELECT segment_id, seq, role, content
FROM segment_entries
WHERE session_id = ?1
ORDER BY segment_id, seq;
```

Structured records for sealed segments:

```sql
SELECT segment_id, goal, result, changes_json, pending_json,
       verified, folded, absorbed_in
FROM records
WHERE session_id = ?1;
```

Current memo (highest revision) and what it covers:

```sql
SELECT revision, text, budget_tokens, chars, created_at
FROM session_summaries
WHERE session_id = ?1
ORDER BY revision DESC
LIMIT 1;

SELECT segment_id
FROM summary_covers
WHERE session_id = ?1 AND revision = ?2;
```

Facts and tasks:

```sql
SELECT scope, key, statement, confirmed, segment_id
FROM facts
WHERE session_id = ?1 AND active = 1
ORDER BY scope, key;

SELECT item_id, text, status, source, position
FROM todos
WHERE session_id = ?1
ORDER BY position;
```

## What not to do

- Do not parse `sessions.snapshot_json` or `sessions.todos_json`. They exist so
  the kernel can resume; the tables are the frontend path.
- Do not write to the database. Status changes go through the CLI and the
  kernel; a frontend that edits rows would break the snapshot and projection
  apart.
- Do not derive "current" from `updated_at`; use the queries above. Timestamps
  are for sorting, not for deciding which summary or turn is current.

## Not available yet

- `fact_history` and `todo_history`: superseded facts and past task revisions
  are not projected. The snapshot keeps current state only.
- `context_snapshots`: the exact message list the model saw on a turn. It is
  designed in ADR 0008 but not written.
- Events: the audit plane lands with the frontend milestone.

## Versioning

`schema_version` lists applied migrations. A frontend should refuse to open a
database whose highest version it does not know. Version 3 adds the projection
tables and `session_current`.
