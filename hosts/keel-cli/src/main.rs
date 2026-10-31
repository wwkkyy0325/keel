mod acceptance;
mod terminal;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use async_trait::async_trait;
use keel_core::{
    ArgValue, CancelToken, KernelError, ModelError, ModelProvider, ModelRequest, ModelStream,
    Policy, ToolArgs,
};
use keel_model_openai::{OpenAiConfig, OpenAiProvider};
use keel_runtime::{
    load_profile, validate_config, CodeSearchTool, ContextBuilder, EchoTool, EventLog,
    InteractionBroker, LatestStage, ListDirTool, LocalTrashStore, MockModel, MockStep,
    PermissionEngine, ReadFileTool, RuntimeConfig, ShellTool, SystemStage, ToolRegistry,
    TurnDriver, TurnDriverConfig, WriteFileTool,
};
use terminal::TerminalInteractionPort;

enum HostModel {
    Mock(MockModel),
    OpenAi(OpenAiProvider),
}

#[async_trait]
impl ModelProvider for HostModel {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        match self {
            Self::Mock(model) => model.stream(request, cancel).await,
            Self::OpenAi(model) => model.stream(request, cancel).await,
        }
    }
}

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
    dotenvy::dotenv().ok();
    let (profile_path, input, workspace, acceptance_path) = parse_args()?;
    let config = load_profile(&profile_path)?;

    let permissions = Arc::new(PermissionEngine::new(workspace, config.permission_level)?);

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool::new()))?;
    registry.register(Arc::new(ReadFileTool::new()))?;
    registry.register(Arc::new(ListDirTool::new()))?;
    registry.register(Arc::new(CodeSearchTool::new()))?;
    registry.register(Arc::new(ShellTool::coding_defaults()))?;
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
        Arc::new(TerminalInteractionPort),
        config.interaction,
        events.clone(),
    ));
    let model = build_model(&config, &input)?;
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

    let driver = match acceptance_path {
        Some(path) => driver.with_acceptance(acceptance::load(&path)?, 2),
        None => driver,
    };
    let outcome = driver.run(&input, CancelToken::new()).await;
    println!("profile: {}", config.profile_name);
    println!("turn: {}", outcome.turn_id.get());
    println!("state: {}", outcome.state);
    println!("assistant: {}", outcome.text);
    println!("events: {}", events.len());
    println!(
        "usage: input={} output={} total={}",
        outcome.usage.input_tokens,
        outcome.usage.output_tokens,
        outcome
            .usage
            .input_tokens
            .saturating_add(outcome.usage.output_tokens)
    );
    println!("duration_ms: {}", outcome.duration_ms);
    println!("reasoning: {}", outcome.reasoning_level.as_str());
    if let Some(report) = &outcome.verification {
        println!(
            "verification: {} required_failed={:?}",
            if report.passed() { "passed" } else { "failed" },
            report.required_failed
        );
    }
    if let Some(kind) = outcome.secret_warning {
        println!(
            "notice: this turn's input matched a secret pattern ({}); it was not blocked. If it is real, rotate it and keep credentials in .env.",
            kind.as_str()
        );
    }

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

fn build_model(config: &RuntimeConfig, input: &str) -> Result<HostModel, KernelError> {
    match config.model_route.provider.as_str() {
        "mock" => Ok(HostModel::Mock(MockModel::new(script_for(config, input)))),
        "openai" | "openai-compatible" => {
            let base_url = config
                .model_base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_owned());
            let api_key_env = config
                .api_key_env
                .clone()
                .unwrap_or_else(|| "OPENAI_API_KEY".to_owned());
            let api_key = std::env::var(&api_key_env).map_err(|_| {
                invalid_config(format!(
                    "{api_key_env} is not set. Copy .env.example to .env and fill it locally; never paste keys into the chat."
                ))
            })?;
            if api_key.trim().is_empty() {
                return Err(invalid_config(format!("{api_key_env} is empty")));
            }
            let provider = OpenAiProvider::new(OpenAiConfig {
                base_url,
                api_key,
                model: config.model_route.model.clone(),
                supports_reasoning: config.supports_reasoning,
                max_retries: 2,
            })
            .map_err(|error| invalid_config(error.to_string()))?;
            Ok(HostModel::OpenAi(provider))
        }
        other => Err(invalid_config(format!("unknown model provider `{other}`"))),
    }
}

fn parse_args() -> Result<(PathBuf, String, PathBuf, Option<PathBuf>), KernelError> {
    let mut profile = PathBuf::from("variants/coding/profile.toml");
    let mut input = String::from("hello from keel");
    let mut acceptance = None;
    let mut workspace = std::env::current_dir().map_err(|error| KernelError::Io {
        message: error.to_string(),
    })?;
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
            "--workspace" => {
                workspace = PathBuf::from(
                    args.next()
                        .ok_or_else(|| invalid_config("missing value for `--workspace`"))?,
                );
            }
            "--acceptance" => {
                acceptance =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        invalid_config("missing value for `--acceptance`")
                    })?));
            }
            other => {
                return Err(invalid_config(format!("unknown argument `{other}`")));
            }
        }
    }

    Ok((profile, input, workspace, acceptance))
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
