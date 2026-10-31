mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use keel_core::{
    CancelToken, Event, Message, MessageRole, ModelError, ModelProvider, ModelRequest, ModelStream,
    PermissionLevel, Policy, TokenUsage,
};
use keel_runtime::{
    validate_config, ContextBuilder, DeferAllInteractionPort, EventLog, HistoryStage,
    InteractionBroker, LatestStage, MemoryStage, MockModel, MockStep, PermissionEngine, Summarized,
    Summarizer, SystemStage, TurnDriver, TurnDriverConfig,
};

/// Records the messages of every model request and replies with fixed text.
struct RecordingModel {
    requests: Arc<Mutex<Vec<Vec<Arc<Message>>>>>,
    reply: String,
}

#[async_trait]
impl ModelProvider for RecordingModel {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        self.requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request.messages.clone());
        MockModel::new(vec![MockStep::Text(self.reply.clone())])
            .stream(request, cancel)
            .await
    }
}

struct Harness {
    requests: Arc<Mutex<Vec<Vec<Arc<Message>>>>>,
    events: Arc<EventLog>,
    driver: TurnDriver<RecordingModel>,
    _dir: tempfile::TempDir,
}

fn harness(budget_tokens: u32) -> Harness {
    harness_with(budget_tokens, "ack", None)
}

fn harness_with(
    budget_tokens: u32,
    reply: &str,
    summarizer: Option<Arc<dyn Summarizer>>,
) -> Harness {
    harness_options(
        budget_tokens,
        reply,
        summarizer,
        keel_runtime::KernelOptions::default(),
    )
}

fn harness_options(
    budget_tokens: u32,
    reply: &str,
    summarizer: Option<Arc<dyn Summarizer>>,
    options: keel_runtime::KernelOptions,
) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let mut config = common::runtime_config(&["echo"], 4, PermissionLevel::WorkspaceAuto);
    config.context.stages = vec![
        "system".to_owned(),
        "memory".to_owned(),
        "history".to_owned(),
        "latest".to_owned(),
    ];
    config.context.budget_tokens = budget_tokens;

    let tools = common::standard_registry(dir.path());
    let context = ContextBuilder::new(vec![
        Arc::new(SystemStage::new()),
        Arc::new(MemoryStage::new()),
        Arc::new(HistoryStage::new()),
        Arc::new(LatestStage::new()),
    ]);
    validate_config(&config, &tools, &context).unwrap();

    let permissions = Arc::new(PermissionEngine::new(dir.path(), config.permission_level).unwrap());
    let interactions = Arc::new(InteractionBroker::new(
        Arc::new(DeferAllInteractionPort),
        config.interaction,
        events.clone(),
    ));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let model = RecordingModel {
        requests: requests.clone(),
        reply: reply.to_owned(),
    };
    let policy: Arc<dyn Policy> = Arc::new(config.clone());
    let driver = TurnDriver::new(
        model,
        tools,
        context,
        policy,
        TurnDriverConfig {
            events: events.clone(),
            permissions,
            interactions,
            system_prompt: config.system_prompt.clone(),
            max_output_tokens: config.max_output_tokens,
            context_window: config.context_window,
            limits: common::test_limits(),
            todos: None,
            options,
        },
    );

    let driver = match summarizer {
        Some(summarizer) => driver.with_summarizer(summarizer),
        None => driver,
    };

    Harness {
        requests,
        events,
        driver,
        _dir: dir,
    }
}

fn role_name(role: MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}

fn contents(messages: &[Arc<Message>]) -> Vec<String> {
    messages
        .iter()
        .map(|message| format!("{}:{}", role_name(message.role), message.content))
        .collect()
}

#[tokio::test]
async fn a_second_turn_sees_the_first_turn() {
    let harness = harness(4096);

    let first = harness.driver.run("first task", CancelToken::new()).await;
    assert!(first.is_done());
    let second = harness.driver.run("second task", CancelToken::new()).await;
    assert!(second.is_done());

    let requests = harness.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        contents(&requests[0]),
        vec!["system:You are a test agent.", "user:first task"]
    );
    assert_eq!(
        contents(&requests[1]),
        vec![
            "system:You are a test agent.",
            "user:first task",
            "assistant:ack",
            "user:second task",
        ]
    );
}

#[tokio::test]
async fn old_turns_are_dropped_when_history_does_not_fit() {
    let harness = harness(200);

    let long = "detail ".repeat(30);
    for turn in 0..4 {
        let outcome = harness
            .driver
            .run(&format!("{long} {turn}"), CancelToken::new())
            .await;
        assert!(outcome.is_done());
    }

    let requests = harness.requests.lock().unwrap();
    let last = contents(requests.last().unwrap());
    // system + the last two turns + the current input.
    assert_eq!(last.len(), 6, "history was not bounded: {last:?}");
    assert_eq!(role_name_of(&last[0]), "system");
    assert_eq!(role_name_of(&last[1]), "user");
    assert!(last[1].ends_with(" 1"));
    assert_eq!(role_name_of(&last[3]), "user");
    assert!(last[3].ends_with(" 2"));
    assert_eq!(role_name_of(&last[5]), "user");
    assert!(last[5].ends_with(" 3"));
}

fn role_name_of(line: &str) -> &str {
    line.split(':').next().unwrap_or_default()
}

#[tokio::test]
async fn older_turns_become_one_digest_block() {
    let harness = harness(420);
    let long = "detail ".repeat(30);
    let mut last = None;
    for turn in 0..8 {
        let outcome = harness
            .driver
            .run(&format!("{long} {turn}"), CancelToken::new())
            .await;
        assert!(outcome.is_done(), "turn {turn} did not finish: {outcome:?}");
        last = Some(outcome);
    }

    let requests = harness.requests.lock().unwrap();
    let last_request = contents(requests.last().unwrap());
    assert!(last_request[0].starts_with("system:You are a test agent."));
    assert!(
        last_request[1].starts_with("system:[earlier turns, compressed]"),
        "{last_request:?}"
    );
    assert!(last_request[1].contains(" 4"));
    assert!(last_request[1].contains(" 1"));
    assert!(!last_request[1].contains(" 0"));
    assert_eq!(role_name_of(&last_request[2]), "user");
    assert!(last_request[2].ends_with(" 5"));
    assert_eq!(role_name_of(&last_request[4]), "user");
    assert!(last_request[4].ends_with(" 6"));
    assert_eq!(role_name_of(&last_request[6]), "user");
    assert!(last_request[6].ends_with(" 7"));

    let turn_id = last.expect("a turn ran").turn_id;
    let assembled = harness
        .events
        .for_turn(turn_id)
        .iter()
        .find_map(|record| match &record.event {
            Event::HistoryAssembled {
                detail_percent,
                detail_turns,
                digest_turns,
                dropped_turns,
                ..
            } => Some((
                *detail_percent,
                *detail_turns,
                *digest_turns,
                *dropped_turns,
            )),
            _ => None,
        })
        .expect("history event");
    assert_eq!(assembled, (20, 2, 4, 1));
}

#[tokio::test]
async fn red_pressure_injects_one_wrap_up() {
    let harness = harness(200);
    let first = harness
        .driver
        .run(&"x".repeat(400), CancelToken::new())
        .await;
    assert!(first.is_done());

    let second = harness.driver.run("continue", CancelToken::new()).await;
    assert!(second.is_done());
    assert_eq!(second.wrap_up_report.as_deref(), Some("ack"));

    let requests = harness.requests.lock().unwrap();
    let request = &requests[1];
    assert!(request.iter().any(|message| {
        message.role == MessageRole::System
            && message.content.starts_with("Context is nearly exhausted")
    }));

    let wrap_ups = harness
        .events
        .for_turn(second.turn_id)
        .iter()
        .filter(|record| matches!(record.event, Event::WrapUpInjected { .. }))
        .count();
    assert_eq!(wrap_ups, 1);
}

#[tokio::test]
async fn a_topic_change_seals_the_segment() {
    let harness = harness(4096);
    let first = harness
        .driver
        .run("fix the parser bug", CancelToken::new())
        .await;
    assert!(first.is_done());

    let second = harness
        .driver
        .run("write a haiku about the sea", CancelToken::new())
        .await;
    assert!(second.is_done());

    let sealed = harness
        .events
        .for_turn(second.turn_id)
        .iter()
        .find_map(|record| match &record.event {
            Event::SegmentSealed {
                segment_id, reason, ..
            } => Some((*segment_id, reason.clone())),
            _ => None,
        })
        .expect("segment seal event");
    assert_eq!(sealed, (0, "topic".to_owned()));

    let opened = harness
        .events
        .for_turn(second.turn_id)
        .iter()
        .any(|record| matches!(record.event, Event::SegmentOpened { segment_id: 1, .. }));
    assert!(opened);

    // The just-sealed segment is still the newest content, so it stays
    // verbatim in the next request.
    {
        let requests = harness.requests.lock().unwrap();
        assert!(requests[1].iter().any(|message| {
            message.role == MessageRole::User && message.content == "fix the parser bug"
        }));
    }

    let third = harness
        .driver
        .run("what is the weather today", CancelToken::new())
        .await;
    assert!(third.is_done());

    let requests = harness.requests.lock().unwrap();
    let record = requests[2]
        .iter()
        .find(|message| {
            message.role == MessageRole::System && message.content.starts_with("[earlier segments]")
        })
        .expect("record block in the third request");
    assert!(record.content.contains("fix the parser bug"));
    assert!(record.content.contains("ack"));
    assert!(requests[2].iter().any(|message| {
        message.role == MessageRole::User && message.content == "write a haiku about the sea"
    }));
}

#[tokio::test]
async fn a_constraint_reaches_the_next_request() {
    let harness = harness(4096);
    let first = harness
        .driver
        .run("always use tabs", CancelToken::new())
        .await;
    assert!(first.is_done());

    let second = harness
        .driver
        .run("write a haiku about the sea", CancelToken::new())
        .await;
    assert!(second.is_done());

    // One statement is a session fact; promotion needs a second segment.
    let promoted = harness
        .events
        .for_turn(second.turn_id)
        .iter()
        .any(|record| matches!(record.event, Event::FactPromoted { .. }));
    assert!(!promoted);

    let requests = harness.requests.lock().unwrap();
    let memory = requests[1]
        .iter()
        .find(|message| {
            message.role == MessageRole::System && message.content.starts_with("[memory]")
        })
        .expect("memory block");
    assert!(memory.content.contains("always use tabs (session)"));
}

struct FixedSummarizer {
    calls: Arc<AtomicU32>,
    inputs: Arc<Mutex<Vec<String>>>,
    text: String,
}

#[async_trait]
impl Summarizer for FixedSummarizer {
    async fn summarize(
        &self,
        input: &str,
        _max_tokens: u32,
        _cancel: &CancelToken,
    ) -> Result<Summarized, ModelError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inputs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(input.to_owned());
        Ok(Summarized {
            text: self.text.clone(),
            usage: TokenUsage::default(),
        })
    }
}

struct FailingSummarizer;

#[async_trait]
impl Summarizer for FailingSummarizer {
    async fn summarize(
        &self,
        _input: &str,
        _max_tokens: u32,
        _cancel: &CancelToken,
    ) -> Result<Summarized, ModelError> {
        Err(ModelError::internal("summary failed"))
    }
}

#[tokio::test]
async fn a_sealed_segment_updates_the_session_summary() {
    let calls = Arc::new(AtomicU32::new(0));
    let inputs = Arc::new(Mutex::new(Vec::new()));
    let summarizer = Arc::new(FixedSummarizer {
        calls: calls.clone(),
        inputs: inputs.clone(),
        text: "MODEL SUMMARY".to_owned(),
    });
    let harness = harness_with(4096, &"x".repeat(4000), Some(summarizer));

    assert!(harness
        .driver
        .run("fix the parser bug", CancelToken::new())
        .await
        .is_done());
    assert!(harness
        .driver
        .run("write a haiku about the sea", CancelToken::new())
        .await
        .is_done());
    let third = harness
        .driver
        .run("what is the weather today", CancelToken::new())
        .await;
    assert!(third.is_done());

    assert!(calls.load(Ordering::Relaxed) >= 2);
    let requests = harness.requests.lock().unwrap();
    let summary = requests[2]
        .iter()
        .find(|message| {
            message.role == MessageRole::System
                && message.content.starts_with("[conversation summary")
        })
        .expect("summary block");
    assert!(summary.content.contains("MODEL SUMMARY"));

    let inputs = inputs.lock().unwrap();
    assert!(inputs.len() >= 2);
    assert!(inputs[1].contains("[previous summary]"));
    assert!(inputs[1].contains("MODEL SUMMARY"));
}

#[tokio::test]
async fn a_short_segment_still_updates_the_summary() {
    let calls = Arc::new(AtomicU32::new(0));
    let inputs = Arc::new(Mutex::new(Vec::new()));
    let summarizer = Arc::new(FixedSummarizer {
        calls: calls.clone(),
        inputs,
        text: "MODEL SUMMARY".to_owned(),
    });
    let harness = harness_with(4096, "ack", Some(summarizer));

    assert!(harness
        .driver
        .run("fix the parser bug", CancelToken::new())
        .await
        .is_done());
    assert!(harness
        .driver
        .run("write a haiku about the sea", CancelToken::new())
        .await
        .is_done());

    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let requests = harness.requests.lock().unwrap();
    assert!(requests[1].iter().any(|message| {
        message.role == MessageRole::System && message.content.starts_with("[conversation summary")
    }));
}

#[tokio::test]
async fn an_absorbed_topic_is_recalled_by_its_terms() {
    let calls = Arc::new(AtomicU32::new(0));
    let inputs = Arc::new(Mutex::new(Vec::new()));
    let summarizer = Arc::new(FixedSummarizer {
        calls,
        inputs,
        text: "MODEL SUMMARY".to_owned(),
    });
    let harness = harness_with(4096, "ack", Some(summarizer));

    for input in [
        "plan the marlin config",
        "write a haiku about the sea",
        "what is the weather today",
        "where is the marlin config",
    ] {
        assert!(
            harness
                .driver
                .run(input, CancelToken::new())
                .await
                .is_done(),
            "{input}"
        );
    }

    let requests = harness.requests.lock().unwrap();
    // The memo is the only older-history view and does not carry the term;
    // recall is what restores it.
    let summary = requests[3]
        .iter()
        .find(|message| {
            message.role == MessageRole::System
                && message.content.starts_with("[conversation summary")
        })
        .expect("summary block");
    assert!(!summary.content.contains("marlin"), "{}", summary.content);
    let recalled = requests[3]
        .iter()
        .find(|message| {
            message.role == MessageRole::System
                && message.content.starts_with("[recalled earlier segments]")
        })
        .expect("recall block");
    assert!(recalled.content.contains("marlin"), "{}", recalled.content);
}

#[tokio::test]
async fn memo_off_keeps_records_in_context() {
    let calls = Arc::new(AtomicU32::new(0));
    let inputs = Arc::new(Mutex::new(Vec::new()));
    let summarizer = Arc::new(FixedSummarizer {
        calls: calls.clone(),
        inputs,
        text: "MODEL SUMMARY".to_owned(),
    });
    let harness = harness_options(
        4096,
        "ack",
        Some(summarizer),
        keel_runtime::KernelOptions {
            memo: false,
            ..Default::default()
        },
    );

    for input in [
        "fix the parser bug",
        "write a haiku about the sea",
        "what is the weather today",
    ] {
        assert!(
            harness
                .driver
                .run(input, CancelToken::new())
                .await
                .is_done(),
            "{input}"
        );
    }

    assert_eq!(calls.load(Ordering::Relaxed), 0);
    let requests = harness.requests.lock().unwrap();
    assert!(requests[2]
        .iter()
        .all(|message| { !message.content.starts_with("[conversation summary") }));
    assert!(requests[2].iter().any(|message| {
        message.content.starts_with("[earlier segments]")
            && message.content.contains("fix the parser bug")
    }));
}

#[tokio::test]
async fn recall_off_has_no_recall_block() {
    let calls = Arc::new(AtomicU32::new(0));
    let inputs = Arc::new(Mutex::new(Vec::new()));
    let summarizer = Arc::new(FixedSummarizer {
        calls,
        inputs,
        text: "MODEL SUMMARY".to_owned(),
    });
    let harness = harness_options(
        4096,
        "ack",
        Some(summarizer),
        keel_runtime::KernelOptions {
            recall: false,
            ..Default::default()
        },
    );

    for input in [
        "plan the marlin config",
        "write a haiku about the sea",
        "what is the weather today",
        "where is the marlin config",
    ] {
        assert!(
            harness
                .driver
                .run(input, CancelToken::new())
                .await
                .is_done(),
            "{input}"
        );
    }

    let requests = harness.requests.lock().unwrap();
    assert!(requests[3]
        .iter()
        .all(|message| !message.content.starts_with("[recalled earlier segments]")));
    assert!(requests[3]
        .iter()
        .any(|message| message.content.starts_with("[conversation summary")));
}

#[tokio::test]
async fn a_summary_failure_keeps_records_in_context() {
    let summarizer: Arc<dyn Summarizer> = Arc::new(FailingSummarizer);
    let harness = harness_with(4096, "ack", Some(summarizer));

    assert!(harness
        .driver
        .run("fix the parser bug", CancelToken::new())
        .await
        .is_done());
    assert!(harness
        .driver
        .run("write a haiku about the sea", CancelToken::new())
        .await
        .is_done());
    assert!(harness
        .driver
        .run("what is the weather today", CancelToken::new())
        .await
        .is_done());

    let requests = harness.requests.lock().unwrap();
    assert!(!requests[2].iter().any(|message| {
        message.role == MessageRole::System && message.content.starts_with("[conversation summary")
    }));
    let record = requests[2]
        .iter()
        .find(|message| {
            message.role == MessageRole::System && message.content.starts_with("[earlier segments]")
        })
        .expect("record fallback");
    assert!(record.content.contains("fix the parser bug"));
}
