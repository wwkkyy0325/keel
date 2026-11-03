mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    AcceptanceItem, ArgValue, CancelToken, CommitCtx, CommitDecision, ContextRecipe, ErrorCode,
    Event, KernelError, MemoryPolicy, Message, MessageRole, ModelChunk, ModelError, ModelProvider,
    ModelRequest, ModelRole, ModelRoute, ModelStream, PermissionLevel, Policy, ReasoningLevel,
    SecretKind, TodoList, TodoStatus, TokenUsage, Tool, ToolArgs, ToolCallId, ToolCtx, ToolOutcome,
    ToolSchema, TurnId, TurnState, VerificationMode, VerifySpec,
};
use keel_runtime::{
    DeferAllInteractionPort, EventLog, InteractionBroker, InterjectionPush, MockModel, MockStep,
    PermissionEngine, RuntimeConfig, TurnDriver, TurnDriverConfig, TurnLimits,
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

fn event_names(events: &EventLog, turn_id: TurnId) -> Vec<&'static str> {
    events
        .for_turn(turn_id)
        .into_iter()
        .map(|record| record.event.name())
        .collect()
}

#[tokio::test]
async fn a_tool_turn_emits_the_host_contract_events_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["write_file"], 4, PermissionLevel::WorkspaceAuto);
    let mut args = ToolArgs::new();
    args.insert("path", ArgValue::String("note.txt".to_owned()));
    args.insert("content", ArgValue::String("hello".to_owned()));
    let model = MockModel::new(vec![
        MockStep::ToolCall {
            name: "write_file".to_owned(),
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
    let outcome = driver.run("write a note", CancelToken::new()).await;
    assert_eq!(outcome.state, TurnState::Done);

    let names = event_names(&events, outcome.turn_id);
    assert_eq!(names.first().copied(), Some("turn_started"), "{names:?}");
    assert_eq!(names.last().copied(), Some("turn_report"), "{names:?}");
    for required in [
        "model_started",
        "permission_decided",
        "tool_started",
        "tool_finished",
        "turn_finished",
    ] {
        assert!(names.contains(&required), "missing {required}: {names:?}");
    }
    let index = |name: &str| {
        names
            .iter()
            .position(|value| *value == name)
            .unwrap_or_else(|| panic!("missing {name}: {names:?}"))
    };
    assert!(index("model_started") < index("permission_decided"));
    assert!(index("permission_decided") < index("tool_started"));
    assert!(index("tool_started") < index("tool_finished"));
    assert!(index("tool_finished") < index("turn_finished"));
    assert!(index("turn_finished") < index("turn_report"));
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
    assert_eq!(outcome.model_calls, 1);
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
    assert!(outcome.answer.missing, "a turn with no text has no answer");
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
            context_window: config.context_window,
            limits: common::test_limits(),
            todos: None,
            background: None,
            options: keel_runtime::KernelOptions::default(),
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
    if let Some(KernelError::VerificationFailed { failures }) = &outcome.error {
        assert!(
            failures.iter().any(|failure| failure.contains("wrong")),
            "{failures:?}"
        );
    } else {
        panic!("expected VerificationFailed, got {:?}", outcome.error);
    }
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

type SeenRequests = Arc<Mutex<Vec<(usize, Vec<Arc<Message>>)>>>;

struct ToolRecordingModel {
    inner: MockModel,
    seen: SeenRequests,
}

#[async_trait]
impl ModelProvider for ToolRecordingModel {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        self.seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((request.tools.len(), request.messages.clone()));
        self.inner.stream(request, cancel).await
    }
}

fn two_call_limits() -> TurnLimits {
    TurnLimits {
        max_model_calls: 2,
        max_tool_calls: 8,
        tool_timeout: Duration::from_millis(200),
        turn_timeout: Duration::from_secs(2),
        max_turn_input_tokens: u64::MAX,
    }
}

#[tokio::test]
async fn the_last_model_call_is_reserved_for_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["echo"], 4, PermissionLevel::WorkspaceAuto);

    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hi".to_owned()));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let model = ToolRecordingModel {
        inner: MockModel::new(vec![
            MockStep::ToolCall {
                name: "echo".to_owned(),
                args,
            },
            MockStep::Text("final answer".to_owned()),
        ]),
        seen: seen.clone(),
    };
    let driver = common::build_driver(model, config, events.clone(), dir.path(), two_call_limits());

    let outcome = driver.run("hi", CancelToken::new()).await;
    assert!(outcome.is_done(), "{outcome:?}");
    assert_eq!(outcome.text, "final answer");

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].0, 1, "the first call may use tools");
    assert_eq!(seen[1].0, 0, "the final call must not receive tools");
    assert!(seen[1]
        .1
        .iter()
        .any(|message| message.content.starts_with("This is the last model call")));

    let forced = events
        .for_turn(outcome.turn_id)
        .iter()
        .filter(|record| matches!(record.event, Event::FinalCallForced { .. }))
        .count();
    assert_eq!(forced, 1);
}

#[tokio::test]
async fn tools_are_not_dispatched_on_the_final_call() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["echo"], 4, PermissionLevel::WorkspaceAuto);

    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hi".to_owned()));
    let model = MockModel::new(vec![
        MockStep::ToolCall {
            name: "echo".to_owned(),
            args: args.clone(),
        },
        MockStep::ToolCall {
            name: "echo".to_owned(),
            args,
        },
    ]);
    let driver = common::build_driver(model, config, events.clone(), dir.path(), two_call_limits());

    let outcome = driver.run("hi", CancelToken::new()).await;
    assert_eq!(outcome.state, TurnState::Error);

    let dispatched = events
        .for_turn(outcome.turn_id)
        .iter()
        .filter(|record| matches!(record.event, Event::ToolStarted { .. }))
        .count();
    assert_eq!(dispatched, 1, "only the first call may dispatch a tool");
}

#[tokio::test]
async fn narration_is_not_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["echo"], 4, PermissionLevel::WorkspaceAuto);

    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hi".to_owned()));
    let model = MockModel::new(vec![
        MockStep::TextAndToolCall {
            text: "let me check".to_owned(),
            name: "echo".to_owned(),
            args,
        },
        MockStep::Text("the result".to_owned()),
    ]);
    let driver = common::build_driver(model, config, events, dir.path(), common::test_limits());

    let outcome = driver.run("hi", CancelToken::new()).await;
    assert!(outcome.is_done());
    assert_eq!(outcome.answer.body, "the result");
    assert_eq!(outcome.answer.revision, 1);
    assert!(outcome.text.contains("let me check"));
}

#[tokio::test]
async fn written_files_are_recorded_in_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["write_file"], 4, PermissionLevel::WorkspaceAuto);

    let mut args = ToolArgs::new();
    args.insert("path", ArgValue::String("note.txt".to_owned()));
    args.insert("content", ArgValue::String("hello".to_owned()));
    let model = MockModel::new(vec![
        MockStep::TextAndToolCall {
            text: "writing the note".to_owned(),
            name: "write_file".to_owned(),
            args,
        },
        MockStep::Text("wrote note.txt".to_owned()),
    ]);
    let driver = common::build_driver(model, config, events, dir.path(), common::test_limits());

    let outcome = driver.run("write a note", CancelToken::new()).await;
    assert!(outcome.is_done());
    assert_eq!(outcome.answer.body, "wrote note.txt");
    assert_eq!(outcome.answer.changes.len(), 1);
    assert_eq!(outcome.answer.changes[0].path, "note.txt");
    assert!(dir.path().join("note.txt").exists());
}

#[tokio::test]
async fn a_large_tool_result_is_summarized_and_spilled() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let mut config = common::runtime_config(&["read_file"], 4, PermissionLevel::WorkspaceAuto);
    config.context.stages = vec![
        "system".to_owned(),
        "history".to_owned(),
        "latest".to_owned(),
    ];
    // The current turn keeps the full result, so it needs room for it.
    config.context.budget_tokens = 60_000;

    let content = "x".repeat(200_000);
    std::fs::write(dir.path().join("big.txt"), &content).unwrap();
    let mut args = ToolArgs::new();
    args.insert("path", ArgValue::String("big.txt".to_owned()));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let model = ToolRecordingModel {
        inner: MockModel::new(vec![
            MockStep::ToolCall {
                name: "read_file".to_owned(),
                args,
            },
            MockStep::Text("done".to_owned()),
            MockStep::Text("again".to_owned()),
        ]),
        seen: seen.clone(),
    };
    let driver = common::build_driver(model, config, events, dir.path(), common::test_limits());

    assert!(driver.run("first", CancelToken::new()).await.is_done());
    // A follow-up on the same topic keeps the segment open, so the raw tool
    // summary is still part of history.
    assert!(driver
        .run("first again", CancelToken::new())
        .await
        .is_done());

    {
        let seen = seen.lock().unwrap();
        let last = &seen[2].1;
        let assistant = last
            .iter()
            .find(|message| {
                message.role == MessageRole::Assistant && message.content.contains("[tools]")
            })
            .unwrap_or_else(|| panic!("no tool block in history: {last:?}"));
        assert!(assistant.content.starts_with("done"));
        assert!(assistant.content.contains("truncated 200000 chars"));
        assert!(assistant.content.contains("turn-1-call-0.txt"));
    }

    let spill = dir.path().join(".keel/outputs/turn-1-call-0.txt");
    let spilled = std::fs::read_to_string(&spill).unwrap();
    assert_eq!(spilled.chars().count(), 200_000);

    let tool = keel_runtime::ReadFileTool::new();
    let mut read_args = ToolArgs::new();
    read_args.insert("path", ArgValue::String(spill.display().to_string()));
    let ctx = ToolCtx {
        turn_id: TurnId::new(99),
        call_id: ToolCallId::next(),
        workspace_root: dir.path().to_path_buf(),
        cancel: CancelToken::new(),
    };
    match tool.call(read_args, &ctx).await.outcome {
        ToolOutcome::Success { content } => assert_eq!(content.chars().count(), 200_000),
        other => panic!("read_file could not read the spill: {other:?}"),
    }
}

struct OverheadModel;

#[async_trait]
impl ModelProvider for OverheadModel {
    async fn stream(
        &self,
        request: ModelRequest,
        _cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        let estimated = request.messages.iter().fold(0u32, |total, message| {
            total.saturating_add(message.estimated_tokens())
        });
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let _ = tx.send(Ok(ModelChunk::TextDelta("done".to_owned()))).await;
        let _ = tx
            .send(Ok(ModelChunk::Finished {
                usage: TokenUsage {
                    input_tokens: estimated.saturating_add(500),
                    output_tokens: 5,
                    cached_input_tokens: 0,
                },
            }))
            .await;
        Ok(ModelStream::new(rx))
    }
}

#[tokio::test]
async fn reported_usage_calibrates_the_next_budget() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let driver = common::build_driver(
        OverheadModel,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    );

    assert!(driver.run("one", CancelToken::new()).await.is_done());
    let second = driver.run("two", CancelToken::new()).await;
    assert!(second.is_done());

    let budgets: Vec<u32> = events
        .for_turn(second.turn_id)
        .iter()
        .filter_map(|record| match record.event {
            Event::ContextBuilt { budget, .. } => Some(budget),
            _ => None,
        })
        .collect();
    assert_eq!(budgets.last(), Some(&(4096 - 500)));
}

struct FlakySummarizer {
    inputs: Arc<Mutex<Vec<String>>>,
    calls: Arc<Mutex<u32>>,
}

#[async_trait]
impl keel_runtime::Summarizer for FlakySummarizer {
    async fn summarize(
        &self,
        input: &str,
        _max_tokens: u32,
        _cancel: &CancelToken,
    ) -> Result<keel_runtime::Summarized, ModelError> {
        let mut calls = self
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let call = *calls;
        *calls = calls.saturating_add(1);
        drop(calls);
        self.inputs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(input.to_owned());
        if call == 0 {
            return Err(ModelError::internal("first summary fails"));
        }
        Ok(keel_runtime::Summarized {
            text: "summary".to_owned(),
            usage: TokenUsage::default(),
        })
    }
}

#[tokio::test]
async fn folded_work_is_included_in_a_later_summary() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let mut config = common::runtime_config(&["write_file"], 4, PermissionLevel::WorkspaceAuto);
    config.context.stages = vec![
        "system".to_owned(),
        "memory".to_owned(),
        "history".to_owned(),
        "latest".to_owned(),
    ];
    config.context.budget_tokens = 8192;

    let mut write_args = ToolArgs::new();
    write_args.insert("path", ArgValue::String("a.txt".to_owned()));
    write_args.insert("content", ArgValue::String("hi".to_owned()));
    let model = MockModel::new(vec![
        MockStep::TextAndToolCall {
            text: "writing".to_owned(),
            name: "write_file".to_owned(),
            args: write_args,
        },
        MockStep::Text("done".to_owned()),
        MockStep::Text("x".repeat(4000)),
        MockStep::Text("ok".to_owned()),
    ]);
    let inputs = Arc::new(Mutex::new(Vec::new()));
    let summarizer = FlakySummarizer {
        inputs: inputs.clone(),
        calls: Arc::new(Mutex::new(0)),
    };
    let driver = common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
    )
    .with_summarizer(Arc::new(summarizer));

    assert!(driver
        .run("write a note", CancelToken::new())
        .await
        .is_done());
    assert!(driver.run("alpha beta", CancelToken::new()).await.is_done());
    let third = driver.run("gamma delta", CancelToken::new()).await;
    assert!(third.is_done());

    let inputs = inputs.lock().unwrap();
    assert!(inputs.len() >= 2, "no catch-up summary was made");
    let last = inputs.last().unwrap();
    assert!(last.contains("[previous summary]"), "{last}");
    assert!(last.contains("[segment 0]"), "{last}");
}

#[tokio::test]
async fn the_turn_start_context_is_captured_when_requested() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&[], 0, PermissionLevel::WorkspaceAuto);
    let permissions = Arc::new(PermissionEngine::new(dir.path(), config.permission_level).unwrap());
    let interactions = Arc::new(InteractionBroker::new(
        Arc::new(DeferAllInteractionPort),
        config.interaction,
        events.clone(),
    ));
    let policy: Arc<dyn Policy> = Arc::new(config.clone());
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
            context_window: config.context_window,
            limits: common::test_limits(),
            todos: None,
            background: None,
            options: keel_runtime::KernelOptions {
                capture_context: true,
                ..Default::default()
            },
        },
    );

    let outcome = driver.run("hi", CancelToken::new()).await;

    let snapshot = outcome.context.expect("context was not captured");
    assert!(snapshot.estimate > 0);
    assert!(snapshot
        .messages
        .iter()
        .any(|message| message.role == "user" && message.content == "hi"));
    assert!(snapshot
        .messages
        .iter()
        .any(|message| message.role == "system"));
}

struct DelayedRecordingModel {
    inner: MockModel,
    seen: SeenRequests,
    delay: Duration,
}

#[async_trait]
impl ModelProvider for DelayedRecordingModel {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        self.seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((request.tools.len(), request.messages.clone()));
        tokio::time::sleep(self.delay).await;
        self.inner.stream(request, cancel).await
    }
}

#[tokio::test]
async fn an_interjection_waits_for_the_next_safe_point() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["echo"], 4, PermissionLevel::WorkspaceAuto);
    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hi".to_owned()));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let model = DelayedRecordingModel {
        inner: MockModel::new(vec![
            MockStep::ToolCall {
                name: "echo".to_owned(),
                args,
            },
            MockStep::Text("done".to_owned()),
        ]),
        seen: seen.clone(),
        delay: Duration::from_millis(50),
    };
    let driver = Arc::new(common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        two_call_limits(),
    ));
    let pusher = driver.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(matches!(
            pusher.interject("stay in the workspace"),
            InterjectionPush::Queued(_)
        ));
    });

    let outcome = driver.run("start", CancelToken::new()).await;
    assert!(outcome.is_done(), "{outcome:?}");

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(
        !seen[0]
            .1
            .iter()
            .any(|message| message.content.contains("[user interjection]")),
        "the message arrived after the first request was built"
    );
    assert!(
        seen[1].1.iter().any(|message| message
            .content
            .contains("[user interjection] stay in the workspace")),
        "the second request must see the interjection"
    );
    assert!(events
        .for_turn(outcome.turn_id)
        .iter()
        .any(|record| matches!(
            &record.event,
            Event::InterjectionDelivered { text, .. } if text == "stay in the workspace"
        )));
}

#[tokio::test]
async fn a_late_interjection_is_reported_instead_of_dropped_silently() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["echo"], 4, PermissionLevel::WorkspaceAuto);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let model = DelayedRecordingModel {
        inner: MockModel::new(vec![MockStep::Text("answer".to_owned())]),
        seen: seen.clone(),
        delay: Duration::from_millis(50),
    };
    let limits = TurnLimits {
        max_model_calls: 1,
        max_tool_calls: 8,
        tool_timeout: Duration::from_millis(200),
        turn_timeout: Duration::from_secs(2),
        max_turn_input_tokens: u64::MAX,
    };
    let driver = Arc::new(common::build_driver(
        model,
        config,
        events.clone(),
        dir.path(),
        limits,
    ));
    let pusher = driver.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        pusher.interject("too late");
    });

    let outcome = driver.run("finish", CancelToken::new()).await;
    assert!(outcome.is_done(), "{outcome:?}");
    assert_eq!(outcome.late_interjections, vec!["too late".to_owned()]);
    assert!(events
        .for_turn(outcome.turn_id)
        .iter()
        .any(|record| matches!(
            &record.event,
            Event::InterjectionDropped { text, reason, .. }
                if text == "too late" && reason.contains("before a safe point")
        )));
}

/// Clears the shared task list on its second call, so the driver can finish
/// the turn instead of continuing forever.
struct CompletingModel {
    inner: MockModel,
    todos: Arc<Mutex<TodoList>>,
    calls: Arc<Mutex<u32>>,
}

#[async_trait]
impl ModelProvider for CompletingModel {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        let call = {
            let mut calls = self
                .calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *calls += 1;
            *calls
        };
        if call > 1 {
            let mut list = self
                .todos
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for item in &mut list.items {
                item.status = TodoStatus::Completed;
            }
        }
        self.inner.stream(request, cancel).await
    }
}

#[tokio::test]
async fn a_turn_continues_while_the_task_list_has_unfinished_items() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["echo"], 4, PermissionLevel::WorkspaceAuto);
    let todos = Arc::new(Mutex::new(TodoList::new()));
    {
        let mut list = todos
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        list.add_model("inspect the workspace").unwrap();
        list.add_model("run the tests").unwrap();
    }
    let model = CompletingModel {
        inner: MockModel::new(vec![
            MockStep::Text("I will start with the first item.".to_owned()),
            MockStep::Text("all items are done".to_owned()),
        ]),
        todos: todos.clone(),
        calls: Arc::new(Mutex::new(0)),
    };
    let driver = common::build_driver_with_todos(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
        todos,
    );

    let outcome = driver.run("do both items", CancelToken::new()).await;

    assert!(outcome.is_done(), "{outcome:?}");
    assert_eq!(outcome.answer.body, "all items are done");
    let continued: Vec<_> = events
        .for_turn(outcome.turn_id)
        .into_iter()
        .filter(|record| matches!(record.event, Event::TurnContinued { .. }))
        .collect();
    assert_eq!(continued.len(), 1, "the driver continues exactly once");
    assert!(matches!(
        continued[0].event,
        Event::TurnContinued { remaining: 2, .. }
    ));
}

#[tokio::test]
async fn the_turn_budget_forces_the_final_call_without_telling_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["echo"], 4, PermissionLevel::WorkspaceAuto);

    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hi".to_owned()));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let model = ToolRecordingModel {
        inner: MockModel::new(vec![
            MockStep::ToolCall {
                name: "echo".to_owned(),
                args,
            },
            MockStep::Text("final answer".to_owned()),
        ]),
        seen: seen.clone(),
    };
    let limits = TurnLimits {
        max_model_calls: 10,
        max_tool_calls: 8,
        tool_timeout: Duration::from_millis(200),
        turn_timeout: Duration::from_secs(5),
        // One token is more than the first call can avoid spending, so the
        // second call is the answer.
        max_turn_input_tokens: 1,
    };
    let driver = common::build_driver(model, config, events.clone(), dir.path(), limits);

    let outcome = driver.run("do it", CancelToken::new()).await;

    assert!(outcome.is_done(), "{outcome:?}");
    assert_eq!(outcome.answer.body, "final answer");
    assert!(events
        .for_turn(outcome.turn_id)
        .iter()
        .any(|record| matches!(record.event, Event::TurnBudgetExhausted { .. })));

    let requests = seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].0, 0, "the budget call carries no tools");
    assert!(!requests[1]
        .1
        .iter()
        .any(|message| message.content.to_lowercase().contains("token")));
}

/// Delivers one finished background command on every drain.
struct StaticBackground;

impl keel_runtime::BackgroundTasks for StaticBackground {
    fn start(
        &self,
        _spec: &keel_runtime::command::CommandSpec<'_>,
    ) -> Result<String, keel_core::ToolError> {
        Err(keel_core::ToolError::invalid_arguments("not used"))
    }

    fn drain(&self) -> Vec<keel_runtime::BackgroundResult> {
        vec![keel_runtime::BackgroundResult {
            id: "bg-9".to_owned(),
            program: "cargo".to_owned(),
            exit_code: Some(0),
            duration_ms: 1200,
            output_path: std::path::PathBuf::from("bg-9.log"),
            tail: "all tests passed".to_owned(),
        }]
    }
}

#[tokio::test]
async fn a_background_result_enters_before_the_next_model_call() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["echo"], 4, PermissionLevel::WorkspaceAuto);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let model = ToolRecordingModel {
        inner: MockModel::new(vec![MockStep::Text("done".to_owned())]),
        seen: seen.clone(),
    };
    let driver = common::build_driver_with_background(
        model,
        config,
        events.clone(),
        dir.path(),
        common::test_limits(),
        Arc::new(StaticBackground),
    );

    let outcome = driver.run("go", CancelToken::new()).await;

    assert!(outcome.is_done(), "{outcome:?}");
    let requests = seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(
        requests[0]
            .1
            .iter()
            .any(|message| message.content.contains("[background bg-9 finished]")),
        "{:?}",
        requests[0]
            .1
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
    );
    assert!(events
        .for_turn(outcome.turn_id)
        .iter()
        .any(|record| matches!(record.event, Event::BackgroundResult { .. })));
}

#[tokio::test]
async fn delegated_usage_counts_toward_the_turn() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = common::runtime_config(&["usage_tool"], 4, PermissionLevel::WorkspaceAuto);
    let model = MockModel::new(vec![
        MockStep::ToolCall {
            name: "usage_tool".to_owned(),
            args: ToolArgs::new(),
        },
        MockStep::Text("done".to_owned()),
    ]);
    let driver = common::build_driver(model, config, events, dir.path(), common::test_limits());

    let outcome = driver.run("go", CancelToken::new()).await;

    assert!(outcome.is_done(), "{outcome:?}");
    assert!(
        outcome.usage.input_tokens >= 500,
        "delegated input tokens are missing: {:?}",
        outcome.usage
    );
    assert!(
        outcome.usage.output_tokens >= 40,
        "delegated output tokens are missing: {:?}",
        outcome.usage
    );
}
