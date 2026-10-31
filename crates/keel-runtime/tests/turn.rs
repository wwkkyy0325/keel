mod common;

use std::sync::Arc;
use std::time::Duration;

use keel_core::{
    AcceptanceItem, ArgValue, CancelToken, CommitCtx, CommitDecision, ContextRecipe, ErrorCode,
    Event, MemoryPolicy, ModelError, ModelRole, ModelRoute, PermissionLevel, Policy,
    ReasoningLevel, SecretKind, ToolArgs, ToolSchema, TurnId, TurnState, VerificationMode,
    VerifySpec,
};
use keel_runtime::{
    DeferAllInteractionPort, EventLog, InteractionBroker, MockModel, MockStep, PermissionEngine,
    RuntimeConfig, TurnDriver, TurnDriverConfig,
};

struct RepromptPolicy {
    inner: RuntimeConfig,
}

impl Policy for RepromptPolicy {
    fn visible_tools(&self, all: &[ToolSchema]) -> Vec<ToolSchema> {
        self.inner.visible_tools(all)
    }

    fn context_recipe(&self) -> ContextRecipe {
        self.inner.context_recipe()
    }

    fn thinking_budget(&self) -> u32 {
        self.inner.thinking_budget()
    }

    fn verification_mode(&self) -> VerificationMode {
        self.inner.verification_mode()
    }

    fn memory_policy(&self) -> MemoryPolicy {
        self.inner.memory_policy()
    }

    fn model_route(&self, role: ModelRole) -> ModelRoute {
        self.inner.model_route(role)
    }

    fn before_commit(&self, _ctx: &CommitCtx<'_>) -> CommitDecision {
        CommitDecision::Reprompt
    }
}

fn tool_finished(events: &EventLog, turn_id: TurnId) -> Vec<(bool, Option<ErrorCode>)> {
    events
        .for_turn(turn_id)
        .into_iter()
        .filter_map(|record| match record.event {
            Event::ToolFinished { ok, error, .. } => Some((ok, error)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn echo_turn_reaches_done_and_replays_states() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["echo"], 4, PermissionLevel::WorkspaceAuto);

    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hi".to_owned()));
    let model = MockModel::new(vec![
        MockStep::ToolCall {
            name: "echo".to_owned(),
            args,
        },
        MockStep::Text("done".to_owned()),
    ]);

    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );
    let outcome = driver.run("hello", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Done);
    assert_eq!(outcome.text, "done");
    assert!(outcome.error.is_none());
    assert_eq!(
        common::states(&events, outcome.turn_id),
        vec![
            TurnState::Observe,
            TurnState::Solve,
            TurnState::Act,
            TurnState::Solve,
            TurnState::Verify,
            TurnState::Converge,
            TurnState::Done,
        ]
    );
    assert_eq!(tool_finished(&events, outcome.turn_id), vec![(true, None)]);
}

#[tokio::test]
async fn text_only_turn_reaches_done() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let model = MockModel::new(vec![MockStep::Text("hello".to_owned())]);

    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Done);
    assert_eq!(outcome.text, "hello");
}

#[tokio::test]
async fn model_error_ends_the_turn_in_error() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let model = MockModel::new(vec![MockStep::Error(ModelError::provider("boom"))]);

    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Error);
    assert_eq!(
        outcome.error.as_ref().map(|error| error.code()),
        Some(ErrorCode::ModelProvider)
    );
}

#[tokio::test]
async fn stream_cut_ends_the_turn_in_error() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let model = MockModel::new(vec![MockStep::CutAfter("partial".to_owned())]);

    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Error);
    assert_eq!(outcome.text, "partial");
    assert_eq!(
        outcome.error.as_ref().map(|error| error.code()),
        Some(ErrorCode::StreamCut)
    );
}

#[tokio::test]
async fn context_overflow_ends_the_turn_in_error() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let model = MockModel::new(vec![MockStep::Error(ModelError::overflow("too large"))]);

    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Error);
    assert_eq!(
        outcome.error.as_ref().map(|error| error.code()),
        Some(ErrorCode::ContextOverflow)
    );
}

#[tokio::test]
async fn thinking_only_exhausts_the_model_budget() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let model = MockModel::repeating(vec![MockStep::Thinking("thinking".to_owned())]);
    let mut limits = common::test_limits();
    limits.max_model_calls = 2;

    let driver = common::build_driver(model, config, events.clone(), dir.path(), limits);
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Done);
    let model_calls = events
        .for_turn(outcome.turn_id)
        .into_iter()
        .filter(|record| matches!(record.event, Event::ModelStarted { .. }))
        .count();
    assert_eq!(model_calls, 2);
}

#[tokio::test]
async fn turn_timeout_ends_the_turn_in_error() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let model = MockModel::new(vec![MockStep::Hang]);
    let mut limits = common::test_limits();
    limits.turn_timeout = Duration::from_millis(50);

    let driver = common::build_driver(model, config, events.clone(), dir.path(), limits);
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Error);
    assert_eq!(
        outcome.error.as_ref().map(|error| error.code()),
        Some(ErrorCode::Timeout)
    );
}

#[tokio::test]
async fn cancellation_before_the_turn_reaches_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let model = MockModel::new(vec![MockStep::Text("unused".to_owned())]);

    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    let outcome = driver.run("hi", cancel).await;

    assert_eq!(outcome.state, TurnState::Cancelled);
    assert!(events
        .for_turn(outcome.turn_id)
        .into_iter()
        .any(|record| matches!(record.event, Event::TurnCancelled { .. })));
}

#[tokio::test]
async fn cancellation_during_a_hanging_model_reaches_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let model = MockModel::new(vec![MockStep::Hang]);

    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );
    let cancel = CancelToken::new();
    let cancel_task = cancel.clone();
    let _cancel_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancel_task.cancel();
    });
    let outcome = driver.run("hi", cancel).await;

    assert_eq!(outcome.state, TurnState::Cancelled);
}

#[tokio::test]
async fn tool_error_does_not_abort_the_turn() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["read_file"], 4, PermissionLevel::WorkspaceAuto);
    let model = MockModel::new(vec![
        MockStep::ToolCall {
            name: "read_file".to_owned(),
            args: ToolArgs::new(),
        },
        MockStep::Text("recovered".to_owned()),
    ]);

    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Done);
    assert_eq!(outcome.text, "recovered");
    assert_eq!(
        tool_finished(&events, outcome.turn_id),
        vec![(false, Some(ErrorCode::InvalidArguments))]
    );
}

#[tokio::test]
async fn invisible_tool_is_rejected_and_the_turn_continues() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 4, PermissionLevel::WorkspaceAuto);
    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hi".to_owned()));
    let model = MockModel::new(vec![
        MockStep::ToolCall {
            name: "echo".to_owned(),
            args,
        },
        MockStep::Text("continued".to_owned()),
    ]);

    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Done);
    assert_eq!(outcome.text, "continued");
    assert_eq!(
        tool_finished(&events, outcome.turn_id),
        vec![(false, Some(ErrorCode::PermissionDenied))]
    );
}

#[tokio::test]
async fn tool_output_over_context_budget_ends_in_error() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let mut config = common::runtime_config(&["echo"], 4, PermissionLevel::WorkspaceAuto);
    config.context.budget_tokens = 20;

    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("x".repeat(200)));
    let model = MockModel::new(vec![
        MockStep::ToolCall {
            name: "echo".to_owned(),
            args,
        },
        MockStep::Text("unused".to_owned()),
    ]);

    let driver = common::build_driver(model, config, events, dir.path(), common::test_limits());
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Error);
    assert_eq!(
        outcome.error.as_ref().map(|error| error.code()),
        Some(ErrorCode::ContextOverflow)
    );
}

#[tokio::test]
async fn non_commit_decision_ends_in_error() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let permissions = Arc::new(PermissionEngine::new(dir.path(), config.permission_level).unwrap());
    let interactions = Arc::new(InteractionBroker::new(
        Arc::new(DeferAllInteractionPort),
        config.interaction,
        events.clone(),
    ));
    let policy: Arc<dyn Policy> = Arc::new(RepromptPolicy {
        inner: config.clone(),
    });

    let driver = TurnDriver::new(
        MockModel::new(vec![MockStep::Text("done".to_owned())]),
        common::standard_registry(dir.path()),
        common::standard_context(),
        policy,
        TurnDriverConfig {
            events,
            permissions,
            interactions,
            system_prompt: config.system_prompt.clone(),
            max_output_tokens: config.max_output_tokens,
            limits: common::test_limits(),
        },
    );
    let outcome = driver.run("hi", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Error);
    assert_eq!(
        outcome.error.as_ref().map(|error| error.code()),
        Some(ErrorCode::Unsupported)
    );
}

#[tokio::test]
async fn acceptance_pass_reaches_done_with_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let driver = common::build_driver(
        MockModel::new(vec![MockStep::Text("done".to_owned())]),
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    )
    .with_acceptance(
        vec![AcceptanceItem {
            id: "sum".to_owned(),
            description: "2 + 2".to_owned(),
            required: true,
            verify: VerifySpec::Compute {
                expression: "2 + 2".to_owned(),
                expected: "4".to_owned(),
            },
        }],
        0,
    );

    let outcome = driver.run("try", CancelToken::new()).await;

    assert!(outcome.is_done());
    assert!(outcome
        .verification
        .as_ref()
        .is_some_and(|report| report.passed()));
    assert!(events
        .for_turn(outcome.turn_id)
        .into_iter()
        .any(|record| matches!(
            record.event,
            Event::VerificationFinished { passed: true, .. }
        )));
}

#[tokio::test]
async fn acceptance_failure_reprompts_then_ends_in_error() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let driver = common::build_driver(
        MockModel::new(vec![
            MockStep::Text("first".to_owned()),
            MockStep::Text("second".to_owned()),
        ]),
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    )
    .with_acceptance(
        vec![AcceptanceItem {
            id: "wrong".to_owned(),
            description: "always fails".to_owned(),
            required: true,
            verify: VerifySpec::Compute {
                expression: "1 + 1".to_owned(),
                expected: "3".to_owned(),
            },
        }],
        1,
    );

    let outcome = driver.run("try", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Error);
    assert_eq!(
        outcome.error.as_ref().map(|error| error.code()),
        Some(ErrorCode::VerificationFailed)
    );
    assert!(outcome
        .verification
        .as_ref()
        .is_some_and(|report| !report.passed()));
    assert!(events
        .for_turn(outcome.turn_id)
        .into_iter()
        .any(|record| matches!(
            record.event,
            Event::ReasoningEscalated { ref reason, .. } if reason == "verification failed"
        )));
    assert_eq!(
        events
            .for_turn(outcome.turn_id)
            .into_iter()
            .filter(|record| matches!(record.event, Event::VerificationFinished { .. }))
            .count(),
        2
    );
}

#[tokio::test]
async fn acceptance_without_verification_cannot_end_done() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["echo"], 8, PermissionLevel::WorkspaceAuto);
    let mut limits = common::test_limits();
    limits.max_tool_calls = 0;
    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hi".to_owned()));
    let driver = common::build_driver(
        MockModel::new(vec![MockStep::ToolCall {
            name: "echo".to_owned(),
            args,
        }]),
        config,
        events,
        dir.path(),
        limits,
    )
    .with_acceptance(
        vec![AcceptanceItem {
            id: "sum".to_owned(),
            description: "2 + 2".to_owned(),
            required: true,
            verify: VerifySpec::Compute {
                expression: "2 + 2".to_owned(),
                expected: "4".to_owned(),
            },
        }],
        0,
    );

    let outcome = driver.run("try", CancelToken::new()).await;

    assert_eq!(outcome.state, TurnState::Error);
    assert_eq!(
        outcome.error.as_ref().map(|error| error.code()),
        Some(ErrorCode::VerificationFailed)
    );
    assert!(outcome.verification.is_none());
}

#[tokio::test]
async fn repeated_tool_failure_escalates_reasoning() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["read_file"], 8, PermissionLevel::WorkspaceAuto);
    let model = MockModel::new(vec![
        MockStep::ToolCall {
            name: "read_file".to_owned(),
            args: ToolArgs::new(),
        },
        MockStep::ToolCall {
            name: "read_file".to_owned(),
            args: ToolArgs::new(),
        },
        MockStep::Text("done".to_owned()),
    ]);
    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );

    let outcome = driver.run("try", CancelToken::new()).await;

    assert!(outcome.is_done());
    assert_eq!(outcome.reasoning_level, ReasoningLevel::High);
    let escalations: Vec<(String, String, String)> = events
        .for_turn(outcome.turn_id)
        .into_iter()
        .filter_map(|record| match record.event {
            Event::ReasoningEscalated {
                from, to, reason, ..
            } => Some((from, to, reason)),
            _ => None,
        })
        .collect();
    assert_eq!(
        escalations,
        vec![(
            "low".to_owned(),
            "high".to_owned(),
            "tool failures".to_owned()
        )]
    );
}

#[tokio::test]
async fn permission_denied_does_not_escalate_reasoning() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["write_file"], 8, PermissionLevel::WorkspaceAuto);
    let write_args = || {
        let mut args = ToolArgs::new();
        args.insert("path", ArgValue::String("../outside.txt".to_owned()));
        args.insert("content", ArgValue::String("x".to_owned()));
        args
    };
    let model = MockModel::new(vec![
        MockStep::ToolCall {
            name: "write_file".to_owned(),
            args: write_args(),
        },
        MockStep::ToolCall {
            name: "write_file".to_owned(),
            args: write_args(),
        },
        MockStep::Text("done".to_owned()),
    ]);
    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );

    let outcome = driver.run("try", CancelToken::new()).await;

    assert!(outcome.is_done());
    assert_eq!(outcome.reasoning_level, ReasoningLevel::Low);
    assert!(!events
        .for_turn(outcome.turn_id)
        .into_iter()
        .any(|record| matches!(record.event, Event::ReasoningEscalated { .. })));
}

#[tokio::test]
async fn repeated_empty_rounds_escalate_reasoning() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let model = MockModel::new(vec![
        MockStep::Thinking("hmm".to_owned()),
        MockStep::Thinking("still thinking".to_owned()),
        MockStep::Text("done".to_owned()),
    ]);
    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );

    let outcome = driver.run("try", CancelToken::new()).await;

    assert!(outcome.is_done());
    assert_eq!(outcome.reasoning_level, ReasoningLevel::High);
    assert!(events
        .for_turn(outcome.turn_id)
        .into_iter()
        .any(|record| matches!(
            record.event,
            Event::ReasoningEscalated { ref reason, .. } if reason == "no progress"
        )));
}

#[tokio::test]
async fn secret_like_input_is_reported_on_the_turn_and_not_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let driver = common::build_driver(
        MockModel::new(vec![MockStep::Text("done".to_owned())]),
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );

    let outcome = driver
        .run("my key sk-1234567890abcdefghij", CancelToken::new())
        .await;

    assert!(outcome.is_done());
    assert_eq!(outcome.secret_warning, Some(SecretKind::OpenAi));

    let records = events.for_turn(outcome.turn_id);
    assert!(records.iter().any(|record| matches!(
        &record.event,
        Event::SecretWarning { kind, .. } if kind.as_str() == "OpenAI API key"
    )));
    let input = records.iter().find_map(|record| match &record.event {
        Event::TurnStarted { input, .. } => Some(input),
        _ => None,
    });
    assert!(input.is_some_and(|input| !input.contains("sk-1234567890abcdefghij")));
}
