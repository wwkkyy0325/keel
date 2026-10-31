//! Measures per-turn kernel overhead with the mock model.
//!
//! Run with `cargo run --release -p keel-runtime --example turn_overhead`.
//! The budget from `docs/spec/04-evals.md` is p50 < 5 ms and p99 < 20 ms.

use std::sync::Arc;
use std::time::{Duration, Instant};

use keel_core::{
    CancelToken, CompactionStep, ContextRecipe, InteractionPolicy, MemoryMode, MemoryPolicy,
    ModelRoute, PermissionLevel, Policy, PromotionMode, VerificationMode,
};
use keel_runtime::{
    ContextBuilder, DeferAllInteractionPort, EventLog, InteractionBroker, LatestStage, MockModel,
    MockStep, PermissionEngine, RuntimeConfig, SystemStage, ToolRegistry, TurnDriver,
    TurnDriverConfig, TurnLimits,
};

const TURNS: usize = 1000;
const WARMUP: usize = 20;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = RuntimeConfig {
        profile_name: "benchmark".to_owned(),
        system_prompt: "You are a benchmark agent.".to_owned(),
        visible_tool_names: Vec::new(),
        plan_mode: false,
        tool_budget: 0,
        context: ContextRecipe {
            stages: vec!["system".to_owned(), "latest".to_owned()],
            budget_tokens: 4096,
            compaction: Vec::<CompactionStep>::new(),
        },
        thinking_budget: 0,
        verification: VerificationMode::None,
        memory: MemoryPolicy {
            mode: MemoryMode::Off,
            promotion: PromotionMode::Strict,
            recall_limit: 8,
        },
        permission_level: PermissionLevel::WorkspaceAuto,
        model_route: ModelRoute {
            provider: "mock".to_owned(),
            model: "mock-benchmark".to_owned(),
        },
        model_base_url: None,
        api_key_env: None,
        supports_reasoning: false,
        max_output_tokens: 256,
        context_window: 128_000,
        interaction: InteractionPolicy::default(),
    };

    let events = Arc::new(EventLog::new());
    let permissions = Arc::new(PermissionEngine::new(
        std::env::current_dir()?,
        config.permission_level,
    )?);
    let interactions = Arc::new(InteractionBroker::new(
        Arc::new(DeferAllInteractionPort),
        config.interaction,
        events.clone(),
    ));
    let model = MockModel::repeating(vec![MockStep::Text("benchmark response".to_owned())]);
    let policy: Arc<dyn Policy> = Arc::new(config);
    let driver = TurnDriver::new(
        model,
        Arc::new(ToolRegistry::new()),
        ContextBuilder::new(vec![
            Arc::new(SystemStage::new()),
            Arc::new(LatestStage::new()),
        ]),
        policy,
        TurnDriverConfig {
            events,
            permissions,
            interactions,
            system_prompt: "You are a benchmark agent.".to_owned(),
            max_output_tokens: 256,
            context_window: 128_000,
            limits: TurnLimits {
                max_model_calls: 4,
                max_tool_calls: 0,
                tool_timeout: Duration::from_secs(1),
                turn_timeout: Duration::from_secs(5),
            },
            todos: None,
        },
    );

    for _ in 0..WARMUP {
        let outcome = driver.run("warmup", CancelToken::new()).await;
        if !outcome.is_done() {
            return Err(std::io::Error::other("warmup turn did not finish").into());
        }
    }

    let mut durations = Vec::with_capacity(TURNS);
    for _ in 0..TURNS {
        let started = Instant::now();
        let outcome = driver.run("benchmark input", CancelToken::new()).await;
        durations.push(started.elapsed());
        if !outcome.is_done() {
            return Err(std::io::Error::other("benchmark turn did not finish").into());
        }
    }

    durations.sort();
    let p50 = durations[durations.len() / 2];
    let p99 = durations[durations.len() * 99 / 100];
    println!("turns: {TURNS}");
    println!("p50_us: {}", p50.as_micros());
    println!("p99_us: {}", p99.as_micros());
    println!("budget_us: p50 < 5000, p99 < 20000");
    Ok(())
}
