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

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use keel_core::{KernelError, SessionSnapshot, TodoList};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

const SCHEMA_VERSION: i64 = 2;

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
    pub usage_json: &'a str,
    pub duration_ms: u64,
}

/// A SQLite-backed host store.
pub struct Store {
    conn: Connection,
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
                 todos_json TEXT
             );
             CREATE TABLE IF NOT EXISTS turns (
                 id TEXT PRIMARY KEY,
                 session_id TEXT NOT NULL REFERENCES sessions(id),
                 seq INTEGER NOT NULL,
                 input TEXT NOT NULL,
                 state TEXT NOT NULL,
                 answer TEXT NOT NULL,
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
             CREATE VIEW IF NOT EXISTS session_current AS
             SELECT s.id AS session_id,
                    s.updated_at AS updated_at,
                    (SELECT MAX(seq) FROM turns t WHERE t.session_id = s.id)
                        AS last_turn_seq,
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
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (?1, ?2)",
            params![SCHEMA_VERSION, now()],
        )
        .map_err(|error| store_error("record schema version", error))?;
        Ok(Self { conn })
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
                 (id, session_id, seq, input, state, answer, usage_json, duration_ms, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    format!("{session_id}:{}", turn.seq),
                    session_id,
                    turn.seq,
                    turn.input,
                    turn.state,
                    turn.answer,
                    turn.usage_json,
                    turn.duration_ms,
                    now,
                ],
            )
            .map_err(|error| store_error("write turn", error))?;
        }
        project(&tx, session_id, snapshot, todos, now)
            .map_err(|error| store_error("project session", error))?;
        tx.commit().map_err(|error| store_error("commit", error))?;
        Ok(())
    }
}

/// Rewrites the normalized read model from the snapshot. The rows are a
/// projection, so a full replace cannot leave a stale segment or task behind.
fn project(
    tx: &Transaction<'_>,
    session_id: &str,
    snapshot: &SessionSnapshot,
    todos: Option<&TodoList>,
    now: i64,
) -> rusqlite::Result<()> {
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
                  verified, folded, absorbed_in)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    session_id,
                    segment.id,
                    record.goal,
                    record.result,
                    changes,
                    pending,
                    i64::from(record.verified),
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
            usage_json: "{}",
            duration_ms: 5,
        };
        store
            .save("s", &session.snapshot(), None, Some(turn))
            .unwrap();
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
