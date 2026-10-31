mod common;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use keel_core::{
    CancelToken, Message, MessageRole, ModelError, ModelProvider, ModelRequest, ModelStream,
    PermissionLevel, Policy,
};
use keel_runtime::{
    validate_config, ContextBuilder, DeferAllInteractionPort, EventLog, HistoryStage,
    InteractionBroker, LatestStage, MockModel, MockStep, PermissionEngine, SystemStage, TurnDriver,
    TurnDriverConfig,
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
    driver: TurnDriver<RecordingModel>,
    _dir: tempfile::TempDir,
}

fn harness(budget_tokens: u32) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let mut config = common::runtime_config(&["echo"], 4, PermissionLevel::WorkspaceAuto);
    config.context.stages = vec![
        "system".to_owned(),
        "history".to_owned(),
        "latest".to_owned(),
    ];
    config.context.budget_tokens = budget_tokens;

    let tools = common::standard_registry(dir.path());
    let context = ContextBuilder::new(vec![
        Arc::new(SystemStage::new()),
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
        reply: "ack".to_owned(),
    };
    let policy: Arc<dyn Policy> = Arc::new(config.clone());
    let driver = TurnDriver::new(
        model,
        tools,
        context,
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

    Harness {
        requests,
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
