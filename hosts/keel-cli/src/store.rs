//! SQLite session store for the CLI host.
//!
//! The kernel owns the session types; this adapter only moves a snapshot in and
//! out of a file. Per ADR 0008 there is one writer and two readers:
//!
//! - the kernel reads the opaque `sessions.snapshot_json` to resume;
//! - a frontend reads the normalized tables and the `session_current` view;
//! - both are written from the same snapshot in one transaction, so they cannot
//!   disagree.
//!
//! The normalized rows are a projection, not a second source of truth. There is
//! no stored `session.status`: status is derived from turns, tasks and the
//! latest summary revision, so a stale column cannot contradict them.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use keel_core::{KernelError, SessionSnapshot, TodoList};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

const SCHEMA_VERSION: i64 = 9;
/// Each checkpoint carries a full session snapshot, so an unbounded history
/// grows with the square of the session length. Twenty recent branch points
/// are enough to fork from recent work; `--keep-checkpoints 0` keeps all.
const DEFAULT_CHECKPOINT_LIMIT: u32 = 20;

/// One stored session.
pub struct StoredSession {
    pub snapshot: SessionSnapshot,
    pub todos: Option<TodoList>,
}

/// The transcript row for one finished turn.
pub struct StoredTurn<'a> {
    pub seq: u32,
    pub input: &'a str,
    pub state: &'a str,
    pub answer: &'a str,
    /// The structured `Answer`: revision, body, changes, pending, verification,
    /// notes and `missing`.
    pub answer_json: &'a str,
    pub usage_json: &'a str,
    pub duration_ms: u64,
    /// Kernel error text with its code, when the turn did not finish.
    pub error: Option<&'a str>,
    /// Turn-start message list, when the host enabled capture.
    pub context: Option<&'a keel_core::ContextSnapshot>,
    /// Serialized kernel events in emission order: `(name, payload_json)`.
    pub events: &'a [(String, String)],
}

/// A SQLite-backed host store.
pub struct Store {
    conn: Connection,
    /// Keep at most this many checkpoints per session; 0 keeps every turn.
    /// Fork points referenced by a branch are always kept.
    checkpoint_limit: u32,
}

impl Store {
    /// Opens or creates the database and applies the schema.
    pub fn open(path: &Path) -> Result<Self, KernelError> {
        let conn = Connection::open(path).map_err(|error| store_error("open", error))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|error| store_error("enable WAL", error))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|error| store_error("enable foreign keys", error))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (
                 version INTEGER PRIMARY KEY,
                 applied_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions (
                 id TEXT PRIMARY KEY,
                 started_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 snapshot_json TEXT NOT NULL,
                 todos_json TEXT,
                 parent_session_id TEXT,
                 fork_turn_seq INTEGER
             );
             CREATE TABLE IF NOT EXISTS turns (
                 id TEXT PRIMARY KEY,
                 session_id TEXT NOT NULL REFERENCES sessions(id),
                 seq INTEGER NOT NULL,
                 input TEXT NOT NULL,
                 state TEXT NOT NULL,
                 answer TEXT NOT NULL,
                 answer_json TEXT NOT NULL DEFAULT '{}',
                 error TEXT,
                 usage_json TEXT NOT NULL,
                 duration_ms INTEGER NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS segments (
                 session_id TEXT NOT NULL,
                 id INTEGER NOT NULL,
                 seq INTEGER NOT NULL,
                 topic TEXT NOT NULL,
                 state TEXT NOT NULL,
                 turns INTEGER NOT NULL,
                 verified INTEGER NOT NULL,
                 PRIMARY KEY (session_id, id)
             );
             CREATE TABLE IF NOT EXISTS segment_entries (
                 session_id TEXT NOT NULL,
                 segment_id INTEGER NOT NULL,
                 seq INTEGER NOT NULL,
                 role TEXT NOT NULL,
                 content TEXT NOT NULL,
                 PRIMARY KEY (session_id, segment_id, seq)
             );
             CREATE TABLE IF NOT EXISTS records (
                 session_id TEXT NOT NULL,
                 segment_id INTEGER NOT NULL,
                 goal TEXT NOT NULL,
                 result TEXT NOT NULL,
                 changes_json TEXT NOT NULL,
                 pending_json TEXT NOT NULL,
                 verified INTEGER NOT NULL,
                 turns INTEGER NOT NULL DEFAULT 0,
                 folded INTEGER NOT NULL,
                 absorbed_in INTEGER,
                 PRIMARY KEY (session_id, segment_id)
             );
             CREATE TABLE IF NOT EXISTS session_summaries (
                 session_id TEXT NOT NULL,
                 revision INTEGER NOT NULL,
                 text TEXT NOT NULL,
                 budget_tokens INTEGER NOT NULL,
                 chars INTEGER NOT NULL,
                 created_at INTEGER NOT NULL,
                 PRIMARY KEY (session_id, revision)
             );
             CREATE TABLE IF NOT EXISTS summary_covers (
                 session_id TEXT NOT NULL,
                 revision INTEGER NOT NULL,
                 segment_id INTEGER NOT NULL,
                 PRIMARY KEY (session_id, revision, segment_id)
             );
             CREATE TABLE IF NOT EXISTS facts (
                 session_id TEXT NOT NULL,
                 scope TEXT NOT NULL,
                 key TEXT NOT NULL,
                 statement TEXT NOT NULL,
                 confirmed INTEGER NOT NULL,
                 segment_id INTEGER NOT NULL,
                 active INTEGER NOT NULL,
                 PRIMARY KEY (session_id, scope, key)
             );
             CREATE TABLE IF NOT EXISTS todos (
                 session_id TEXT NOT NULL,
                 item_id INTEGER NOT NULL,
                 text TEXT NOT NULL,
                 status TEXT NOT NULL,
                 source TEXT NOT NULL,
                 position INTEGER NOT NULL,
                 PRIMARY KEY (session_id, item_id)
             );
             CREATE TABLE IF NOT EXISTS fact_history (
                 session_id TEXT NOT NULL,
                 scope TEXT NOT NULL,
                 key TEXT NOT NULL,
                 version INTEGER NOT NULL,
                 statement TEXT NOT NULL,
                 active INTEGER NOT NULL,
                 reason TEXT NOT NULL,
                 changed_at INTEGER NOT NULL,
                 PRIMARY KEY (session_id, scope, key, version)
             );
             CREATE TABLE IF NOT EXISTS todo_history (
                 session_id TEXT NOT NULL,
                 revision INTEGER NOT NULL,
                 snapshot_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 PRIMARY KEY (session_id, revision)
             );
             CREATE TABLE IF NOT EXISTS session_checkpoints (
                 session_id TEXT NOT NULL,
                 turn_seq INTEGER NOT NULL,
                 snapshot_json TEXT NOT NULL,
                 todos_json TEXT,
                 created_at INTEGER NOT NULL,
                 PRIMARY KEY (session_id, turn_seq)
             );
             CREATE TABLE IF NOT EXISTS events (
                 session_id TEXT NOT NULL,
                 turn_seq INTEGER NOT NULL,
                 seq INTEGER NOT NULL,
                 name TEXT NOT NULL,
                 payload_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 PRIMARY KEY (session_id, turn_seq, seq)
             );
             CREATE TABLE IF NOT EXISTS context_snapshots (
                 turn_id TEXT PRIMARY KEY,
                 session_id TEXT NOT NULL,
                 estimate INTEGER NOT NULL,
                 budget INTEGER NOT NULL,
                 stages_json TEXT NOT NULL,
                 messages_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             DROP VIEW IF EXISTS session_current;
             CREATE VIEW session_current AS
             SELECT s.id AS session_id,
                    s.updated_at AS updated_at,
                    (SELECT MAX(seq) FROM turns t WHERE t.session_id = s.id)
                        AS last_turn_seq,
                    (SELECT state FROM turns t WHERE t.session_id = s.id
                      ORDER BY seq DESC LIMIT 1)
                        AS last_turn_state,
                    (SELECT COUNT(*) FROM turns t WHERE t.session_id = s.id)
                        AS turns,
                    (SELECT revision FROM session_summaries m
                      WHERE m.session_id = s.id ORDER BY revision DESC LIMIT 1)
                        AS summary_revision,
                    (SELECT id FROM segments g
                      WHERE g.session_id = s.id ORDER BY seq DESC LIMIT 1)
                        AS current_segment_id,
                    (SELECT COUNT(*) FROM facts f
                      WHERE f.session_id = s.id AND f.active = 1)
                        AS active_facts,
                    (SELECT COUNT(*) FROM todos d
                      WHERE d.session_id = s.id AND d.status = 'completed')
                        AS tasks_completed,
                    (SELECT COUNT(*) FROM todos d WHERE d.session_id = s.id)
                        AS tasks_total
             FROM sessions s;",
        )
        .map_err(|error| store_error("create schema", error))?;
        // Additive migrations for databases written by earlier slices.
        ensure_column(
            &conn,
            "turns",
            "answer_json",
            "ALTER TABLE turns ADD COLUMN answer_json TEXT NOT NULL DEFAULT '{}'",
        )?;
        ensure_column(
            &conn,
            "records",
            "turns",
            "ALTER TABLE records ADD COLUMN turns INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            &conn,
            "turns",
            "error",
            "ALTER TABLE turns ADD COLUMN error TEXT",
        )?;
        ensure_column(
            &conn,
            "sessions",
            "parent_session_id",
            "ALTER TABLE sessions ADD COLUMN parent_session_id TEXT",
        )?;
        ensure_column(
            &conn,
            "sessions",
            "fork_turn_seq",
            "ALTER TABLE sessions ADD COLUMN fork_turn_seq INTEGER",
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (?1, ?2)",
            params![SCHEMA_VERSION, now()],
        )
        .map_err(|error| store_error("record schema version", error))?;
        Ok(Self {
            conn,
            checkpoint_limit: DEFAULT_CHECKPOINT_LIMIT,
        })
    }

    /// Sets the per-session checkpoint cap. Zero keeps every checkpoint.
    pub fn set_checkpoint_limit(&mut self, keep: u32) {
        self.checkpoint_limit = keep;
    }

    /// Read-only access for the composed frontend views.
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Loads a session snapshot and its task list, if they exist.
    pub fn load(&self, session_id: &str) -> Result<Option<StoredSession>, KernelError> {
        let row: Option<(String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT snapshot_json, todos_json FROM sessions WHERE id = ?1",
                params![session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| store_error("load session", error))?;
        let Some((snapshot_json, todos_json)) = row else {
            return Ok(None);
        };
        let snapshot: SessionSnapshot = serde_json::from_str(&snapshot_json)
            .map_err(|error| invalid_store(format!("invalid session snapshot: {error}")))?;
        let todos = match todos_json {
            Some(raw) => Some(
                serde_json::from_str(&raw)
                    .map_err(|error| invalid_store(format!("invalid task list: {error}")))?,
            ),
            None => None,
        };
        Ok(Some(StoredSession { snapshot, todos }))
    }

    /// Next transcript sequence number for a session.
    pub fn next_turn_seq(&self, session_id: &str) -> Result<u32, KernelError> {
        let seq: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM turns WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(|error| store_error("read turn sequence", error))?;
        Ok(u32::try_from(seq).unwrap_or(u32::MAX))
    }

    /// Writes the snapshot and optionally appends one turn, in one transaction.
    pub fn save(
        &mut self,
        session_id: &str,
        snapshot: &SessionSnapshot,
        todos: Option<&TodoList>,
        turn: Option<StoredTurn<'_>>,
    ) -> Result<(), KernelError> {
        let snapshot_json = serde_json::to_string(snapshot)
            .map_err(|error| invalid_store(format!("cannot encode session: {error}")))?;
        let todos_json = match todos {
            Some(todos) => Some(
                serde_json::to_string(todos)
                    .map_err(|error| invalid_store(format!("cannot encode tasks: {error}")))?,
            ),
            None => None,
        };
        let now = now();
        let keep = self.checkpoint_limit;
        let tx = self
            .conn
            .transaction()
            .map_err(|error| store_error("begin transaction", error))?;
        tx.execute(
            "INSERT INTO sessions (id, started_at, updated_at, snapshot_json, todos_json)
             VALUES (?1, ?2, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                 updated_at = excluded.updated_at,
                 snapshot_json = excluded.snapshot_json,
                 todos_json = excluded.todos_json",
            params![session_id, now, snapshot_json, todos_json],
        )
        .map_err(|error| store_error("write session", error))?;
        if let Some(turn) = turn {
            tx.execute(
                "INSERT OR REPLACE INTO turns
                 (id, session_id, seq, input, state, answer, answer_json,
                  error, usage_json, duration_ms, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    format!("{session_id}:{}", turn.seq),
                    session_id,
                    turn.seq,
                    turn.input,
                    turn.state,
                    turn.answer,
                    turn.answer_json,
                    turn.error,
                    turn.usage_json,
                    turn.duration_ms,
                    now,
                ],
            )
            .map_err(|error| store_error("write turn", error))?;
            if let Some(context) = turn.context {
                let stages =
                    serde_json::to_string(&context.stages).unwrap_or_else(|_| "[]".to_owned());
                let messages =
                    serde_json::to_string(&context.messages).unwrap_or_else(|_| "[]".to_owned());
                tx.execute(
                    "INSERT OR REPLACE INTO context_snapshots
                     (turn_id, session_id, estimate, budget, stages_json, messages_json,
                      created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        format!("{session_id}:{}", turn.seq),
                        session_id,
                        context.estimate,
                        context.budget,
                        stages,
                        messages,
                        now,
                    ],
                )
                .map_err(|error| store_error("write context snapshot", error))?;
            }
            // Kernel events in emission order, for a UI or post-mortem. A
            // re-save of the same turn replaces the previous stream.
            tx.execute(
                "DELETE FROM events WHERE session_id = ?1 AND turn_seq = ?2",
                params![session_id, turn.seq],
            )
            .map_err(|error| store_error("replace events", error))?;
            for (index, (name, payload)) in turn.events.iter().enumerate() {
                tx.execute(
                    "INSERT OR REPLACE INTO events
                     (session_id, turn_seq, seq, name, payload_json, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![session_id, turn.seq, (index + 1) as i64, name, payload, now],
                )
                .map_err(|error| store_error("write event", error))?;
            }
            // The state after this turn, so a later branch can resume here
            // without replaying a memo the model wrote.
            tx.execute(
                "INSERT OR REPLACE INTO session_checkpoints
                 (session_id, turn_seq, snapshot_json, todos_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![session_id, turn.seq, snapshot_json, todos_json, now],
            )
            .map_err(|error| store_error("write checkpoint", error))?;
            if keep != 0 {
                prune_checkpoints(&tx, session_id, keep)
                    .map_err(|error| store_error("prune checkpoints", error))?;
            }
        }
        project(&tx, session_id, snapshot, todos, now)
            .map_err(|error| store_error("project session", error))?;
        tx.commit().map_err(|error| store_error("commit", error))?;
        Ok(())
    }

    /// Creates a new session from the checkpoint at `turn` of `source`.
    ///
    /// The checkpoint is the state after that turn: segments, records, memo,
    /// facts and the task list. The transcript rows up to `turn` are copied so
    /// the branch reads as a conversation, and the projection is rebuilt from
    /// the snapshot. The parent is untouched.
    pub fn fork(&mut self, source: &str, turn: u32, target: &str) -> Result<(), KernelError> {
        if source == target {
            return Err(invalid_store(
                "the branch must use a different session id".to_owned(),
            ));
        }
        let checkpoint: Option<(String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT snapshot_json, todos_json FROM session_checkpoints
                 WHERE session_id = ?1 AND turn_seq = ?2",
                params![source, turn],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| store_error("read checkpoint", error))?;
        let Some((snapshot_json, todos_json)) = checkpoint else {
            return Err(invalid_store(format!(
                "session `{source}` has no checkpoint at turn {turn}"
            )));
        };
        let snapshot: SessionSnapshot = serde_json::from_str(&snapshot_json)
            .map_err(|error| invalid_store(format!("invalid session snapshot: {error}")))?;
        let todos: Option<TodoList> = match todos_json.as_deref() {
            Some(raw) => Some(
                serde_json::from_str(raw)
                    .map_err(|error| invalid_store(format!("invalid task list: {error}")))?,
            ),
            None => None,
        };
        let exists: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE id = ?1",
                params![target],
                |row| row.get(0),
            )
            .map_err(|error| store_error("check session", error))?;
        if exists != 0 {
            return Err(invalid_store(format!(
                "session `{target}` already exists; choose another id"
            )));
        }

        let now = now();
        let tx = self
            .conn
            .transaction()
            .map_err(|error| store_error("begin transaction", error))?;
        tx.execute(
            "INSERT INTO sessions
             (id, started_at, updated_at, snapshot_json, todos_json,
              parent_session_id, fork_turn_seq)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6)",
            params![target, now, snapshot_json, todos_json, source, turn],
        )
        .map_err(|error| store_error("write branch session", error))?;
        tx.execute(
            "INSERT OR REPLACE INTO session_checkpoints
             (session_id, turn_seq, snapshot_json, todos_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![target, turn, snapshot_json, todos_json, now],
        )
        .map_err(|error| store_error("copy checkpoint", error))?;
        tx.execute(
            "INSERT INTO turns
             (id, session_id, seq, input, state, answer, answer_json,
              usage_json, duration_ms, created_at)
             SELECT ?1 || ':' || seq, ?1, seq, input, state, answer, answer_json,
                    usage_json, duration_ms, created_at
             FROM turns WHERE session_id = ?2 AND seq <= ?3",
            params![target, source, turn],
        )
        .map_err(|error| store_error("copy turns", error))?;
        tx.execute(
            "INSERT INTO events
             (session_id, turn_seq, seq, name, payload_json, created_at)
             SELECT ?1, turn_seq, seq, name, payload_json, created_at
             FROM events WHERE session_id = ?2 AND turn_seq <= ?3",
            params![target, source, turn],
        )
        .map_err(|error| store_error("copy events", error))?;
        project(&tx, target, &snapshot, todos.as_ref(), now)
            .map_err(|error| store_error("project branch", error))?;
        tx.commit()
            .map_err(|error| store_error("commit branch", error))?;
        Ok(())
    }

    /// Deletes a session and every row derived from it. Branches are separate
    /// sessions and are not deleted with the parent.
    pub fn delete_session(&mut self, session_id: &str) -> Result<bool, KernelError> {
        let tx = self
            .conn
            .transaction()
            .map_err(|error| store_error("begin transaction", error))?;
        let exists: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(|error| store_error("check session", error))?;
        if exists == 0 {
            return Ok(false);
        }
        for table in [
            "events",
            "context_snapshots",
            "session_checkpoints",
            "fact_history",
            "facts",
            "todo_history",
            "todos",
            "summary_covers",
            "session_summaries",
            "records",
            "segment_entries",
            "segments",
            "turns",
        ] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE session_id = ?1"),
                params![session_id],
            )
            .map_err(|error| store_error("delete session rows", error))?;
        }
        tx.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])
            .map_err(|error| store_error("delete session", error))?;
        tx.commit()
            .map_err(|error| store_error("commit delete", error))?;
        Ok(true)
    }
}

/// Rewrites the normalized read model from the snapshot. The rows are a
/// projection, so a full replace cannot leave a stale segment or task behind.
/// The rows that the next projection is about to replace.
struct PreviousState {
    facts: BTreeMap<(String, String), String>,
    fact_versions: BTreeMap<(String, String), i64>,
    todos: Vec<(i64, String, String, String, i64)>,
}

/// Drops old checkpoints for one session, keeping the newest `keep` and every
/// turn that a branch was forked from.
fn prune_checkpoints(tx: &Transaction<'_>, session_id: &str, keep: u32) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM session_checkpoints
         WHERE session_id = ?1
           AND turn_seq NOT IN (
               SELECT turn_seq FROM session_checkpoints
               WHERE session_id = ?1
               ORDER BY turn_seq DESC
               LIMIT ?2
           )
           AND turn_seq NOT IN (
               SELECT fork_turn_seq FROM sessions
               WHERE parent_session_id = ?1 AND fork_turn_seq IS NOT NULL
           )",
        params![session_id, i64::from(keep)],
    )?;
    Ok(())
}

fn project(
    tx: &Transaction<'_>,
    session_id: &str,
    snapshot: &SessionSnapshot,
    todos: Option<&TodoList>,
    now: i64,
) -> rusqlite::Result<()> {
    // Read the previous projection before replacing it: the history tables
    // need the values that are about to be overwritten.
    let mut old_facts: BTreeMap<(String, String), String> = BTreeMap::new();
    {
        let mut stmt =
            tx.prepare("SELECT scope, key, statement FROM facts WHERE session_id = ?1")?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (key, statement) = row?;
            old_facts.insert(key, statement);
        }
    }
    let mut versions: BTreeMap<(String, String), i64> = BTreeMap::new();
    {
        let mut stmt = tx.prepare(
            "SELECT scope, key, MAX(version) FROM fact_history
             WHERE session_id = ?1 GROUP BY scope, key",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
                row.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (key, version) = row?;
            versions.insert(key, version);
        }
    }
    let mut old_todos: Vec<(i64, String, String, String, i64)> = Vec::new();
    {
        let mut stmt = tx.prepare(
            "SELECT item_id, text, status, source, position FROM todos
             WHERE session_id = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        for row in rows {
            old_todos.push(row?);
        }
    }
    let previous = PreviousState {
        facts: old_facts,
        fact_versions: versions,
        todos: old_todos,
    };

    for table in ["segment_entries", "segments", "records", "facts", "todos"] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE session_id = ?1"),
            params![session_id],
        )?;
    }

    for (index, segment) in snapshot.segments.iter().enumerate() {
        tx.execute(
            "INSERT INTO segments (session_id, id, seq, topic, state, turns, verified)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session_id,
                segment.id,
                (index + 1) as i64,
                segment.topic,
                match segment.state {
                    keel_core::SegmentState::Open => "open",
                    keel_core::SegmentState::Sealed => "sealed",
                },
                segment.turns,
                i64::from(segment.verified),
            ],
        )?;
        for (entry_seq, entry) in segment.entries.iter().enumerate() {
            tx.execute(
                "INSERT INTO segment_entries (session_id, segment_id, seq, role, content)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    session_id,
                    segment.id,
                    (entry_seq + 1) as i64,
                    entry.role,
                    entry.content
                ],
            )?;
        }
        if let Some(record) = &segment.record {
            let changes =
                serde_json::to_string(&record.changes).unwrap_or_else(|_| "[]".to_owned());
            let pending =
                serde_json::to_string(&record.pending).unwrap_or_else(|_| "[]".to_owned());
            tx.execute(
                "INSERT INTO records
                 (session_id, segment_id, goal, result, changes_json, pending_json,
                  verified, turns, folded, absorbed_in)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    session_id,
                    segment.id,
                    record.goal,
                    record.result,
                    changes,
                    pending,
                    i64::from(record.verified),
                    record.turns,
                    i64::from(record.folded),
                    record.absorbed_in,
                ],
            )?;
        }
    }

    if let Some(summary) = &snapshot.summary {
        tx.execute(
            "INSERT OR IGNORE INTO session_summaries
             (session_id, revision, text, budget_tokens, chars, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session_id,
                summary.revision,
                summary.text,
                summary.budget_tokens,
                summary.text.chars().count() as i64,
                now,
            ],
        )?;
        for segment_id in &summary.covers {
            tx.execute(
                "INSERT OR IGNORE INTO summary_covers (session_id, revision, segment_id)
                 VALUES (?1, ?2, ?3)",
                params![session_id, summary.revision, segment_id],
            )?;
        }
    }

    for (scope, facts) in [
        ("session", &snapshot.facts),
        ("workspace", &snapshot.workspace_facts),
        ("global", &snapshot.global_facts),
    ] {
        for fact in facts {
            tx.execute(
                "INSERT OR REPLACE INTO facts
                 (session_id, scope, key, statement, confirmed, segment_id, active)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
                params![
                    session_id,
                    scope,
                    fact.key,
                    fact.statement,
                    i64::from(fact.confirmed),
                    fact.segment_id,
                ],
            )?;
        }
    }

    if let Some(todos) = todos {
        for (position, item) in todos.items.iter().enumerate() {
            tx.execute(
                "INSERT INTO todos (session_id, item_id, text, status, source, position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    session_id,
                    item.id,
                    item.text,
                    item.status.as_str(),
                    item.source.as_str(),
                    (position + 1) as i64,
                ],
            )?;
        }
    }

    record_history(tx, session_id, &previous, snapshot, todos, now)
}

/// Appends the fact and task history rows. The projection keeps current state;
/// these tables keep the transitions that would otherwise be overwritten.
fn record_history(
    tx: &Transaction<'_>,
    session_id: &str,
    previous: &PreviousState,
    snapshot: &SessionSnapshot,
    todos: Option<&TodoList>,
    now: i64,
) -> rusqlite::Result<()> {
    let mut new_facts: BTreeMap<(String, String), String> = BTreeMap::new();
    for (scope, facts) in [
        ("session", &snapshot.facts),
        ("workspace", &snapshot.workspace_facts),
        ("global", &snapshot.global_facts),
    ] {
        for fact in facts {
            new_facts.insert((scope.to_owned(), fact.key.clone()), fact.statement.clone());
        }
    }

    for (key, statement) in &new_facts {
        // A fact with no history row is seeded as its first version, even if
        // it was already in the current table (a migrated database).
        let changed = if !previous.fact_versions.contains_key(key) {
            Some("added")
        } else {
            match previous.facts.get(key) {
                None => Some("added"),
                Some(old) if old != statement => Some("superseded"),
                Some(_) => None,
            }
        };
        if let Some(reason) = changed {
            let version = previous.fact_versions.get(key).copied().unwrap_or(0) + 1;
            tx.execute(
                "INSERT INTO fact_history
                 (session_id, scope, key, version, statement, active, reason, changed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7)",
                params![session_id, key.0, key.1, version, statement, reason, now],
            )?;
        }
    }
    for (key, statement) in &previous.facts {
        if new_facts.contains_key(key) {
            continue;
        }
        let version = previous.fact_versions.get(key).copied().unwrap_or(0) + 1;
        tx.execute(
            "INSERT INTO fact_history
             (session_id, scope, key, version, statement, active, reason, changed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, 'forgotten', ?6)",
            params![session_id, key.0, key.1, version, statement, now],
        )?;
    }

    // The task list keeps one snapshot per revision that changed it.
    if let Some(todos) = todos {
        let new_todos: Vec<(i64, String, String, String, i64)> = todos
            .items
            .iter()
            .enumerate()
            .map(|(position, item)| {
                (
                    i64::from(item.id),
                    item.text.clone(),
                    item.status.as_str().to_owned(),
                    item.source.as_str().to_owned(),
                    (position + 1) as i64,
                )
            })
            .collect();
        if new_todos.as_slice() != previous.todos.as_slice() {
            let snapshot_json = serde_json::to_string(todos).unwrap_or_else(|_| "{}".to_owned());
            tx.execute(
                "INSERT OR REPLACE INTO todo_history
                 (session_id, revision, snapshot_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![session_id, todos.revision, snapshot_json, now],
            )?;
        }
    }
    Ok(())
}

/// Adds a column to an existing table. `table`, `column` and `ddl` are
/// compile-time constants, never user input.
fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    ddl: &str,
) -> Result<(), KernelError> {
    let present: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"),
            [],
            |row| row.get(0),
        )
        .map_err(|error| store_error("inspect table", error))?;
    if present == 0 {
        conn.execute(ddl, [])
            .map_err(|error| store_error("add column", error))?;
    }
    Ok(())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn store_error(action: &str, error: rusqlite::Error) -> KernelError {
    KernelError::Io {
        message: format!("store {action}: {error}"),
    }
}

fn invalid_store(message: String) -> KernelError {
    KernelError::InvalidConfig { message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_round_trips_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keel.db");
        let mut store = Store::open(&path).unwrap();
        let mut session = keel_core::Session::new();
        session.record_turn(keel_core::TurnRecord {
            input: "write chapter one",
            text: "chapter one is done",
            tools: &[],
            artifacts: &[],
            pending: &[],
            verified: true,
        });
        let mut todos = TodoList::default();
        todos.add_host("write chapter one").unwrap();

        store
            .save("session-1", &session.snapshot(), Some(&todos), None)
            .unwrap();
        let loaded = store.load("session-1").unwrap().unwrap();

        assert_eq!(loaded.snapshot.segments.len(), 1);
        assert_eq!(loaded.snapshot.segments[0].entries.len(), 2);
        assert_eq!(loaded.todos.unwrap().items.len(), 1);

        let segments: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM segments", [], |row| row.get(0))
            .unwrap();
        let entries: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM segment_entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!((segments, entries), (1, 2));
    }

    #[test]
    fn a_second_save_replaces_the_projection() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let mut session = keel_core::Session::new();
        session.record_turn(keel_core::TurnRecord {
            input: "write chapter one",
            text: "chapter one is done",
            tools: &[],
            artifacts: &[],
            pending: &[],
            verified: true,
        });
        let mut todos = TodoList::default();
        let item = todos.add_host("write chapter one").unwrap();
        store
            .save("s", &session.snapshot(), Some(&todos), None)
            .unwrap();

        todos
            .update_status(item, keel_core::TodoStatus::Completed)
            .unwrap();
        session.set_summary("memo", &[0], 256);
        store
            .save("s", &session.snapshot(), Some(&todos), None)
            .unwrap();

        let current: (Option<i64>, i64, i64) = store
            .conn
            .query_row(
                "SELECT summary_revision, tasks_completed, tasks_total
                 FROM session_current WHERE session_id = 's'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(current, (Some(1), 1, 1));

        let segments: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM segments", [], |row| row.get(0))
            .unwrap();
        assert_eq!(segments, 1, "the projection must not duplicate rows");
    }

    #[test]
    fn a_context_snapshot_is_stored_with_the_turn() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let session = keel_core::Session::new();
        let context = keel_core::ContextSnapshot {
            estimate: 12,
            budget: 100,
            stages: vec!["system".to_owned(), "latest".to_owned()],
            messages: vec![keel_core::ContextMessage {
                role: "user".to_owned(),
                content: "hello".to_owned(),
            }],
        };
        let turn = StoredTurn {
            seq: 1,
            input: "hello",
            state: "done",
            answer: "hi",
            answer_json: "{}",
            usage_json: "{}",
            duration_ms: 5,
            context: Some(&context),
            error: None,
            events: &[],
        };
        store
            .save("s", &session.snapshot(), None, Some(turn))
            .unwrap();

        let (estimate, messages): (i64, String) = store
            .conn
            .query_row(
                "SELECT estimate, messages_json FROM context_snapshots
                 WHERE turn_id = 's:1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(estimate, 12);
        assert!(messages.contains("hello"));
    }

    #[test]
    fn fact_changes_are_appended_to_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let session = keel_core::Session::new();
        let mut snapshot = session.snapshot();
        snapshot.facts.push(keel_core::Fact {
            key: "project".to_owned(),
            statement: "project is blue whale".to_owned(),
            scope: keel_core::FactScope::Session,
            confirmed: true,
            segment_id: 0,
        });
        store.save("s", &snapshot, None, None).unwrap();

        snapshot.facts[0].statement = "project is seagull".to_owned();
        store.save("s", &snapshot, None, None).unwrap();

        snapshot.facts.clear();
        store.save("s", &snapshot, None, None).unwrap();

        let mut stmt = store
            .conn
            .prepare(
                "SELECT version, reason, active FROM fact_history
                 WHERE session_id = 's' ORDER BY version",
            )
            .unwrap();
        let rows: Vec<(i64, String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            rows,
            vec![
                (1, "added".to_owned(), 1),
                (2, "superseded".to_owned(), 1),
                (3, "forgotten".to_owned(), 0),
            ]
        );
    }

    #[test]
    fn task_changes_are_appended_to_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let session = keel_core::Session::new();
        let mut todos = TodoList::default();
        let item = todos.add_host("write chapter one").unwrap();
        store
            .save("s", &session.snapshot(), Some(&todos), None)
            .unwrap();

        todos
            .update_status(item, keel_core::TodoStatus::Completed)
            .unwrap();
        store
            .save("s", &session.snapshot(), Some(&todos), None)
            .unwrap();

        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM todo_history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let latest: String = store
            .conn
            .query_row(
                "SELECT snapshot_json FROM todo_history ORDER BY revision DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(latest.contains("Completed"));
    }

    #[test]
    fn a_failed_save_leaves_the_previous_turn_intact() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let session = keel_core::Session::new();
        let first = StoredTurn {
            seq: 1,
            input: "hello",
            state: "done",
            answer: "hi",
            answer_json: "{}",
            usage_json: "{}",
            duration_ms: 5,
            context: None,
            error: None,
            events: &[],
        };
        store
            .save("s", &session.snapshot(), None, Some(first))
            .unwrap();

        // Fail the next save after the session and turn rows are written, so
        // the test proves the transaction rolls back rather than leaving a
        // half-applied turn behind.
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER reject_facts BEFORE INSERT ON facts
                 BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
            .unwrap();
        let mut snapshot = session.snapshot();
        snapshot.facts.push(keel_core::Fact {
            key: "project".to_owned(),
            statement: "project is blue whale".to_owned(),
            scope: keel_core::FactScope::Session,
            confirmed: true,
            segment_id: 0,
        });
        let second = StoredTurn {
            seq: 2,
            input: "next",
            state: "done",
            answer: "next done",
            answer_json: "{}",
            usage_json: "{}",
            duration_ms: 5,
            context: None,
            error: None,
            events: &[],
        };
        let error = store.save("s", &snapshot, None, Some(second)).unwrap_err();
        assert!(
            format!("{error:?}").contains("injected failure"),
            "the injected fault must be the failure: {error:?}"
        );

        let loaded = store.load("s").unwrap().unwrap();
        assert!(loaded.snapshot.facts.is_empty());
        let turns: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(turns, 1, "the failed turn must not be visible");
        let state: String = store
            .conn
            .query_row(
                "SELECT last_turn_state FROM session_current WHERE session_id = 's'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "done");
    }

    #[test]
    fn a_corrupt_snapshot_fails_as_a_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        store
            .save("s", &keel_core::Session::new().snapshot(), None, None)
            .unwrap();
        store
            .conn
            .execute("UPDATE sessions SET snapshot_json = '{' WHERE id = 's'", [])
            .unwrap();

        let error = store.load("s").err().expect("load must not panic");
        match error {
            KernelError::InvalidConfig { message } => {
                assert!(message.contains("invalid session snapshot"), "{message}");
            }
            other => panic!("unexpected error: {other:?}"),
        }

        store
            .save(
                "s",
                &keel_core::Session::new().snapshot(),
                Some(&TodoList::default()),
                None,
            )
            .unwrap();
        store
            .conn
            .execute("UPDATE sessions SET todos_json = 'nope' WHERE id = 's'", [])
            .unwrap();
        let error = store.load("s").err().expect("load must not panic");
        match error {
            KernelError::InvalidConfig { message } => {
                assert!(message.contains("invalid task list"), "{message}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    fn turn(seq: u32) -> StoredTurn<'static> {
        StoredTurn {
            seq,
            input: "prompt",
            state: "done",
            answer: "answer",
            answer_json: "{}",
            usage_json: "{}",
            duration_ms: 5,
            context: None,
            error: None,
            events: &[],
        }
    }

    fn recorded(session: &mut keel_core::Session, input: &str) {
        session.record_turn(keel_core::TurnRecord {
            input,
            text: "done",
            tools: &[],
            artifacts: &[],
            pending: &[],
            verified: true,
        });
    }

    #[test]
    fn each_turn_writes_a_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let mut session = keel_core::Session::new();
        recorded(&mut session, "one");
        store
            .save("s", &session.snapshot(), None, Some(turn(1)))
            .unwrap();

        let (seq, json): (i64, String) = store
            .conn
            .query_row(
                "SELECT turn_seq, snapshot_json FROM session_checkpoints
                 WHERE session_id = 's'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(seq, 1);
        let snapshot: SessionSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snapshot.segments.len(), 1);
    }

    #[test]
    fn the_default_checkpoint_cap_bounds_the_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let mut session = keel_core::Session::new();
        for seq in 1..=(DEFAULT_CHECKPOINT_LIMIT + 5) {
            recorded(&mut session, "turn");
            store
                .save("s", &session.snapshot(), None, Some(turn(seq)))
                .unwrap();
        }

        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM session_checkpoints WHERE session_id = 's'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(count, i64::from(DEFAULT_CHECKPOINT_LIMIT));
    }

    #[test]
    fn a_fork_resumes_from_the_checkpoint_and_copies_the_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let mut session = keel_core::Session::new();
        let mut todos = TodoList::default();
        todos.add_host("write chapter one").unwrap();

        recorded(&mut session, "one");
        let first = session.snapshot();
        store
            .save("parent", &first, Some(&todos), Some(turn(1)))
            .unwrap();
        recorded(&mut session, "two");
        store
            .save("parent", &session.snapshot(), Some(&todos), Some(turn(2)))
            .unwrap();

        store.fork("parent", 1, "child").unwrap();

        let child = store.load("child").unwrap().unwrap();
        assert_eq!(child.snapshot.segments.len(), first.segments.len());
        assert_eq!(
            child.snapshot.segments[0].entries.len(),
            first.segments[0].entries.len(),
            "the branch resumes at the checkpoint, not at the parent's head"
        );
        assert_eq!(child.todos.unwrap().items.len(), 1);
        let meta: (Option<String>, Option<i64>) = store
            .conn
            .query_row(
                "SELECT parent_session_id, fork_turn_seq FROM sessions WHERE id = 'child'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(meta, (Some("parent".to_owned()), Some(1)));
        let turns: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM turns WHERE session_id = 'child'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(turns, 1);
        let segments: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM segments WHERE session_id = 'child'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(segments, 1, "the branch projection is rebuilt");

        // The branch continues on its own without touching the parent.
        store
            .save("child", &child.snapshot, None, Some(turn(2)))
            .unwrap();
        let parent_turns: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM turns WHERE session_id = 'parent'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(parent_turns, 2);
    }

    #[test]
    fn a_fork_needs_a_checkpoint_and_a_free_target() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let mut session = keel_core::Session::new();
        recorded(&mut session, "one");
        store
            .save("parent", &session.snapshot(), None, Some(turn(1)))
            .unwrap();

        let missing = store.fork("parent", 3, "child").unwrap_err();
        assert!(format!("{missing:?}").contains("no checkpoint at turn 3"));

        store
            .save("child", &session.snapshot(), None, Some(turn(1)))
            .unwrap();
        let taken = store.fork("parent", 1, "child").unwrap_err();
        assert!(format!("{taken:?}").contains("already exists"));

        let same = store.fork("parent", 1, "parent").unwrap_err();
        assert!(format!("{same:?}").contains("different session id"));
    }

    #[test]
    fn old_checkpoints_are_pruned_but_fork_points_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let mut session = keel_core::Session::new();
        for seq in 1..=4 {
            recorded(&mut session, "turn");
            store
                .save("parent", &session.snapshot(), None, Some(turn(seq)))
                .unwrap();
        }
        store.fork("parent", 1, "child").unwrap();

        store.set_checkpoint_limit(2);
        recorded(&mut session, "turn");
        store
            .save("parent", &session.snapshot(), None, Some(turn(5)))
            .unwrap();

        let seqs: Vec<i64> = store
            .conn
            .prepare(
                "SELECT turn_seq FROM session_checkpoints
                 WHERE session_id = 'parent' ORDER BY turn_seq",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(seqs, vec![1, 4, 5], "newest two plus the fork point");

        let pruned = store.fork("parent", 2, "another").unwrap_err();
        assert!(format!("{pruned:?}").contains("no checkpoint at turn 2"));
    }

    #[test]
    fn deleting_a_session_removes_its_rows_but_not_its_branches() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let mut session = keel_core::Session::new();
        recorded(&mut session, "one");
        let mut todos = TodoList::default();
        todos.add_host("write chapter one").unwrap();
        let mut snapshot = session.snapshot();
        snapshot.facts.push(keel_core::Fact {
            key: "project".to_owned(),
            statement: "project is blue whale".to_owned(),
            scope: keel_core::FactScope::Session,
            confirmed: true,
            segment_id: 0,
        });
        store
            .save("parent", &snapshot, Some(&todos), Some(turn(1)))
            .unwrap();
        store.fork("parent", 1, "child").unwrap();

        assert!(store.delete_session("parent").unwrap());
        assert!(!store.delete_session("parent").unwrap());
        assert!(store.load("parent").unwrap().is_none());
        assert!(
            store.load("child").unwrap().is_some(),
            "a branch outlives its parent"
        );

        for table in [
            "turns",
            "session_checkpoints",
            "segments",
            "segment_entries",
            "records",
            "session_summaries",
            "summary_covers",
            "facts",
            "fact_history",
            "todos",
            "todo_history",
            "context_snapshots",
        ] {
            let count: i64 = store
                .conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE session_id = 'parent'"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "{table} still has rows");
        }
    }

    #[test]
    fn events_and_errors_are_stored_with_the_turn() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let session = keel_core::Session::new();
        let events = vec![
            ("turn_started".to_owned(), r#"{"input":"hi"}"#.to_owned()),
            (
                "permission_decided".to_owned(),
                r#"{"decision":"allow"}"#.to_owned(),
            ),
        ];
        let turn = StoredTurn {
            seq: 1,
            input: "hi",
            state: "Error",
            answer: "",
            answer_json: "{}",
            usage_json: "{}",
            duration_ms: 3,
            error: Some("model failure (model_provider)"),
            context: None,
            events: &events,
        };
        store
            .save("s", &session.snapshot(), None, Some(turn))
            .unwrap();

        let error: String = store
            .conn
            .query_row("SELECT error FROM turns WHERE seq = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(error, "model failure (model_provider)");
        let names: Vec<String> = {
            let mut statement = store
                .conn
                .prepare("SELECT name FROM events ORDER BY seq")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(names, vec!["turn_started", "permission_decided"]);

        store.fork("s", 1, "branch").unwrap();
        let copied: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE session_id = 'branch'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(copied, 2, "a branch keeps the events of its turns");
    }

    #[test]
    #[ignore = "timing gate; run in release"]
    fn saving_turns_with_events_stays_bounded() {
        use std::time::{Duration, Instant};

        fn p(values: &[Duration], fraction: f64) -> Duration {
            let mut sorted = values.to_vec();
            sorted.sort();
            sorted[((sorted.len() as f64 - 1.0) * fraction).round() as usize]
        }

        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let session = keel_core::Session::new();
        let events: Vec<(String, String)> = (0..50)
            .map(|index| (format!("event_{index}"), r#"{"x":1}"#.to_owned()))
            .collect();

        let mut serialized = Vec::new();
        for _ in 0..100 {
            let started = Instant::now();
            for (name, payload) in &events {
                let _ = serde_json::to_string(&(name, payload)).unwrap();
            }
            serialized.push(started.elapsed());
        }

        let mut with = Vec::new();
        for seq in 1..=100 {
            let turn = StoredTurn {
                seq,
                input: "hi",
                state: "Done",
                answer: "ok",
                answer_json: "{}",
                usage_json: "{}",
                duration_ms: 1,
                error: None,
                context: None,
                events: &events,
            };
            let started = Instant::now();
            store
                .save("s", &session.snapshot(), None, Some(turn))
                .unwrap();
            with.push(started.elapsed());
        }
        let mut without = Vec::new();
        for seq in 101..=200 {
            let turn = StoredTurn {
                seq,
                input: "hi",
                state: "Done",
                answer: "ok",
                answer_json: "{}",
                usage_json: "{}",
                duration_ms: 1,
                error: None,
                context: None,
                events: &[],
            };
            let started = Instant::now();
            store
                .save("s", &session.snapshot(), None, Some(turn))
                .unwrap();
            without.push(started.elapsed());
        }
        println!(
            "serialize 50 events: p50={:.3} ms",
            p(&serialized, 0.50).as_secs_f64() * 1000.0
        );
        println!(
            "save with 50 events: p50={:.3} ms p99={:.3} ms",
            p(&with, 0.50).as_secs_f64() * 1000.0,
            p(&with, 0.99).as_secs_f64() * 1000.0
        );
        println!(
            "save without events: p50={:.3} ms p99={:.3} ms",
            p(&without, 0.50).as_secs_f64() * 1000.0,
            p(&without, 0.99).as_secs_f64() * 1000.0
        );
        assert!(
            p(&with, 0.99) < Duration::from_millis(100),
            "event persistence exceeded 100 ms"
        );
    }

    #[test]
    fn an_unknown_session_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("keel.db")).unwrap();
        assert!(store.load("missing").unwrap().is_none());
    }

    #[test]
    fn turns_are_appended_with_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let session = keel_core::Session::new();
        let turn = StoredTurn {
            seq: 1,
            input: "hello",
            state: "done",
            answer: "hi",
            answer_json: "{}",
            usage_json: "{}",
            duration_ms: 5,
            context: None,
            error: None,
            events: &[],
        };
        store
            .save("s", &session.snapshot(), None, Some(turn))
            .unwrap();
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        let state: String = store
            .conn
            .query_row(
                "SELECT last_turn_state FROM session_current WHERE session_id = 's'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "done");
    }
}
