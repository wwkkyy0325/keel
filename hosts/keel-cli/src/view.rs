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
    pub records: Vec<RecordView>,
    pub turns: Vec<TurnView>,
}

/// One sealed segment's cumulative line.
#[derive(Debug, Serialize)]
pub struct RecordView {
    pub segment_id: u32,
    pub segment_seq: u32,
    pub topic: String,
    pub goal: String,
    pub result: String,
    pub changes: Vec<String>,
    pub pending: Vec<String>,
    pub verified: bool,
    pub turns: u32,
    pub folded: bool,
    pub absorbed_in: Option<u32>,
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
    /// Kernel error text and code, when the turn did not finish.
    pub error: Option<String>,
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

/// One turn with the exact message list it started from, when captured.
#[derive(Debug, Serialize)]
pub struct TurnDetailView {
    pub turn: TurnView,
    pub context: Option<ContextView>,
    pub events: Vec<EventView>,
}

/// One persisted kernel event.
#[derive(Debug, Serialize)]
pub struct EventView {
    pub turn: u32,
    pub seq: u32,
    pub name: String,
    pub payload: serde_json::Value,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContextView {
    pub estimate: u32,
    pub budget: u32,
    pub stages: Vec<String>,
    pub messages: Vec<ContextMessageView>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContextMessageView {
    pub role: String,
    pub content: String,
}

/// The memory screen: every fact version and every task revision.
#[derive(Debug, Serialize)]
pub struct MemoryView {
    pub facts: Vec<FactEventView>,
    pub tasks: Vec<TaskRevisionView>,
}

#[derive(Debug, Serialize)]
pub struct FactEventView {
    pub scope: String,
    pub key: String,
    pub version: u32,
    pub statement: String,
    pub active: bool,
    pub reason: String,
    pub changed_at: i64,
}

#[derive(Debug, Serialize)]
pub struct TaskRevisionView {
    pub revision: u32,
    pub created_at: i64,
    pub items: Vec<HistoricTaskView>,
}

#[derive(Debug, Serialize)]
pub struct HistoricTaskView {
    pub id: u32,
    pub text: String,
    pub status: String,
    pub source: String,
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

/// How many turns to return and where to stop, for long sessions.
///
/// `limit = 0` returns every turn, `before` is an exclusive sequence cursor:
/// the next page passes the smallest `seq` it received. The result is always
/// ascending, so the newest page reads top to bottom.
#[derive(Debug, Clone, Copy, Default)]
pub struct TurnWindow {
    pub limit: u32,
    pub before: Option<u32>,
}

/// The session screen, or `None` when the id does not exist.
///
/// The turn list is bounded by `window`; the summary still reports the total.
pub fn session_window(
    conn: &Connection,
    id: &str,
    window: TurnWindow,
) -> Result<Option<SessionView>, KernelError> {
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
        records: records(conn, id)?,
        turns: turns(conn, id, window)?,
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

fn records(conn: &Connection, id: &str) -> Result<Vec<RecordView>, KernelError> {
    let mut statement = conn
        .prepare(
            "SELECT r.segment_id, g.seq, g.topic, r.goal, r.result,
                    r.changes_json, r.pending_json, r.verified, r.turns,
                    r.folded, r.absorbed_in
             FROM records r
             JOIN segments g ON g.session_id = r.session_id AND g.id = r.segment_id
             WHERE r.session_id = ?1 ORDER BY g.seq",
        )
        .map_err(view_error)?;
    let rows = statement
        .query_map([id], |row| {
            let changes: String = row.get(5)?;
            let pending: String = row.get(6)?;
            Ok(RecordView {
                segment_id: count(row.get(0)?),
                segment_seq: count(row.get(1)?),
                topic: row.get(2)?,
                goal: row.get(3)?,
                result: row.get(4)?,
                changes: decode_strings(&changes),
                pending: decode_strings(&pending),
                verified: row.get(7)?,
                turns: count(row.get(8)?),
                folded: row.get(9)?,
                absorbed_in: row.get::<_, Option<i64>>(10)?.map(count),
            })
        })
        .map_err(view_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(view_error)
}

fn turns(conn: &Connection, id: &str, window: TurnWindow) -> Result<Vec<TurnView>, KernelError> {
    let mut statement = conn
        .prepare(
            "SELECT seq, state, input, answer, answer_json, usage_json,
                    duration_ms, created_at, error
             FROM turns
             WHERE session_id = ?1
               AND seq < COALESCE(?2, 9223372036854775807)
             ORDER BY seq DESC
             LIMIT ?3",
        )
        .map_err(view_error)?;
    let limit = if window.limit == 0 {
        -1
    } else {
        i64::from(window.limit)
    };
    let rows = statement
        .query_map(rusqlite::params![id, window.before, limit], turn_from_row)
        .map_err(view_error)?;
    let mut turns = rows.collect::<Result<Vec<_>, _>>().map_err(view_error)?;
    turns.reverse();
    Ok(turns)
}

fn turn_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TurnView> {
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
        error: row.get(8)?,
    })
}

/// One turn plus the context snapshot captured when it started.
pub fn turn(conn: &Connection, id: &str, seq: u32) -> Result<Option<TurnDetailView>, KernelError> {
    let turn = conn
        .query_row(
            "SELECT seq, state, input, answer, answer_json, usage_json,
                    duration_ms, created_at, error
             FROM turns WHERE session_id = ?1 AND seq = ?2",
            rusqlite::params![id, seq],
            turn_from_row,
        )
        .optional()
        .map_err(view_error)?;
    let Some(turn) = turn else {
        return Ok(None);
    };
    let events = event_rows(conn, id, Some(seq))?;
    let context = conn
        .query_row(
            "SELECT estimate, budget, stages_json, messages_json
             FROM context_snapshots WHERE turn_id = ?1",
            [format!("{id}:{seq}")],
            |row| {
                let stages: String = row.get(2)?;
                let messages: String = row.get(3)?;
                Ok(ContextView {
                    estimate: count(row.get(0)?),
                    budget: count(row.get(1)?),
                    stages: serde_json::from_str(&stages).unwrap_or_default(),
                    messages: serde_json::from_str(&messages).unwrap_or_default(),
                })
            },
        )
        .optional()
        .map_err(view_error)?;
    Ok(Some(TurnDetailView {
        turn,
        context,
        events,
    }))
}

/// Kernel events for one session, paged by turns like the session view.
pub fn events(
    conn: &Connection,
    id: &str,
    window: TurnWindow,
) -> Result<Option<Vec<EventView>>, KernelError> {
    let exists: Option<i64> = conn
        .query_row("SELECT 1 FROM sessions WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(view_error)?;
    if exists.is_none() {
        return Ok(None);
    }
    if window.limit == 0 && window.before.is_none() {
        return event_rows(conn, id, None).map(Some);
    }
    // The window is described in turns; find the oldest turn of the page and
    // return every event in that range.
    let limit = if window.limit == 0 {
        -1
    } else {
        i64::from(window.limit)
    };
    let floor: Option<i64> = conn
        .query_row(
            "SELECT seq FROM turns
             WHERE session_id = ?1
               AND seq < COALESCE(?2, 9223372036854775807)
             ORDER BY seq DESC
             LIMIT ?3",
            rusqlite::params![id, window.before, limit],
            |row| row.get(0),
        )
        .optional()
        .map_err(view_error)?;
    let Some(floor) = floor else {
        return Ok(Some(Vec::new()));
    };
    let mut statement = conn
        .prepare(
            "SELECT turn_seq, seq, name, payload_json, created_at FROM events
             WHERE session_id = ?1
               AND turn_seq >= ?2
               AND turn_seq < COALESCE(?3, 9223372036854775807)
             ORDER BY turn_seq, seq",
        )
        .map_err(view_error)?;
    let rows = statement
        .query_map(rusqlite::params![id, floor, window.before], event_from_row)
        .map_err(view_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map(Some)
        .map_err(view_error)
}

fn event_rows(
    conn: &Connection,
    id: &str,
    turn: Option<u32>,
) -> Result<Vec<EventView>, KernelError> {
    let mut statement = conn
        .prepare(
            "SELECT turn_seq, seq, name, payload_json, created_at FROM events
             WHERE session_id = ?1 AND (?2 IS NULL OR turn_seq = ?2)
             ORDER BY turn_seq, seq",
        )
        .map_err(view_error)?;
    let rows = statement
        .query_map(rusqlite::params![id, turn], event_from_row)
        .map_err(view_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(view_error)
}

fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EventView> {
    let payload: String = row.get(3)?;
    Ok(EventView {
        turn: count(row.get(0)?),
        seq: count(row.get(1)?),
        name: row.get(2)?,
        payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
        created_at: row.get(4)?,
    })
}

/// The memory screen: fact versions and task revisions, oldest first.
pub fn memory(conn: &Connection, id: &str) -> Result<Option<MemoryView>, KernelError> {
    let exists: Option<i64> = conn
        .query_row("SELECT 1 FROM sessions WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(view_error)?;
    if exists.is_none() {
        return Ok(None);
    }
    let mut statement = conn
        .prepare(
            "SELECT scope, key, version, statement, active, reason, changed_at
             FROM fact_history WHERE session_id = ?1
             ORDER BY scope, key, version",
        )
        .map_err(view_error)?;
    let rows = statement
        .query_map([id], |row| {
            Ok(FactEventView {
                scope: row.get(0)?,
                key: row.get(1)?,
                version: count(row.get(2)?),
                statement: row.get(3)?,
                active: row.get::<_, i64>(4)? != 0,
                reason: row.get(5)?,
                changed_at: row.get(6)?,
            })
        })
        .map_err(view_error)?;
    let facts = rows.collect::<Result<Vec<_>, _>>().map_err(view_error)?;

    let mut statement = conn
        .prepare(
            "SELECT revision, snapshot_json, created_at FROM todo_history
             WHERE session_id = ?1 ORDER BY revision",
        )
        .map_err(view_error)?;
    let rows = statement
        .query_map([id], |row| {
            let snapshot: String = row.get(1)?;
            Ok(TaskRevisionView {
                revision: count(row.get(0)?),
                created_at: row.get(2)?,
                items: decode_tasks(&snapshot),
            })
        })
        .map_err(view_error)?;
    let tasks = rows.collect::<Result<Vec<_>, _>>().map_err(view_error)?;
    Ok(Some(MemoryView { facts, tasks }))
}

fn decode_strings(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn decode_tasks(raw: &str) -> Vec<HistoricTaskView> {
    let list: keel_core::TodoList = serde_json::from_str(raw).unwrap_or_default();
    list.items
        .into_iter()
        .map(|item| HistoricTaskView {
            id: item.id,
            text: item.text,
            status: item.status.as_str().to_owned(),
            source: item.source.as_str().to_owned(),
        })
        .collect()
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
    use keel_core::{
        ContextMessage, ContextSnapshot, Fact, FactScope, SegmentRecord, Session, TodoList,
        TodoStatus, TurnRecord,
    };

    fn turn<'a>(
        seq: u32,
        answer_json: &'a str,
        context: Option<&'a ContextSnapshot>,
        events: &'a [(String, String)],
    ) -> StoredTurn<'a> {
        StoredTurn {
            seq,
            input: "prompt",
            state: "done",
            answer: "answer",
            answer_json,
            usage_json: r#"{"input":10,"output":5,"cached":4}"#,
            duration_ms: 12,
            context,
            error: None,
            events,
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
        let record = SegmentRecord {
            goal: "write chapter one".to_owned(),
            result: "chapter one is done".to_owned(),
            changes: vec!["a.py".to_owned()],
            pending: Vec::new(),
            verified: true,
            turns: 1,
            folded: false,
            absorbed_in: None,
            memo: None,
            excerpt: None,
        };
        let context = ContextSnapshot {
            estimate: 100,
            budget: 200,
            stages: vec!["system".to_owned(), "latest".to_owned()],
            messages: vec![ContextMessage {
                role: "user".to_owned(),
                content: "prompt".to_owned(),
            }],
        };
        let first_events = vec![
            (
                "turn_started".to_owned(),
                r#"{"input":"prompt"}"#.to_owned(),
            ),
            ("turn_finished".to_owned(), r#"{"state":"Done"}"#.to_owned()),
        ];
        let mut first = session.snapshot();
        first.facts.push(fact.clone());
        first.segments[0].record = Some(record.clone());
        store
            .save(
                "parent",
                &first,
                Some(&todos),
                Some(turn(
                    1,
                    r#"{"revision":1,"body":"first","changes":["a.py"],"pending":[],"verification":null,"notes":null,"missing":false}"#,
                    Some(&context),
                    &first_events,
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
        second.segments[0].record = Some(record);
        store
            .save(
                "parent",
                &second,
                Some(&todos),
                Some(turn(
                    2,
                    r#"{"revision":1,"body":"second","changes":[],"pending":[],"verification":{"passed":true,"required_failed":[],"required_unverified":[]},"notes":null,"missing":false}"#,
                    None,
                    &[],
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
        let view = session_window(store.connection(), "parent", TurnWindow::default())
            .unwrap()
            .unwrap();
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

        assert_eq!(view.records.len(), 1);
        assert_eq!(view.records[0].goal, "write chapter one");
        assert_eq!(view.records[0].changes, vec!["a.py"]);
        assert!(view.records[0].verified);
        assert!(!view.records[0].folded);
    }

    #[test]
    fn a_turn_view_includes_the_captured_context() {
        let (_dir, store) = seed();
        let detail = super::turn(store.connection(), "parent", 1)
            .unwrap()
            .unwrap();
        assert_eq!(detail.turn.seq, 1);
        assert_eq!(detail.turn.answer_model.body, "first");
        let context = detail.context.unwrap();
        assert_eq!(context.estimate, 100);
        assert_eq!(context.budget, 200);
        assert_eq!(context.stages, vec!["system", "latest"]);
        assert_eq!(context.messages.len(), 1);
        assert_eq!(context.messages[0].role, "user");
        assert_eq!(context.messages[0].content, "prompt");

        assert_eq!(detail.events.len(), 2);
        assert_eq!(detail.events[0].name, "turn_started");
        assert_eq!(detail.events[0].payload["input"], "prompt");
        assert_eq!(detail.events[1].name, "turn_finished");
        assert!(detail.turn.error.is_none());

        let second = super::turn(store.connection(), "parent", 2)
            .unwrap()
            .unwrap();
        assert!(second.context.is_none());
        assert!(second.events.is_empty());
        assert!(super::turn(store.connection(), "parent", 9)
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_turn_window_pages_backward_in_ascending_order() {
        let (_dir, store) = seed();
        let conn = store.connection();

        let all = session_window(conn, "parent", TurnWindow::default())
            .unwrap()
            .unwrap();
        assert_eq!(
            all.turns.iter().map(|turn| turn.seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(all.session.turns, 2);

        let newest = session_window(
            conn,
            "parent",
            TurnWindow {
                limit: 1,
                before: None,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            newest.turns.iter().map(|turn| turn.seq).collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(newest.session.turns, 2, "the summary reports the total");

        let older = session_window(
            conn,
            "parent",
            TurnWindow {
                limit: 10,
                before: Some(2),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            older.turns.iter().map(|turn| turn.seq).collect::<Vec<_>>(),
            vec![1]
        );

        let empty = session_window(
            conn,
            "parent",
            TurnWindow {
                limit: 1,
                before: Some(1),
            },
        )
        .unwrap()
        .unwrap();
        assert!(empty.turns.is_empty());
    }

    #[test]
    fn the_events_view_pages_with_the_turn_window() {
        let (_dir, store) = seed();
        let conn = store.connection();
        let all = super::events(conn, "parent", TurnWindow::default())
            .unwrap()
            .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "turn_started");

        let older = super::events(
            conn,
            "parent",
            TurnWindow {
                limit: 0,
                before: Some(2),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(older.len(), 2, "the window stops before turn 2");

        let newest = super::events(
            conn,
            "parent",
            TurnWindow {
                limit: 1,
                before: None,
            },
        )
        .unwrap()
        .unwrap();
        assert!(newest.is_empty(), "turn 2 has no events in the seed");

        assert!(super::events(conn, "missing", TurnWindow::default())
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_failed_turn_exposes_its_error_text() {
        let (_dir, mut store) = seed();
        let snapshot = store.load("parent").unwrap().unwrap().snapshot;
        let events = vec![("turn_started".to_owned(), r#"{"input":"bad"}"#.to_owned())];
        store
            .save(
                "parent",
                &snapshot,
                None,
                Some(StoredTurn {
                    seq: 3,
                    input: "bad",
                    state: "Error",
                    answer: "",
                    answer_json: "{}",
                    usage_json: "{}",
                    duration_ms: 1,
                    error: Some("model failure (model_provider)"),
                    context: None,
                    events: &events,
                }),
            )
            .unwrap();

        let detail = super::turn(store.connection(), "parent", 3)
            .unwrap()
            .unwrap();
        assert_eq!(
            detail.turn.error.as_deref(),
            Some("model failure (model_provider)")
        );
        assert_eq!(detail.events.len(), 1);
        assert_eq!(detail.events[0].name, "turn_started");
    }

    #[test]
    fn the_memory_view_lists_fact_versions_and_task_revisions() {
        let (_dir, mut store) = seed();
        let stored = store.load("parent").unwrap().unwrap();
        let mut snapshot = stored.snapshot;
        let todos = stored.todos;
        snapshot.facts[0].statement = "project is seagull".to_owned();
        store
            .save("parent", &snapshot, todos.as_ref(), None)
            .unwrap();

        let view = memory(store.connection(), "parent").unwrap().unwrap();
        assert_eq!(view.facts.len(), 2);
        assert_eq!(view.facts[0].reason, "added");
        assert!(view.facts[0].active);
        assert_eq!(view.facts[1].reason, "superseded");
        assert_eq!(view.facts[1].statement, "project is seagull");
        assert_eq!(view.tasks.len(), 2);
        assert_eq!(view.tasks[0].items[0].status, "pending");
        assert_eq!(view.tasks[1].items[0].status, "completed");
        assert!(memory(store.connection(), "missing").unwrap().is_none());
    }

    #[test]
    fn the_session_view_matches_the_golden_file() {
        let (_dir, store) = seed();
        let view = session_window(store.connection(), "parent", TurnWindow::default())
            .unwrap()
            .unwrap();
        let mut value = serde_json::to_value(&view).unwrap();
        normalize_timestamps(&mut value);
        let rendered = serde_json::to_string_pretty(&value).unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/session-view.golden.json");
        let expected = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => {
                // First run writes the fixture; the file is reviewed and
                // committed, so later runs compare against a fixed shape.
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, &rendered).unwrap();
                return;
            }
        };
        assert_eq!(rendered.trim(), expected.trim());
    }

    fn normalize_timestamps(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, item) in map.iter_mut() {
                    if matches!(key.as_str(), "updated_at" | "created_at" | "changed_at") {
                        *item = serde_json::json!(0);
                    } else {
                        normalize_timestamps(item);
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    normalize_timestamps(item);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn a_branch_view_starts_at_the_fork_state() {
        let (_dir, store) = seed();
        let view = session_window(store.connection(), "child", TurnWindow::default())
            .unwrap()
            .unwrap();
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
        assert!(
            session_window(store.connection(), "missing", TurnWindow::default())
                .unwrap()
                .is_none()
        );
    }
}
