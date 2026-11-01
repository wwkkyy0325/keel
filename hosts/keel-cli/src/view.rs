//! Composed read models for a frontend.
//!
//! The host performs the joins, ordering and JSON decoding so a frontend can
//! render values instead of deriving state. Every list has an explicit
//! `ORDER BY`, and the field order is fixed by these structs, so the same store
//! produces the same view. A frontend test can therefore assert this JSON
//! without starting the UI.

use keel_core::KernelError;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// One row of the session list.
#[derive(Debug, Serialize)]
pub struct SessionSummaryView {
    pub id: String,
    pub updated_at: i64,
    pub turns: u32,
    pub last_turn_state: Option<String>,
    pub summary_revision: Option<u32>,
    pub current_segment_id: Option<u32>,
    pub active_facts: u32,
    pub tasks_completed: u32,
    pub tasks_total: u32,
    pub parent_session_id: Option<String>,
    pub fork_turn_seq: Option<u32>,
}

/// Everything a session screen renders.
#[derive(Debug, Serialize)]
pub struct SessionView {
    pub session: SessionSummaryView,
    pub memo: Option<MemoView>,
    pub facts: Vec<FactView>,
    pub tasks: Vec<TaskView>,
    pub branches: Vec<BranchView>,
    pub turns: Vec<TurnView>,
}

#[derive(Debug, Serialize)]
pub struct MemoView {
    pub revision: u32,
    pub text: String,
    pub budget_tokens: u32,
    pub covers: Vec<u32>,
}

#[derive(Debug, Serialize)]
pub struct FactView {
    pub scope: String,
    pub key: String,
    pub statement: String,
    pub confirmed: bool,
    pub segment_id: u32,
}

#[derive(Debug, Serialize)]
pub struct TaskView {
    pub item_id: u32,
    pub text: String,
    pub status: String,
    pub source: String,
    pub position: u32,
}

#[derive(Debug, Serialize)]
pub struct BranchView {
    pub id: String,
    pub fork_turn_seq: u32,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct TurnView {
    pub seq: u32,
    pub state: String,
    pub input: String,
    pub answer: String,
    pub answer_model: AnswerView,
    pub usage: UsageView,
    pub duration_ms: u64,
    pub created_at: i64,
}

/// The structured answer, decoded by the host.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AnswerView {
    pub revision: u32,
    pub body: String,
    pub changes: Vec<String>,
    pub pending: Vec<String>,
    pub verification: Option<VerificationView>,
    pub notes: Option<String>,
    pub missing: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerificationView {
    pub passed: bool,
    pub required_failed: Vec<String>,
    pub required_unverified: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageView {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
}

/// The list screen: newest first, then id for a stable tie-break.
pub fn sessions(conn: &Connection) -> Result<Vec<SessionSummaryView>, KernelError> {
    let mut statement = conn
        .prepare(
            "SELECT c.session_id, c.updated_at, c.turns, c.last_turn_state,
                    c.summary_revision, c.current_segment_id, c.active_facts,
                    c.tasks_completed, c.tasks_total,
                    s.parent_session_id, s.fork_turn_seq
             FROM session_current c
             JOIN sessions s ON s.id = c.session_id
             ORDER BY c.updated_at DESC, c.session_id ASC",
        )
        .map_err(view_error)?;
    let rows = statement
        .query_map([], summary_from_row)
        .map_err(view_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(view_error)
}

/// The session screen, or `None` when the id does not exist.
pub fn session(conn: &Connection, id: &str) -> Result<Option<SessionView>, KernelError> {
    let session = conn
        .query_row(
            "SELECT c.session_id, c.updated_at, c.turns, c.last_turn_state,
                    c.summary_revision, c.current_segment_id, c.active_facts,
                    c.tasks_completed, c.tasks_total,
                    s.parent_session_id, s.fork_turn_seq
             FROM session_current c
             JOIN sessions s ON s.id = c.session_id
             WHERE c.session_id = ?1",
            [id],
            summary_from_row,
        )
        .optional()
        .map_err(view_error)?;
    let Some(session) = session else {
        return Ok(None);
    };
    Ok(Some(SessionView {
        session,
        memo: memo(conn, id)?,
        facts: facts(conn, id)?,
        tasks: tasks(conn, id)?,
        branches: branches(conn, id)?,
        turns: turns(conn, id)?,
    }))
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummaryView> {
    Ok(SessionSummaryView {
        id: row.get(0)?,
        updated_at: row.get(1)?,
        turns: count(row.get(2)?),
        last_turn_state: row.get(3)?,
        summary_revision: row.get::<_, Option<i64>>(4)?.map(count),
        current_segment_id: row.get::<_, Option<i64>>(5)?.map(count),
        active_facts: count(row.get(6)?),
        tasks_completed: count(row.get(7)?),
        tasks_total: count(row.get(8)?),
        parent_session_id: row.get(9)?,
        fork_turn_seq: row.get::<_, Option<i64>>(10)?.map(count),
    })
}

fn memo(conn: &Connection, id: &str) -> Result<Option<MemoView>, KernelError> {
    let memo: Option<(u32, String, u32)> = conn
        .query_row(
            "SELECT revision, text, budget_tokens FROM session_summaries
             WHERE session_id = ?1 ORDER BY revision DESC LIMIT 1",
            [id],
            |row| Ok((count(row.get(0)?), row.get(1)?, count(row.get(2)?))),
        )
        .optional()
        .map_err(view_error)?;
    let Some((revision, text, budget_tokens)) = memo else {
        return Ok(None);
    };
    let mut statement = conn
        .prepare(
            "SELECT segment_id FROM summary_covers
             WHERE session_id = ?1 AND revision = ?2 ORDER BY segment_id",
        )
        .map_err(view_error)?;
    let covers = statement
        .query_map(rusqlite::params![id, revision], |row| {
            Ok(count(row.get(0)?))
        })
        .map_err(view_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(view_error)?;
    Ok(Some(MemoView {
        revision,
        text,
        budget_tokens,
        covers,
    }))
}

fn facts(conn: &Connection, id: &str) -> Result<Vec<FactView>, KernelError> {
    let mut statement = conn
        .prepare(
            "SELECT scope, key, statement, confirmed, segment_id FROM facts
             WHERE session_id = ?1 AND active = 1 ORDER BY scope, key",
        )
        .map_err(view_error)?;
    let rows = statement
        .query_map([id], |row| {
            Ok(FactView {
                scope: row.get(0)?,
                key: row.get(1)?,
                statement: row.get(2)?,
                confirmed: row.get(3)?,
                segment_id: count(row.get(4)?),
            })
        })
        .map_err(view_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(view_error)
}

fn tasks(conn: &Connection, id: &str) -> Result<Vec<TaskView>, KernelError> {
    let mut statement = conn
        .prepare(
            "SELECT item_id, text, status, source, position FROM todos
             WHERE session_id = ?1 ORDER BY position",
        )
        .map_err(view_error)?;
    let rows = statement
        .query_map([id], |row| {
            Ok(TaskView {
                item_id: count(row.get(0)?),
                text: row.get(1)?,
                status: row.get(2)?,
                source: row.get(3)?,
                position: count(row.get(4)?),
            })
        })
        .map_err(view_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(view_error)
}

fn branches(conn: &Connection, id: &str) -> Result<Vec<BranchView>, KernelError> {
    let mut statement = conn
        .prepare(
            "SELECT id, fork_turn_seq, updated_at FROM sessions
             WHERE parent_session_id = ?1 AND fork_turn_seq IS NOT NULL
             ORDER BY updated_at, id",
        )
        .map_err(view_error)?;
    let rows = statement
        .query_map([id], |row| {
            Ok(BranchView {
                id: row.get(0)?,
                fork_turn_seq: count(row.get(1)?),
                updated_at: row.get(2)?,
            })
        })
        .map_err(view_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(view_error)
}

fn turns(conn: &Connection, id: &str) -> Result<Vec<TurnView>, KernelError> {
    let mut statement = conn
        .prepare(
            "SELECT seq, state, input, answer, answer_json, usage_json,
                    duration_ms, created_at
             FROM turns WHERE session_id = ?1 ORDER BY seq",
        )
        .map_err(view_error)?;
    let rows = statement
        .query_map([id], |row| {
            let answer_json: String = row.get(4)?;
            let usage_json: String = row.get(5)?;
            let answer: String = row.get(3)?;
            Ok(TurnView {
                seq: count(row.get(0)?),
                state: row.get(1)?,
                input: row.get(2)?,
                answer: answer.clone(),
                answer_model: decode_answer(&answer_json, answer),
                usage: decode_usage(&usage_json),
                duration_ms: u64::try_from(row.get::<_, i64>(6)?).unwrap_or(0),
                created_at: row.get(7)?,
            })
        })
        .map_err(view_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(view_error)
}

fn decode_answer(raw: &str, fallback_body: String) -> AnswerView {
    serde_json::from_str(raw).unwrap_or_else(|_| AnswerView {
        body: fallback_body,
        ..AnswerView::default()
    })
}

fn decode_usage(raw: &str) -> UsageView {
    serde_json::from_str(raw).unwrap_or_default()
}

fn count(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn view_error(error: rusqlite::Error) -> KernelError {
    KernelError::Io {
        message: format!("read view: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Store, StoredTurn};
    use keel_core::{Fact, FactScope, Session, TodoList, TodoStatus, TurnRecord};

    fn turn(seq: u32, answer_json: &'static str) -> StoredTurn<'static> {
        StoredTurn {
            seq,
            input: "prompt",
            state: "done",
            answer: "answer",
            answer_json,
            usage_json: r#"{"input":10,"output":5,"cached":4}"#,
            duration_ms: 12,
            context: None,
        }
    }

    fn seed() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("keel.db")).unwrap();
        let mut session = Session::new();
        let mut todos = TodoList::default();
        let item = todos.add_host("write chapter one").unwrap();

        session.record_turn(TurnRecord {
            input: "one",
            text: "first",
            tools: &[],
            artifacts: &[],
            pending: &[],
            verified: true,
        });
        let fact = Fact {
            key: "project".to_owned(),
            statement: "project is blue whale".to_owned(),
            scope: FactScope::Session,
            confirmed: true,
            segment_id: 0,
        };
        let mut first = session.snapshot();
        first.facts.push(fact.clone());
        store
            .save(
                "parent",
                &first,
                Some(&todos),
                Some(turn(
                    1,
                    r#"{"revision":1,"body":"first","changes":["a.py"],"pending":[],"verification":null,"notes":null,"missing":false}"#,
                )),
            )
            .unwrap();

        session.record_turn(TurnRecord {
            input: "two",
            text: "second",
            tools: &[],
            artifacts: &[],
            pending: &[],
            verified: true,
        });
        session.set_summary("memo text", &[0], 256);
        todos.update_status(item, TodoStatus::Completed).unwrap();
        let mut second = session.snapshot();
        second.facts.push(fact);
        store
            .save(
                "parent",
                &second,
                Some(&todos),
                Some(turn(
                    2,
                    r#"{"revision":1,"body":"second","changes":[],"pending":[],"verification":{"passed":true,"required_failed":[],"required_unverified":[]},"notes":null,"missing":false}"#,
                )),
            )
            .unwrap();
        store.fork("parent", 1, "child").unwrap();
        (dir, store)
    }

    #[test]
    fn the_session_list_is_composed_and_stable() {
        let (_dir, store) = seed();
        let first = sessions(store.connection()).unwrap();
        let second = sessions(store.connection()).unwrap();
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap(),
            "the same store must produce the same view"
        );
        assert_eq!(first.len(), 2);

        let parent = first.iter().find(|row| row.id == "parent").unwrap();
        assert_eq!(parent.turns, 2);
        assert_eq!(parent.last_turn_state.as_deref(), Some("done"));
        assert_eq!(parent.summary_revision, Some(1));
        assert_eq!(parent.current_segment_id, Some(0));
        assert_eq!(parent.active_facts, 1);
        assert_eq!(parent.tasks_completed, 1);
        assert_eq!(parent.tasks_total, 1);
        assert_eq!(parent.parent_session_id, None);

        let child = first.iter().find(|row| row.id == "child").unwrap();
        assert_eq!(child.turns, 1);
        assert_eq!(child.parent_session_id.as_deref(), Some("parent"));
        assert_eq!(child.fork_turn_seq, Some(1));
    }

    #[test]
    fn a_session_view_carries_every_panel_the_ui_renders() {
        let (_dir, store) = seed();
        let view = session(store.connection(), "parent").unwrap().unwrap();
        assert_eq!(view.session.id, "parent");
        assert_eq!(view.turns.len(), 2);
        assert_eq!(view.turns[0].seq, 1);
        assert_eq!(view.turns[0].answer_model.body, "first");
        assert_eq!(view.turns[0].answer_model.changes, vec!["a.py"]);
        assert_eq!(view.turns[0].usage.input, 10);
        assert_eq!(view.turns[0].usage.cached, 4);
        assert_eq!(
            view.turns[1]
                .answer_model
                .verification
                .as_ref()
                .map(|report| report.passed),
            Some(true)
        );

        let memo = view.memo.unwrap();
        assert_eq!(memo.revision, 1);
        assert_eq!(memo.text, "memo text");
        assert_eq!(memo.covers, vec![0]);

        assert_eq!(view.facts.len(), 1);
        assert_eq!(view.facts[0].key, "project");
        assert_eq!(view.tasks.len(), 1);
        assert_eq!(view.tasks[0].status, "completed");
        assert_eq!(view.tasks[0].position, 1);
        assert_eq!(view.branches.len(), 1);
        assert_eq!(view.branches[0].id, "child");
        assert_eq!(view.branches[0].fork_turn_seq, 1);
    }

    #[test]
    fn a_branch_view_starts_at_the_fork_state() {
        let (_dir, store) = seed();
        let view = session(store.connection(), "child").unwrap().unwrap();
        assert_eq!(view.session.parent_session_id.as_deref(), Some("parent"));
        assert_eq!(view.session.fork_turn_seq, Some(1));
        assert_eq!(view.turns.len(), 1);
        assert_eq!(view.turns[0].answer_model.body, "first");
        assert_eq!(view.facts.len(), 1);
        assert_eq!(view.tasks.len(), 1);
        assert_eq!(view.tasks[0].status, "pending");
        assert!(view.memo.is_none());
        assert!(view.branches.is_empty());
    }

    #[test]
    fn an_unknown_session_has_no_view() {
        let (_dir, store) = seed();
        assert!(session(store.connection(), "missing").unwrap().is_none());
    }
}
