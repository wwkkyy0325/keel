//! SQLite session store for the CLI host.
//!
//! The kernel owns the session types; this adapter only moves a snapshot in and
//! out of a file. Per ADR 0008 the snapshot is the derived plane: segments,
//! records, the memo and facts. The transcript rows are appended per turn so a
//! later frontend or audit can replay what happened.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use keel_core::{KernelError, SessionSnapshot, TodoList};
use rusqlite::{params, Connection, OptionalExtension};

const SCHEMA_VERSION: i64 = 1;

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
             );",
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
        tx.commit().map_err(|error| store_error("commit", error))?;
        Ok(())
    }
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
