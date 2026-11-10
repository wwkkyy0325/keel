//! Kernel performance gates from `docs/spec/04-evals.md`.
//!
//! These are ignored by default because timing on a debug build is not
//! meaningful and CI machines vary. Run them in release, locally:
//!
//! ```bash
//! cargo test --release -p keel-runtime --test performance -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use keel_core::{
    ArgValue, CancelToken, ContextRecipe, ContextStage, Event, Message, PermissionLevel, Session,
    TodoList, ToolArgs, ToolCallRequest, TurnEvent, TurnRecord, TurnState,
};
use keel_runtime::{
    EventLog, HistoryStage, MockModel, MockStep, PermissionEngine, SystemStage, ToolDispatcher,
    TurnDriver,
};

fn percentile(mut values: Vec<Duration>, fraction: f64) -> Duration {
    values.sort();
    let index = ((values.len() as f64 - 1.0) * fraction).round() as usize;
    values[index]
}

fn report(label: &str, values: &[Duration]) {
    let p50 = percentile(values.to_vec(), 0.50);
    let p99 = percentile(values.to_vec(), 0.99);
    println!(
        "{label}: p50={:.3} ms p99={:.3} ms n={}",
        p50.as_secs_f64() * 1000.0,
        p99.as_secs_f64() * 1000.0,
        values.len()
    );
}

#[tokio::test]
#[ignore = "timing gate; run in release"]
async fn turn_overhead_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let model = MockModel::repeating(vec![MockStep::Text("ok".to_owned())]);
    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );

    for _ in 0..10 {
        assert!(driver.run("warm up", CancelToken::new()).await.is_done());
    }
    let mut durations = Vec::with_capacity(200);
    for _ in 0..200 {
        let started = Instant::now();
        let outcome = driver.run("hello", CancelToken::new()).await;
        durations.push(started.elapsed());
        assert!(outcome.is_done(), "turn failed: {:?}", outcome.error);
    }
    report("turn overhead", &durations);
    let p50 = percentile(durations.clone(), 0.50);
    let p99 = percentile(durations, 0.99);
    assert!(p50 < Duration::from_millis(5), "p50 {p50:?}");
    assert!(p99 < Duration::from_millis(20), "p99 {p99:?}");
}

#[tokio::test]
#[ignore = "timing gate; run in release"]
async fn context_assembly_with_ten_thousand_messages() {
    let history: Vec<Arc<Message>> = (0..10_000)
        .map(|index| {
            Arc::new(if index % 2 == 0 {
                Message::user(format!("message {index} alpha beta"))
            } else {
                Message::assistant(format!("reply {index} gamma"))
            })
        })
        .collect();
    let stages: Vec<Arc<dyn ContextStage>> =
        vec![Arc::new(SystemStage::new()), Arc::new(HistoryStage::new())];
    let builder = keel_runtime::ContextBuilder::new(stages);
    let recipe = ContextRecipe {
        stages: vec!["system".to_owned(), "history".to_owned()],
        budget_tokens: 1_000_000,
        max_budget_tokens: 1_000_000,
        compaction: Vec::new(),
    };
    let todos = TodoList::default();
    let facts = Vec::new();
    let mut durations = Vec::new();
    for _ in 0..5 {
        let ctx = keel_core::BuildCtx {
            turn_id: keel_core::TurnId::new(1),
            input: "summarize",
            history: &history,
            records: &[],
            summary: None,
            recalled: &[],
            todos: Some(&todos),
            facts: &facts,
            system_prompt: "system",
            budget_tokens: 1_000_000,
            detail_share: 0.5,
            events: None,
        };
        let started = Instant::now();
        builder
            .build(&ctx, &recipe, &CancelToken::new())
            .await
            .unwrap();
        durations.push(started.elapsed());
    }
    report("context assembly (10k messages)", &durations);
    assert!(
        percentile(durations, 0.50) < Duration::from_millis(20),
        "context assembly exceeded 20 ms"
    );
}

#[tokio::test]
#[ignore = "timing gate; run in release"]
async fn tool_dispatch_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    let registry = common::standard_registry(dir.path());
    let events = Arc::new(EventLog::new());
    let permission =
        Arc::new(PermissionEngine::new(dir.path(), PermissionLevel::WorkspaceAuto).unwrap());
    let dispatcher = ToolDispatcher::new(registry, events, permission, Duration::from_secs(1));
    let visible: BTreeSet<String> = ["echo".to_owned()].into_iter().collect();
    let mut durations = Vec::new();
    for turn in 0..100 {
        let mut args = ToolArgs::new();
        args.insert("text", ArgValue::String("hi".to_owned()));
        let call = ToolCallRequest {
            id: keel_core::ToolCallId::new(turn + 1),
            name: "echo".to_owned(),
            args,
        };
        let started = Instant::now();
        let results = dispatcher
            .dispatch(
                keel_core::TurnId::new(turn + 1),
                vec![call],
                &visible,
                &CancelToken::new(),
            )
            .await;
        durations.push(started.elapsed());
        assert!(results[0].is_success());
    }
    report("tool dispatch", &durations);
    assert!(
        percentile(durations, 0.50) < Duration::from_millis(1),
        "tool dispatch exceeded 1 ms"
    );
}

#[test]
#[ignore = "timing gate; run in release"]
fn memory_recall_within_budget() {
    let mut session = Session::new();
    for index in 0..200 {
        let input = format!("subject{index} only");
        let artifacts = vec![format!("file{index}.rs")];
        session.begin_turn(&input);
        session.record_turn(TurnRecord {
            input: &input,
            text: "noted",
            tools: &[],
            artifacts: &artifacts,
            pending: &[],
            verified: true,
        });
    }
    let mut durations = Vec::new();
    for _ in 0..100 {
        let started = Instant::now();
        let recalled = session.recall("subject42 only");
        durations.push(started.elapsed());
        assert!(!recalled.is_empty());
    }
    report("memory recall", &durations);
    assert!(
        percentile(durations, 0.50) < Duration::from_millis(50),
        "memory recall exceeded 50 ms"
    );
}

#[test]
#[ignore = "timing gate; run in release"]
fn event_enqueue_within_budget() {
    let log = EventLog::new();
    let mut durations = Vec::with_capacity(10_000);
    for index in 0..10_000 {
        let started = Instant::now();
        log.append(Event::StateChanged {
            turn_id: keel_core::TurnId::new(index),
            from: TurnState::Init,
            to: TurnState::Observe,
            event: TurnEvent::Ready,
        });
        durations.push(started.elapsed());
    }
    report("event enqueue", &durations);
    assert!(
        percentile(durations, 0.50) < Duration::from_millis(1),
        "event enqueue exceeded 1 ms"
    );
}

#[tokio::test]
#[ignore = "stress gate; run in release"]
async fn five_hundred_turns_stay_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let model = MockModel::repeating(vec![MockStep::Text("ok".to_owned())]);
    let driver: TurnDriver<MockModel> = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );
    let started = Instant::now();
    for index in 0..500 {
        let outcome = driver
            .run(&format!("turn {index}"), CancelToken::new())
            .await;
        assert!(
            outcome.is_done(),
            "turn {index} failed: {:?}",
            outcome.error
        );
    }
    let elapsed = started.elapsed();
    println!("500 turns: {:.1} ms", elapsed.as_secs_f64() * 1000.0);
    assert!(
        elapsed < Duration::from_secs(120),
        "500 turns took {elapsed:?}"
    );

    let last = events
        .snapshot()
        .into_iter()
        .rev()
        .find_map(|record| match record.event {
            Event::ContextBuilt { tokens, budget, .. } => Some((tokens, budget)),
            _ => None,
        })
        .expect("a context build event");
    assert!(last.0 <= last.1, "context exceeded budget: {last:?}");
}
