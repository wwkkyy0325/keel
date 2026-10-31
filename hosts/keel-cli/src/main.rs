mod acceptance;
mod terminal;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use keel_core::{
    ArgValue, CancelToken, Event, KernelError, ModelError, ModelProvider, ModelRequest,
    ModelStream, Policy, TodoList, ToolArgs,
};
use keel_model_openai::{OpenAiConfig, OpenAiProvider};
use keel_runtime::{
    load_profile, validate_config, CodeSearchTool, ContextBuilder, EchoTool, EventLog,
    HistoryStage, InteractionBroker, LatestStage, ListDirTool, LocalTrashStore, MemoryStage,
    MockModel, MockStep, PermissionEngine, ProviderSummarizer, ReadFileTool, RuntimeConfig,
    ShellTool, Summarizer, SystemStage, TaskStateStage, TodoTool, ToolRegistry, TurnDriver,
    TurnDriverConfig, TurnOutcome, WriteFileTool,
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
    let args = parse_args()?;
    let config = load_profile(&args.profile)?;

    let permissions = Arc::new(PermissionEngine::new(
        args.workspace,
        config.permission_level,
    )?);

    let todos = Arc::new(Mutex::new(TodoList::new()));
    if let Some(path) = &args.todo_file {
        let text = std::fs::read_to_string(path).map_err(|error| KernelError::Io {
            message: format!("{}: {error}", path.display()),
        })?;
        let mut list = todos
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            list.add_host(line)
                .map_err(|error| invalid_config(error.to_string()))?;
        }
        list.lock();
    }

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool::new()))?;
    registry.register(Arc::new(ReadFileTool::new()))?;
    registry.register(Arc::new(ListDirTool::new()))?;
    registry.register(Arc::new(CodeSearchTool::new()))?;
    registry.register(Arc::new(ShellTool::coding_defaults()))?;
    registry.register(Arc::new(WriteFileTool::new(Arc::new(
        LocalTrashStore::new(permissions.root().to_path_buf()),
    ))))?;
    registry.register(Arc::new(TodoTool::new(todos.clone())))?;
    let registry = Arc::new(registry);

    let context = ContextBuilder::new(vec![
        Arc::new(SystemStage::new()),
        Arc::new(TaskStateStage::new()),
        Arc::new(MemoryStage::new()),
        Arc::new(HistoryStage::new()),
        Arc::new(LatestStage::new()),
    ]);
    validate_config(&config, &registry, &context)?;

    let events = Arc::new(EventLog::new());
    let interactions = Arc::new(InteractionBroker::new(
        Arc::new(TerminalInteractionPort),
        config.interaction,
        events.clone(),
    ));
    let model = build_model(&config, &args.input, args.session)?;
    let policy: Arc<dyn Policy> = Arc::new(config.clone());
    let mut limits = config.limits();
    if let Some(calls) = args.max_model_calls {
        limits.max_model_calls = calls;
    }
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
            limits,
            todos: Some(todos.clone()),
        },
    );

    let driver = match build_summarizer(&config)? {
        Some(summarizer) => driver.with_summarizer(summarizer),
        None => driver,
    };
    let driver = match &args.acceptance {
        Some(path) => driver.with_acceptance(acceptance::load(path)?, 2),
        None => driver,
    };

    if args.session {
        return run_session(&driver, &config, &events, &todos).await;
    }

    let outcome = driver.run(&args.input, CancelToken::new()).await;
    print_outcome(&config, &events, &outcome);
    print_todos(&todos);
    done_or_error(outcome)
}

/// Reads inputs from stdin until EOF, keeping one session across turns.
async fn run_session(
    driver: &TurnDriver<HostModel>,
    config: &RuntimeConfig,
    events: &Arc<EventLog>,
    todos: &Arc<Mutex<TodoList>>,
) -> Result<(), KernelError> {
    use std::io::{BufRead, IsTerminal, Write};

    let stdin = std::io::stdin();
    let interactive = stdin.is_terminal();
    let mut line = String::new();
    loop {
        if interactive {
            print!("keel> ");
            let _ = std::io::stdout().flush();
        }
        line.clear();
        let read = stdin
            .lock()
            .read_line(&mut line)
            .map_err(|error| KernelError::Io {
                message: error.to_string(),
            })?;
        if read == 0 {
            break;
        }
        let input = line.trim_end_matches(['\r', '\n']);
        if input.trim().is_empty() {
            continue;
        }
        let outcome = driver.run(input, CancelToken::new()).await;
        print_outcome(config, events, &outcome);
        print_todos(todos);
        if outcome.is_cancelled() {
            break;
        }
    }
    Ok(())
}

fn print_todos(todos: &Arc<Mutex<TodoList>>) {
    let list = todos
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if list.items.is_empty() {
        return;
    }
    if list.state.is_closed() {
        println!(
            "tasks: closed={} completed={} failed={} total={}",
            list.state.as_str(),
            list.completed(),
            list.failed(),
            list.items.len()
        );
        return;
    }
    let current = list
        .in_progress()
        .map(|item| format!("#{}", item.id))
        .unwrap_or_else(|| "-".to_owned());
    println!(
        "tasks: done={} current={} total={}",
        list.completed(),
        current,
        list.items.len()
    );
}

fn print_outcome(config: &RuntimeConfig, events: &EventLog, outcome: &TurnOutcome) {
    println!("profile: {}", config.profile_name);
    println!("turn: {}", outcome.turn_id.get());
    println!("state: {}", outcome.state);
    if let Some(error) = &outcome.error {
        println!("error: {error} ({})", error.code().as_str());
    }
    if let Some((estimate, budget)) =
        events
            .for_turn(outcome.turn_id)
            .iter()
            .rev()
            .find_map(|record| match &record.event {
                Event::ContextBuilt { tokens, budget, .. } => Some((*tokens, *budget)),
                _ => None,
            })
    {
        println!("context: estimate={estimate} budget={budget}");
    }
    if let Some((detail, digest, records, summary)) = events
        .for_turn(outcome.turn_id)
        .iter()
        .rev()
        .find_map(|record| match &record.event {
            Event::HistoryAssembled {
                detail_turns,
                digest_turns,
                record_segments,
                summary_tokens,
                ..
            } => Some((
                *detail_turns,
                *digest_turns,
                *record_segments,
                *summary_tokens,
            )),
            _ => None,
        })
    {
        println!("history: detail={detail} digest={digest} records={records} summary={summary}");
    }
    if let Some(allocation) = events
        .for_turn(outcome.turn_id)
        .iter()
        .rev()
        .find_map(|record| match &record.event {
            Event::HistoryAssembled {
                recall_tokens,
                recall_segments,
                recall_candidates,
                record_tokens,
                record_segments,
                record_candidates,
                detail_tokens,
                digest_tokens,
                ..
            } => Some(format!(
                "recall={recall_tokens}({recall_segments}/{recall_candidates}) \
                     records={record_tokens}({record_segments}/{record_candidates}) \
                     detail={detail_tokens} digest={digest_tokens}"
            )),
            _ => None,
        })
    {
        println!("alloc: {allocation}");
    }
    if let Some((segments, tokens)) =
        events
            .for_turn(outcome.turn_id)
            .iter()
            .rev()
            .find_map(|record| match &record.event {
                Event::HistoryRecalled {
                    segments, tokens, ..
                } => Some((segments.clone(), *tokens)),
                _ => None,
            })
    {
        println!("recall: segments={segments:?} tokens={tokens}");
    }
    if let Some((revision, covers, chars, budget)) = events
        .for_turn(outcome.turn_id)
        .iter()
        .rev()
        .find_map(|record| match &record.event {
            Event::SummaryUpdated {
                revision,
                covers,
                chars,
                budget,
                ..
            } => Some((*revision, covers.len(), *chars, *budget)),
            _ => None,
        })
    {
        println!("summary: revision={revision} covers={covers} chars={chars} budget={budget}");
    }
    if let Some(reason) = events
        .for_turn(outcome.turn_id)
        .iter()
        .rev()
        .find_map(|record| match &record.event {
            Event::SummaryFailed { reason, .. } => Some(reason.clone()),
            _ => None,
        })
    {
        println!("summary: update failed: {reason}");
    }
    println!("answer: {}", outcome.answer.body);
    if outcome.answer.missing {
        println!("notice: the turn ended without an answer; the transcript is kept separately");
    }
    if !outcome.answer.changes.is_empty() {
        let paths: Vec<&str> = outcome
            .answer
            .changes
            .iter()
            .map(|change| change.path.as_str())
            .collect();
        println!("changes: {}", paths.join(", "));
    }
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
}

fn done_or_error(outcome: TurnOutcome) -> Result<(), KernelError> {
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

fn build_openai_provider(
    config: &RuntimeConfig,
    max_retries: u32,
) -> Result<OpenAiProvider, KernelError> {
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
    OpenAiProvider::new(OpenAiConfig {
        base_url,
        api_key,
        model: config.model_route.model.clone(),
        supports_reasoning: config.supports_reasoning,
        max_retries,
    })
    .map_err(|error| invalid_config(error.to_string()))
}

fn build_summarizer(config: &RuntimeConfig) -> Result<Option<Arc<dyn Summarizer>>, KernelError> {
    match config.model_route.provider.as_str() {
        "openai" | "openai-compatible" => {
            let provider = build_openai_provider(config, 1)?;
            Ok(Some(Arc::new(ProviderSummarizer::new(provider))))
        }
        _ => Ok(None),
    }
}

fn build_model(
    config: &RuntimeConfig,
    input: &str,
    session: bool,
) -> Result<HostModel, KernelError> {
    match config.model_route.provider.as_str() {
        "mock" => {
            let model = if session {
                MockModel::repeating(vec![MockStep::Text("mock session turn".to_owned())])
            } else {
                MockModel::new(script_for(config, input))
            };
            Ok(HostModel::Mock(model))
        }
        "openai" | "openai-compatible" => {
            let provider = build_openai_provider(config, 2)?;
            Ok(HostModel::OpenAi(provider))
        }
        other => Err(invalid_config(format!("unknown model provider `{other}`"))),
    }
}

struct Args {
    profile: PathBuf,
    input: String,
    workspace: PathBuf,
    acceptance: Option<PathBuf>,
    session: bool,
    max_model_calls: Option<u32>,
    todo_file: Option<PathBuf>,
}

fn parse_args() -> Result<Args, KernelError> {
    let mut profile = PathBuf::from("variants/coding/profile.toml");
    let mut input = String::from("hello from keel");
    let mut acceptance = None;
    let mut session = false;
    let mut max_model_calls = None;
    let mut todo_file = None;
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
            "--session" => {
                session = true;
            }
            "--max-model-calls" => {
                let value = args
                    .next()
                    .ok_or_else(|| invalid_config("missing value for `--max-model-calls`"))?;
                let calls = value.parse::<u32>().map_err(|_| {
                    invalid_config("`--max-model-calls` must be a positive integer")
                })?;
                if calls == 0 {
                    return Err(invalid_config(
                        "`--max-model-calls` must be a positive integer",
                    ));
                }
                max_model_calls = Some(calls);
            }
            "--todo-file" => {
                todo_file =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        invalid_config("missing value for `--todo-file`")
                    })?));
            }
            other => {
                return Err(invalid_config(format!("unknown argument `{other}`")));
            }
        }
    }

    Ok(Args {
        profile,
        input,
        workspace,
        acceptance,
        session,
        max_model_calls,
        todo_file,
    })
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
