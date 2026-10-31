use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use keel_core::{ArgValue, CancelToken, KernelError, Policy, ToolArgs};
use keel_runtime::{
    load_profile, validate_config, ContextBuilder, DeferAllInteractionPort, EchoTool, EventLog,
    InteractionBroker, LatestStage, LocalTrashStore, MockModel, MockStep, PermissionEngine,
    ReadFileTool, RuntimeConfig, SystemStage, ToolRegistry, TurnDriver, TurnDriverConfig,
    WriteFileTool,
};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {} ({})", error, error.code().as_str());
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), KernelError> {
    let (profile_path, input) = parse_args()?;
    let config = load_profile(&profile_path)?;

    let workspace = std::env::current_dir().map_err(|error| KernelError::Io {
        message: error.to_string(),
    })?;
    let permissions = Arc::new(PermissionEngine::new(workspace, config.permission_level)?);

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool::new()))?;
    registry.register(Arc::new(ReadFileTool::new()))?;
    registry.register(Arc::new(WriteFileTool::new(Arc::new(
        LocalTrashStore::new(permissions.root().to_path_buf()),
    ))))?;
    let registry = Arc::new(registry);

    let context = ContextBuilder::new(vec![
        Arc::new(SystemStage::new()),
        Arc::new(LatestStage::new()),
    ]);
    validate_config(&config, &registry, &context)?;

    let events = Arc::new(EventLog::new());
    let interactions = Arc::new(InteractionBroker::new(
        Arc::new(DeferAllInteractionPort),
        config.interaction,
        events.clone(),
    ));
    let model = MockModel::new(script_for(&config, &input));
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
            limits: config.limits(),
        },
    );

    let outcome = driver.run(&input, CancelToken::new()).await;
    println!("profile: {}", config.profile_name);
    println!("turn: {}", outcome.turn_id.get());
    println!("state: {}", outcome.state);
    println!("assistant: {}", outcome.text);
    println!("events: {}", events.len());

    if outcome.is_done() {
        Ok(())
    } else {
        let message = outcome
            .error
            .map(|error| error.to_string())
            .unwrap_or_else(|| format!("turn ended in {}", outcome.state));
        Err(KernelError::Internal { message })
    }
}

fn parse_args() -> Result<(PathBuf, String), KernelError> {
    let mut profile = PathBuf::from("variants/coding/profile.toml");
    let mut input = String::from("hello from keel");
    let mut args = std::env::args().skip(1);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--profile" => {
                profile = PathBuf::from(
                    args.next()
                        .ok_or_else(|| invalid_config("missing value for `--profile`"))?,
                );
            }
            "--input" => {
                input = args
                    .next()
                    .ok_or_else(|| invalid_config("missing value for `--input`"))?;
            }
            other => {
                return Err(invalid_config(format!("unknown argument `{other}`")));
            }
        }
    }

    Ok((profile, input))
}

fn script_for(config: &RuntimeConfig, input: &str) -> Vec<MockStep> {
    if config.visible_tool_names.iter().any(|name| name == "echo") {
        let mut args = ToolArgs::new();
        args.insert("text", ArgValue::String(input.to_owned()));
        vec![
            MockStep::ToolCall {
                name: "echo".to_owned(),
                args,
            },
            MockStep::Text(format!("mock turn complete: echoed {} bytes", input.len())),
        ]
    } else {
        vec![MockStep::Text("mock turn complete".to_owned())]
    }
}

fn invalid_config(message: impl Into<String>) -> KernelError {
    KernelError::InvalidConfig {
        message: message.into(),
    }
}
