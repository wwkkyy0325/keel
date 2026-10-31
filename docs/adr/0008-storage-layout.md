# ADR 0008 — Storage layout for transcript, context and memory

## Status

Proposed. The schema is fixed here; implementation lands with M5 persistence.
The in-memory types already match this layout.

## Context

Compression must never lose history. The model should see a small, derived
projection, while the user must be able to inspect everything: the full
transcript, every segment and record, folded records, summaries and facts.
The same data feeds a future frontend, debugging and audit.

The kernel already separates planes implicitly:

- the event log is append-only;
- the session keeps raw entries even after a record replaces them in context;
- facts have scope and provenance;
- the context is assembled per turn.

One rolling session memo now absorbs finished segments at every seal, and
fact-covered records can fold out of the context before that. The storage
design must make both reversible and visible, including every memo revision.

## Decision

### Four planes

1. **Audit** — append-only events.
2. **Transcript** — sessions, turns and raw messages. Never deleted or edited.
3. **Derived context** — segments, records, summaries, fold flags and context
   snapshots. Regenerable from the transcript, so edits are allowed.
4. **Memory** — scoped facts with provenance. Superseded, not deleted.

The model reads only the assembled projection. The frontend and audit read the
raw planes and the derived rows.

### Tables (SQLite, WAL, foreign keys on)

```sql
schema_version(version integer primary key, applied_at text)

sessions(id text primary key, started_at text, metadata_json text)
turns(id text primary key, session_id text, seq integer, input text,
      state text, answer text, usage_json text, duration_ms integer,
      created_at text)
messages(id text primary key, turn_id text, seq integer, role text,
         content text, tool_call_id text, created_at text)

segments(id text primary key, session_id text, seq integer, topic text,
         state text, turns integer, verified integer,
         opened_at text, sealed_at text)
segment_messages(segment_id text, message_id text, seq integer,
                 primary key(segment_id, message_id))

records(segment_id text primary key, goal text, result text,
        changes_json text, pending_json text, verified integer,
        folded integer not null default 0,
        absorbed_in integer,        -- summary revision that includes it
        created_at text, updated_at text)

session_summaries(session_id text, revision integer, text text,
                  budget_tokens integer, chars integer, model text,
                  usage_json text, created_at text,
                  primary key(session_id, revision))
summary_covers(session_id text, revision integer, segment_id text,
               primary key(session_id, revision, segment_id))

todos(session_id text, item_id integer, text text, status text,
      source text, position integer, revision integer,
      updated_at text, primary key(session_id, item_id))
todo_history(session_id text, revision integer, snapshot_json text,
             created_at text, primary key(session_id, revision))

facts(id text primary key, key text, statement text, scope text,
      confirmed integer, segment_id text, active integer not null default 1,
      superseded_by text, created_at text, updated_at text)
fact_history(fact_id text, version integer, statement text, scope text,
             changed_at text, reason text)

events(id integer primary key autoincrement, turn_id text, seq integer,
       kind text, payload_json text, created_at text)

context_snapshots(id integer primary key autoincrement, turn_id text,
                  estimate integer, budget integer, detail_turns integer,
                  digest_turns integer, records integer, folded integer,
                  messages_json text, created_at text)
```

Lists such as `changes`, `pending`, `usage` and the message projection are JSON
in one column; they are read as whole values and never queried inside.

### Invariants

- Raw planes are append-only. No API hard-deletes a message, turn or event.
- Compression only changes derived rows: a record gets `folded = 1`, a new
  summary revision is inserted, `summary_covers` lists the segments it
  absorbed, and each absorbed record gets `absorbed_in = revision`.
- A folded or absorbed record keeps its `result` and `changes`; only the
  context query filters it out (`folded = 0 and absorbed_in is null`).
- Summary revisions are append-only. The current memo is the highest revision;
  an earlier revision can be restored by writing a new revision, never by
  editing history.
- The task list is kernel-owned. Every status or membership change is a
  `todo_history` snapshot; host items are never removed by the model.
- Facts are superseded (`active = 0`, `superseded_by`), never removed; history
  keeps the previous statement.
- Every derived row references its source segment or turn; provenance is a
  foreign key, not free text.
- A turn writes its rows in one transaction. A crash leaves the previous turn
  intact; there is no half-visible turn.
- Sequence numbers are monotonic per session, assigned inside the transaction.

### L4 and catch-up summaries

- When every change in a record is covered by an `artifact:` fact and `pending`
  is empty, the record is folded: `folded = 1`. It leaves the context, not the
  database.
- At every seal the driver rewrites the session memo. Its input is the previous
  revision, the new record and transcript, and every unabsorbed record that
  fits the input budget, each marked `[segment <id>]`.
- On success a new `session_summaries` row is inserted, `summary_covers` lists
  every absorbed segment, and each record gets `absorbed_in = revision`.
- On failure the previous revision stays current, records keep
  `absorbed_in = null`, and the next update includes them again.
- A frontend can therefore show: the raw transcript, every memo revision, which
  segments each revision absorbed, which records were folded, and the exact
  source text the memo saw.

### Why SQLite

Embedded, transactional, single file, no server, and easy to inspect with any
SQLite client. WAL keeps reads non-blocking while a turn writes. If a host needs
another backend, the repository interface stays the same.

## Consequences

- The frontend can render a timeline and a memory view without parsing prompts.
- Context snapshots let a developer see exactly what the model saw on any turn,
  which is the debugging path for compression problems.
- Storage grows with the raw transcript; retention and vacuum are host policy,
  not kernel behavior.
- Implementation is deferred to M5; the in-memory `Session`, `SegmentRecord`
  and `Fact` types already carry every field this schema needs.

## Alternatives rejected

- Deleting folded records: loses history and breaks the later catch-up summary.
- Storing only summaries: the frontend cannot show what happened, and a bad
  summary cannot be audited.
- Hard-deleting superseded facts: loses why a memory changed.
- Event-sourcing only, with no derived tables: every frontend query would have
  to replay the log.
