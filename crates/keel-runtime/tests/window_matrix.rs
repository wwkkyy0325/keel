mod common;

use std::sync::Arc;

use async_trait::async_trait;
use keel_core::{
    ArgValue, CancelToken, KernelError, MessageRole, ModelError, ModelProvider, ModelRequest,
    ModelStream, PermissionLevel, Policy, ToolArgs,
};
use keel_runtime::{
    validate_config, ContextBuilder, DeferAllInteractionPort, EventLog, HistoryStage,
    InteractionBroker, LatestStage, MemoryStage, MockModel, MockStep, PermissionEngine,
    SystemStage, TurnDriver, TurnDriverConfig,
};

const WINDOWS: [u32; 6] = [8_192, 16_384, 32_768, 65_536, 131_072, 1_048_576];
const TURNS: usize = 40;

#[derive(Default)]
struct Metrics {
    done: u32,
    overflow: u32,
    failed: u32,
    wrap_ups: u32,
    digest_turns: u32,
    records: u32,
    tokens: u64,
}

/// Returns a tool call first, then a fixed-size answer after the tool result.
struct GrowthModel {
    reply: String,
    tool: bool,
}

#[async_trait]
impl ModelProvider for GrowthModel {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        let last_is_tool = request
            .messages
            .last()
            .map(|message| message.role == MessageRole::Tool)
            .unwrap_or(false);
        let step = if self.tool && !last_is_tool {
            let mut args = ToolArgs::new();
            args.insert("text", ArgValue::String("y".repeat(2000)));
            MockStep::TextAndToolCall {
                text: "checking".to_owned(),
                name: "echo".to_owned(),
                args,
            }
        } else {
            MockStep::Text(self.reply.clone())
        };
        MockModel::new(vec![step]).stream(request, cancel).await
    }
}

fn config_for(window: u32, tools: bool) -> keel_runtime::RuntimeConfig {
    let mut config = common::runtime_config(
        if tools { &["echo"] } else { &[] },
        8,
        PermissionLevel::WorkspaceAuto,
    );
    config.context.stages = vec![
        "system".to_owned(),
        "memory".to_owned(),
        "history".to_owned(),
        "latest".to_owned(),
    ];
    let max_output = if tools { 4096 } else { 1024 };
    config.max_output_tokens = max_output;
    config.context_window = window;
    config.context.budget_tokens = window.saturating_sub(max_output);
    config
}

async fn run_window(window: u32, tools: bool) -> Metrics {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(EventLog::new());
    let config = config_for(window, tools);
    let registry = common::standard_registry(dir.path());
    let context = ContextBuilder::new(vec![
        Arc::new(SystemStage::new()),
        Arc::new(MemoryStage::new()),
        Arc::new(HistoryStage::new()),
        Arc::new(LatestStage::new()),
    ]);
    validate_config(&config, &registry, &context).unwrap();

    let permissions = Arc::new(PermissionEngine::new(dir.path(), config.permission_level).unwrap());
    let interactions = Arc::new(InteractionBroker::new(
        Arc::new(DeferAllInteractionPort),
        config.interaction,
        events.clone(),
    ));
    let model = GrowthModel {
        reply: "done ".repeat(400),
        tool: tools,
    };
    let policy: Arc<dyn Policy> = Arc::new(config.clone());
    let driver = TurnDriver::new(
        model,
        registry,
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
        },
    );

    let mut metrics = Metrics::default();
    for turn in 0..TURNS {
        let outcome = driver
            .run(&format!("turn {turn}"), CancelToken::new())
            .await;
        if outcome.is_done() {
            metrics.done += 1;
        } else if matches!(outcome.error, Some(KernelError::ContextOverflow { .. })) {
            metrics.overflow += 1;
        } else {
            metrics.failed += 1;
        }
        metrics.tokens = metrics.tokens.saturating_add(
            u64::from(outcome.usage.input_tokens)
                .saturating_add(u64::from(outcome.usage.output_tokens)),
        );

        for record in events.for_turn(outcome.turn_id) {
            match record.event {
                keel_core::Event::WrapUpInjected { .. } => {
                    metrics.wrap_ups = metrics.wrap_ups.saturating_add(1);
                }
                keel_core::Event::HistoryAssembled {
                    digest_turns,
                    record_segments,
                    ..
                } => {
                    metrics.digest_turns = metrics.digest_turns.saturating_add(digest_turns);
                    metrics.records = metrics.records.saturating_add(record_segments);
                }
                _ => {}
            }
        }
    }
    metrics
}

#[tokio::test]
async fn the_window_matrix_records_the_supported_floor() {
    let mut lines = Vec::new();
    for tools in [false, true] {
        for window in WINDOWS {
            let metrics = run_window(window, tools).await;
            lines.push(format!(
                "{:>8} {:>7} done={:>2}/{} overflow={} failed={} wrap_ups={} digests={} records={} tokens={}",
                window,
                if tools { "coding" } else { "companion" },
                metrics.done,
                TURNS,
                metrics.overflow,
                metrics.failed,
                metrics.wrap_ups,
                metrics.digest_turns,
                metrics.records,
                metrics.tokens
            ));
            if !tools {
                assert_eq!(
                    metrics.done, TURNS as u32,
                    "companion at {window}: {lines:?}"
                );
            } else if window >= 32_768 {
                assert_eq!(metrics.done, TURNS as u32, "coding at {window}: {lines:?}");
            } else if window == 8_192 {
                assert!(
                    metrics.overflow > 0,
                    "8k coding was expected to overflow: {lines:?}"
                );
            }
        }
    }
    for line in lines {
        println!("{line}");
    }
}
