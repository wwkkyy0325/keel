//! Run one kernel turn without the reference CLI.
//!
//! This is the smallest derived host: a profile, two context stages, one tool,
//! a mock model and a driver. Replace the mock with a provider and the prints
//! with a UI to build a real product.
//!
//! ```bash
//! cargo run -p keel-runtime --example minimal_turn
//! ```

use std::path::Path;
use std::sync::Arc;

use keel_core::{CancelToken, Policy};
use keel_runtime::{
    load_profile, validate_config, ContextBuilder, DeferAllInteractionPort, EchoTool, EventLog,
    InteractionBroker, KernelOptions, LatestStage, MockModel, MockStep, PermissionEngine,
    SystemStage, ToolRegistry, TurnDriver, TurnDriverConfig,
};

const PROFILE: &str = r#"
[profile]
name = "embed"

[model]
provider = "mock"
model = "mock"
thinking_budget = 0

[context]
stages = ["system", "latest"]
budget_tokens = 2048

[tools]
visible = ["echo"]
budget = 1

[verification]
mode = "none"

[permission]
level = "workspace_auto"

[persona]
system = "prompt.md"
"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("keel-embed-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("prompt.md"), "You are a minimal embedding host.")?;
    std::fs::write(dir.join("profile.toml"), PROFILE)?;
    let config = load_profile(&dir.join("profile.toml"))?;
    run(&dir, config).await?;
    std::fs::remove_dir_all(&dir).ok();
    Ok(())
}

async fn run(
    root: &Path,
    config: keel_runtime::RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool::new()))?;
    let registry = Arc::new(registry);

    let context = ContextBuilder::new(vec![
        Arc::new(SystemStage::new()),
        Arc::new(LatestStage::new()),
    ]);
    validate_config(&config, &registry, &context)?;

    let events = Arc::new(EventLog::new());
    let permissions = Arc::new(PermissionEngine::new(root, config.permission_level)?);
    let interactions = Arc::new(InteractionBroker::new(
        Arc::new(DeferAllInteractionPort),
        config.interaction,
        events.clone(),
    ));
    let model = MockModel::new(vec![MockStep::Text("Hello from the kernel.".to_owned())]);
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
            limits: config.limits(),
            todos: None,
            background: None,
            options: KernelOptions::default(),
        },
    );

    let outcome = driver.run("Say hello.", CancelToken::new()).await;
    println!("state: {}", outcome.state);
    println!("answer: {}", outcome.answer.body);
    println!("events: {}", events.for_turn(outcome.turn_id).len());
    Ok(())
}
