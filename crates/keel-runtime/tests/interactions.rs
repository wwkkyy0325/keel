mod common;

use std::sync::Arc;
use std::time::Duration;

use keel_core::{
    CancelToken, ErrorCode, Event, InteractionKind, InteractionPolicy, InteractionRequest,
    InteractionResponse, OnTimeout, TurnId, TurnState,
};
use keel_runtime::{EventLog, MockModel, MockStep};

fn interaction() -> MockStep {
    MockStep::Interaction(InteractionRequest::new(
        TurnId::new(1),
        InteractionKind::Clarify,
        "Which value should I use?",
    ))
}

#[tokio::test]
async fn question_resumes_the_turn_with_the_user_answer() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, keel_core::PermissionLevel::WorkspaceAuto);
    let port = Arc::new(common::ScriptedInteractionPort::new(vec![
        InteractionResponse::Freeform {
            text: "42".to_owned(),
        },
    ]));
    let model = MockModel::new(vec![interaction(), MockStep::Text("thanks".to_owned())]);

    let driver = common::build_driver_with_port(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
        port,
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Done);
    assert_eq!(outcome.text, "thanks");
    assert!(common::states(&events, outcome.turn_id).contains(&TurnState::WaitInteraction));
    assert_eq!(
        events
            .for_turn(outcome.turn_id)
            .into_iter()
            .filter(|record| matches!(record.event, Event::InteractionRequested { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .for_turn(outcome.turn_id)
            .into_iter()
            .filter(|record| matches!(record.event, Event::InteractionResolved { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn rejected_interaction_returns_to_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, keel_core::PermissionLevel::WorkspaceAuto);
    let port = Arc::new(common::ScriptedInteractionPort::new(vec![
        InteractionResponse::Rejected {
            reason: "not now".to_owned(),
        },
    ]));
    let model = MockModel::new(vec![interaction(), MockStep::Text("revised".to_owned())]);

    let driver = common::build_driver_with_port(
        model,
        config,
        events,
        dir.path(),
        common::test_limits(),
        port,
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Done);
    assert_eq!(outcome.text, "revised");
}

#[tokio::test]
async fn deferred_interaction_is_reported_and_the_turn_continues() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, keel_core::PermissionLevel::WorkspaceAuto);
    let port = Arc::new(common::ScriptedInteractionPort::new(vec![
        InteractionResponse::Deferred,
    ]));
    let model = MockModel::new(vec![interaction(), MockStep::Text("continued".to_owned())]);

    let driver = common::build_driver_with_port(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
        port,
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Done);
    assert_eq!(outcome.text, "continued");
    assert_eq!(outcome.deferred_interactions.len(), 1);
    assert!(events
        .for_turn(outcome.turn_id)
        .into_iter()
        .any(|record| { matches!(record.event, Event::TurnReport { deferred: 1, .. }) }));
}

#[tokio::test]
async fn interaction_timeout_with_fail_policy_ends_in_error() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let mut config = common::runtime_config(&[], 0, keel_core::PermissionLevel::WorkspaceAuto);
    config.interaction = InteractionPolicy {
        timeout: Duration::from_millis(20),
        on_timeout: OnTimeout::Fail,
        max_per_turn: 4,
    };
    let model = MockModel::new(vec![interaction(), MockStep::Text("unused".to_owned())]);

    let driver = common::build_driver_with_port(
        model,
        config,
        events,
        dir.path(),
        common::test_limits(),
        Arc::new(common::HangingInteractionPort),
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Error);
    assert_eq!(
        outcome.error.as_ref().map(|error| error.code()),
        Some(ErrorCode::Timeout)
    );
}

#[tokio::test]
async fn interaction_timeout_with_defer_policy_continues() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let mut config = common::runtime_config(&[], 0, keel_core::PermissionLevel::WorkspaceAuto);
    config.interaction = InteractionPolicy {
        timeout: Duration::from_millis(20),
        on_timeout: OnTimeout::DeferAndContinue,
        max_per_turn: 4,
    };
    let model = MockModel::new(vec![interaction(), MockStep::Text("after".to_owned())]);

    let driver = common::build_driver_with_port(
        model,
        config,
        events,
        dir.path(),
        common::test_limits(),
        Arc::new(common::HangingInteractionPort),
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Done);
    assert_eq!(outcome.text, "after");
    assert_eq!(outcome.deferred_interactions.len(), 1);
}

#[tokio::test]
async fn cancellation_while_waiting_for_an_interaction_is_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, keel_core::PermissionLevel::WorkspaceAuto);
    let model = MockModel::new(vec![interaction()]);

    let driver = common::build_driver_with_port(
        model,
        config,
        events,
        dir.path(),
        common::test_limits(),
        Arc::new(common::HangingInteractionPort),
    );
    let cancel = CancelToken::new();
    let task_cancel = cancel.clone();
    let _cancel_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        task_cancel.cancel();
    });
    let outcome = driver.run("hi", cancel).await;

    assert_eq!(outcome.state, TurnState::Cancelled);
}

#[tokio::test]
async fn interaction_budget_exceeded_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let mut config = common::runtime_config(&[], 0, keel_core::PermissionLevel::WorkspaceAuto);
    config.interaction.max_per_turn = 1;
    let port = Arc::new(common::ScriptedInteractionPort::new(vec![
        InteractionResponse::Deferred,
        InteractionResponse::Deferred,
    ]));
    let model = MockModel::new(vec![
        interaction(),
        interaction(),
        MockStep::Text("unused".to_owned()),
    ]);

    let driver = common::build_driver_with_port(
        model,
        config,
        events,
        dir.path(),
        common::test_limits(),
        port,
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Error);
    assert_eq!(
        outcome.error.as_ref().map(|error| error.code()),
        Some(ErrorCode::InvalidConfig)
    );
}
