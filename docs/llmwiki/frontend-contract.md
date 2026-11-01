# Frontend read contract

The CLI host writes one SQLite file per deployment (`--store`). A frontend
opens it **read-only** and reads the normalized tables and `session_current`.
It never reads `sessions.snapshot_json`; that column is the kernel's resume
format and may change shape.

## Composed views

The host assembles the screens. A frontend renders values; it does not join
tables, decode JSON columns or sort rows.

```bash
keel-cli --store keel.db --view sessions
keel-cli --store keel.db --view session --session-id mem
keel-cli --store keel.db --view turn --session-id mem --turn 3
keel-cli --store keel.db --view memory --session-id mem
```

- `sessions` is an array ordered by `updated_at` descending, then `id`.
- `session` is one object with `session`, `memo`, `facts`, `tasks`, `branches`,
  `records` and `turns`. Ordering is fixed: memo covers by segment id, facts by
  `scope,key`, tasks by `position`, branches by `updated_at,id`, records by
  segment `seq`, turns by `seq`.
- `turns[].answer_model` is the decoded structured answer and
  `turns[].usage` is the decoded token row. The UI never parses `answer_json`
  or `usage_json`.
- `turn` is one turn plus `context`: the captured turn-start message list with
  decoded `stages` and `messages`.
- `memory` is the typed timeline: every `fact_history` row with
  `added`/`superseded`/`forgotten`, and every `todo_history` revision with
  decoded items.
- A missing session or turn is an error with a non-zero exit, not an empty
  screen.
- Long sessions page turns backward: `--limit N` returns the newest N turns
  (`0` is all), and `--before <seq>` is an exclusive cursor. The next page
  passes the smallest `seq` it received. The window is always ascending;
  `session.turns` still reports the total, and `turns[0].seq > 1` means an
  older page exists.

Invariants: the same store produces the same view (no clock, randomness or
map iteration in the assembly); the view is read-only. Tests in
`hosts/keel-cli/src/view.rs` assert every panel and that two calls serialize
identically, so the frontend contract is verified without starting a UI.

## Three layers of "the final answer"

The session does not overwrite one answer document. It keeps three layers, each
at a different scope:

| Layer | Where | Written | Meaning |
|---|---|---|---|
| Turn answer | `turns.answer` (body) and `turns.answer_json` (structured) | every turn | The `Answer` artifact: the latest round with no tool call, the observed changes, pending items, verification and `missing`. |
| Segment record | `records` | at a segment seal | One cumulative line for the segment: `goal`, the last assistant `result`, accumulated `changes` and `pending`, `turns`, `verified`. |
| Session memo | `session_summaries` | at a segment seal | One rolling summary revision covering every sealed segment so far. |

`turns.answer` is append-only per turn; a retry of the same turn replaces the
same row. `records` are derived from the segment at seal, and
`session_summaries` revisions are appended, never edited. So the per-turn
answers and the memo history are not overwritten.

A store can legitimately have `turns` rows but no `records` or summaries: a
segment only seals when the previous turn passed an acceptance plan, hit the
turn cap, or the topic changed. A run without acceptance can stay in one open
segment forever. That is why a fresh store shows answers but no cumulative
record.

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
SELECT seq, state, input, answer, answer_json,
       usage_json, duration_ms, created_at
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

Structured records for sealed segments, in story order. This is the cumulative
"one result per segment" timeline:

```sql
SELECT g.seq, g.topic, r.goal, r.result, r.changes_json, r.pending_json,
       r.turns, r.verified, r.folded, r.absorbed_in
FROM records r
JOIN segments g ON g.session_id = r.session_id AND g.id = r.segment_id
WHERE r.session_id = ?1
ORDER BY g.seq;
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

Transitions:

```sql
SELECT scope, key, version, statement, active, reason, changed_at
FROM fact_history
WHERE session_id = ?1
ORDER BY scope, key, version;

SELECT revision, snapshot_json, created_at
FROM todo_history
WHERE session_id = ?1
ORDER BY revision;
```

`reason` is `added`, `superseded` or `forgotten`. `active = 0` marks the
forgotten version. `todo_history.snapshot_json` is the full `TodoList` at that
revision.

The exact prompt the model received, when the host ran with
`--context-snapshots`:

```sql
SELECT turn_id, estimate, budget, stages_json, messages_json, created_at
FROM context_snapshots
WHERE session_id = ?1
ORDER BY turn_id;
```

`messages_json` is the turn-start assembly after budget, memo, recall and
truncation, with secrets redacted. It is not the transcript; a later
reconstruction from segments can differ. Turns run without the flag have no
row.

## Branches

A branch is a normal session with a parent. There is no branch table:

```sql
SELECT id, updated_at, parent_session_id, fork_turn_seq
FROM sessions
WHERE parent_session_id = ?1
ORDER BY updated_at;
```

`fork_turn_seq` is the turn whose end state the branch starts from; the branch
copies turns `1..fork_turn_seq` and rebuilds every derived table. Its facts,
memo and tasks are those of the parent at that turn, not the parent's current
state. A branch is independent after creation: later parent turns do not
appear in it.

Deleting a session through the CLI removes its own rows; branches are separate
sessions and survive. The parent link is historical metadata.

Checkpoints are kept for every turn by default. With `--keep-checkpoints N` the
host keeps the newest N and every turn a branch was forked from; older turns
can no longer be fork points and a fork attempt reports a missing checkpoint.

## What not to do

- Do not parse `sessions.snapshot_json` or `sessions.todos_json`. They exist so
  the kernel can resume; the tables are the frontend path.
- Do not write to the database. Status changes go through the CLI and the
  kernel; a frontend that edits rows would break the snapshot and projection
  apart.
- Do not derive "current" from `updated_at`; use the queries above. Timestamps
  are for sorting, not for deciding which summary or turn is current.

## Overwritten history

`facts` and `todos` hold current state. Their transitions are kept in
`fact_history` and `todo_history`:

- A fact that appears is version 1 with reason `added`; a changed statement
  appends `superseded`; a fact that leaves the set appends `forgotten` with
  `active = 0`.
- Every task revision that changes the list appends the full list snapshot.

Turn answers, segment records and memo revisions are append-only at their own
level. The resume snapshot (`snapshot_json`) remains current state only.

## Not available yet

- Event rows as a queryable audit plane; the CLI keeps the event log in memory
  and writes turn rows.
- A checkpoints query: `session_checkpoints` is the kernel's fork source, not a
  frontend surface. Show branch lineage through `sessions` instead.

## Versioning

`schema_version` lists applied migrations. A frontend should refuse to open a
database whose highest version it does not know. Version 8 adds per-turn
checkpoints and the branch columns.
