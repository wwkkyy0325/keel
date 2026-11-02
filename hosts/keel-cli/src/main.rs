mod acceptance;
mod bundle;
mod job;
mod mcp;
mod store;
mod terminal;
mod tui;
mod view;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    AcceptanceItem, AccessKind, ArgValue, CancelToken, Event, InteractionPort, KernelError,
    ModelChunk, ModelError, ModelProvider, ModelRequest, ModelStream, Policy, Session, TodoList,
    ToolArgs,
};
use keel_model_openai::{OpenAiConfig, OpenAiProvider};
use keel_runtime::{
    load_profile, validate_config, AcceptanceVerifier, CodeSearchTool, ContextBuilder,
    DeleteFileTool, EchoTool, EventLog, HistoryStage, InteractionBroker, KernelOptions,
    LatestStage, ListDirTool, ListTrashTool, LocalTrashStore, MakeDirTool, MemoryStage, MockModel,
    MockStep, MoveFileTool, PermissionEngine, ProviderSummarizer, ReadFileTool, RestoreFileTool,
    RuntimeConfig, ShellTool, Summarizer, SystemStage, TaskStateStage, TodoTool, ToolRegistry,
    TrashStore, TurnDriver, TurnDriverConfig, TurnOutcome, WriteFileTool,
};
use terminal::TerminalInteractionPort;

enum HostModel {
    Mock(MockModel),
    OpenAi(OpenAiProvider),
    /// A paced script for `--demo`: a tool call with a visible wait, then an
    /// answer streamed piece by piece.
    Demo(DemoModel),
    /// Forwards text deltas to the TUI while the driver consumes the stream.
    Tui {
        inner: Box<HostModel>,
        ui: tokio::sync::mpsc::Sender<tui::UiEvent>,
    },
    /// Delays chunks so a mock turn looks like a real streaming turn.
    Paced {
        inner: Box<HostModel>,
        delay: Duration,
    },
}

impl HostModel {
    fn with_ui(self, ui: tokio::sync::mpsc::Sender<tui::UiEvent>) -> Self {
        Self::Tui {
            inner: Box::new(self),
            ui,
        }
    }

    fn with_pacing(self, delay: Duration) -> Self {
        Self::Paced {
            inner: Box::new(self),
            delay,
        }
    }
}

/// Two model calls per turn: a tool call, then the answer. The counter keeps
/// repeating so every turn in a demo shows both phases.
struct DemoModel {
    calls: AtomicU64,
}

impl DemoModel {
    fn new() -> Self {
        Self {
            calls: AtomicU64::new(0),
        }
    }
}

const DEMO_ANSWER: &str = "The check finished. This answer is streamed piece by piece so you can see the process: a model call, a tool call with its permission prompt, the tool result, and then the answer. When the turn ends the intermediate lines disappear and only this answer and its aggregate fields remain.";

fn demo_tool_step() -> MockStep {
    let mut args = ToolArgs::new();
    args.insert("program", ArgValue::String("python".to_owned()));
    args.insert(
        "args",
        ArgValue::StringList(vec![
            "-c".to_owned(),
            "import time; time.sleep(2); print('check passed')".to_owned(),
        ]),
    );
    MockStep::TextAndToolCall {
        text: "Let me run a quick check before answering.".to_owned(),
        name: "shell".to_owned(),
        args,
    }
}

#[async_trait]
impl ModelProvider for DemoModel {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        let call = self.calls.fetch_add(1, Ordering::Relaxed);
        let step = if call % 2 == 0 {
            demo_tool_step()
        } else {
            MockStep::Text(DEMO_ANSWER.to_owned())
        };
        MockModel::new(vec![step]).stream(request, cancel).await
    }
}

fn text_chunks(text: &str, size: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut count = 0;
    for character in text.chars() {
        current.push(character);
        count += 1;
        if count >= size {
            chunks.push(std::mem::take(&mut current));
            count = 0;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
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
            Self::Demo(model) => model.stream(request, cancel).await,
            Self::Paced { inner, delay } => {
                let mut stream = inner.stream(request, cancel).await?;
                let delay = *delay;
                let cancel = cancel.clone();
                let (tx, rx) = tokio::sync::mpsc::channel(64);
                tokio::spawn(async move {
                    while let Some(chunk) = stream.recv().await {
                        if cancel.is_cancelled() {
                            break;
                        }
                        match chunk {
                            Ok(ModelChunk::TextDelta(text)) => {
                                for piece in text_chunks(&text, 6) {
                                    tokio::time::sleep(delay).await;
                                    if tx.send(Ok(ModelChunk::TextDelta(piece))).await.is_err() {
                                        return;
                                    }
                                }
                            }
                            other => {
                                tokio::time::sleep(delay).await;
                                if tx.send(other).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                });
                Ok(ModelStream::new(rx))
            }
            Self::Tui { inner, ui } => {
                let mut stream = inner.stream(request, cancel).await?;
                let ui = ui.clone();
                let (tx, rx) = tokio::sync::mpsc::channel(64);
                tokio::spawn(async move {
                    while let Some(chunk) = stream.recv().await {
                        if let Ok(ModelChunk::TextDelta(text)) = &chunk {
                            let _ = ui.send(tui::UiEvent::Text(text.clone())).await;
                        }
                        if tx.send(chunk).await.is_err() {
                            break;
                        }
                    }
                });
                Ok(ModelStream::new(rx))
            }
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {} ({})", error, error.code().as_str());
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode, KernelError> {
    dotenvy::dotenv().ok();
    let mut args = parse_args()?;
    if let Some(path) = &args.bundle {
        let bundle = bundle::Bundle::load(path)?;
        args.profile = bundle.profile;
        if args.plugins.is_none() {
            args.plugins = bundle.plugins;
        }
        if args.state_dir.is_none() {
            args.state_dir = bundle.state_dir;
        }
    }
    if args.json && !args.plugins_check {
        return Err(invalid_config(
            "`--json` is only supported with `--plugins-check`",
        ));
    }
    if args.plugins_check {
        let path = args
            .plugins
            .as_deref()
            .ok_or_else(|| invalid_config("`--plugins-check` needs `--plugins <file>`"))?;
        let mut roots = vec![args.workspace.clone()];
        roots.extend(args.roots.iter().map(|(path, _)| path.clone()));
        return check_plugins(path, &roots, args.json).await;
    }
    let state_dir = match &args.state_dir {
        Some(dir) => {
            std::fs::create_dir_all(dir).map_err(|error| KernelError::Io {
                message: format!("cannot create state directory `{}`: {error}", dir.display()),
            })?;
            Some(std::fs::canonicalize(dir).map_err(|error| KernelError::Io {
                message: format!(
                    "cannot resolve state directory `{}`: {error}",
                    dir.display()
                ),
            })?)
        }
        None => None,
    };
    let store_path = resolve_store_path(&args.store, &state_dir);
    let mut store = match &store_path {
        Some(path) => Some(store::Store::open(path)?),
        None => None,
    };
    if let (Some(store), Some(keep)) = (store.as_mut(), args.keep_checkpoints) {
        store.set_checkpoint_limit(keep);
    }
    let windowed = matches!(args.view.as_deref(), Some("session" | "events"));
    if (args.limit != 0 || args.before.is_some()) && !windowed {
        return Err(invalid_config(
            "`--limit` and `--before` apply to `--view session` or `--view events`",
        ));
    }
    if let Some(name) = &args.view {
        let Some(store) = store.as_ref() else {
            return Err(invalid_config("`--view` needs `--store`"));
        };
        match name.as_str() {
            "sessions" => print_json(&view::sessions(store.connection())?)?,
            "session" => {
                let window = view::TurnWindow {
                    limit: args.limit,
                    before: args.before,
                };
                let Some(session) =
                    view::session_window(store.connection(), &args.session_id, window)?
                else {
                    return Err(invalid_config(format!(
                        "session `{}` not found",
                        args.session_id
                    )));
                };
                print_json(&session)?;
            }
            "turn" => {
                let Some(seq) = args.turn else {
                    return Err(invalid_config("`--view turn` needs `--turn`"));
                };
                let Some(detail) = view::turn(store.connection(), &args.session_id, seq)? else {
                    return Err(invalid_config(format!(
                        "turn {seq} of session `{}` not found",
                        args.session_id
                    )));
                };
                print_json(&detail)?;
            }
            "events" => {
                let window = view::TurnWindow {
                    limit: args.limit,
                    before: args.before,
                };
                let Some(events) = view::events(store.connection(), &args.session_id, window)?
                else {
                    return Err(invalid_config(format!(
                        "session `{}` not found",
                        args.session_id
                    )));
                };
                print_json(&events)?;
            }
            "memory" => {
                let Some(memory) = view::memory(store.connection(), &args.session_id)? else {
                    return Err(invalid_config(format!(
                        "session `{}` not found",
                        args.session_id
                    )));
                };
                print_json(&memory)?;
            }
            other => return Err(invalid_config(format!("unknown view `{other}`"))),
        }
        return Ok(ExitCode::SUCCESS);
    }
    if let Some(session_id) = &args.delete_session {
        let Some(store) = store.as_mut() else {
            return Err(invalid_config("`--delete-session` needs `--store`"));
        };
        if !store.delete_session(session_id)? {
            return Err(invalid_config(format!("session `{session_id}` not found")));
        }
        println!("deleted session {session_id}");
        return Ok(ExitCode::SUCCESS);
    }
    if let Some(value) = &args.fork_from {
        let Some(store) = store.as_mut() else {
            return Err(invalid_config("`--fork-from` needs `--store`"));
        };
        let (source, turn) = parse_fork(value)?;
        store.fork(&source, turn, &args.session_id)?;
        println!("forked {source}:{turn} into {}", args.session_id);
    }

    let mut config = load_profile(&args.profile)?;
    if args.no_todo {
        config.visible_tool_names.retain(|name| name != "todo");
    }

    let mut permissions = PermissionEngine::new(&args.workspace, config.permission_level)?;
    for (path, access) in &args.roots {
        permissions = permissions.with_root(path, *access)?;
    }
    let outputs_dir = state_dir
        .as_ref()
        .map(|dir| dir.join("outputs"))
        .unwrap_or_else(|| permissions.root().join(".keel").join("outputs"));
    permissions = permissions.with_outputs(outputs_dir.clone());
    let permissions = Arc::new(permissions);

    let stored = match &store {
        Some(store) => store.load(&args.session_id)?,
        None => None,
    };
    let (restored_session, restored_todos) = match stored {
        Some(stored) => (Some(stored.snapshot), stored.todos),
        None => (None, None),
    };
    let had_stored_todos = restored_todos.is_some();

    let todos = if args.no_todo {
        None
    } else {
        Some(Arc::new(Mutex::new(
            restored_todos.unwrap_or_else(TodoList::new),
        )))
    };
    if !had_stored_todos {
        if let (Some(todos), Some(path)) = (&todos, &args.todo_file) {
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
    }

    let mut limits = config.limits();
    if let Some(calls) = args.max_model_calls {
        limits.max_model_calls = calls;
    }
    if let Some(ms) = args.turn_timeout_ms {
        limits.turn_timeout = std::time::Duration::from_millis(ms);
    }
    if let Some(ms) = args.tool_timeout_ms {
        limits.tool_timeout = std::time::Duration::from_millis(ms);
    }

    let trash: Arc<dyn TrashStore> = match &state_dir {
        Some(dir) => Arc::new(LocalTrashStore::at(dir.join("trash"))),
        None => Arc::new(LocalTrashStore::new(permissions.root().to_path_buf())),
    };

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool::new()))?;
    registry.register(Arc::new(ReadFileTool::new()))?;
    registry.register(Arc::new(ListDirTool::new()))?;
    registry.register(Arc::new(MakeDirTool::new()))?;
    registry.register(Arc::new(CodeSearchTool::new()))?;
    registry.register(Arc::new(
        ShellTool::coding_defaults().with_timeout(limits.tool_timeout),
    ))?;
    registry.register(Arc::new(WriteFileTool::new(trash.clone())))?;
    registry.register(Arc::new(MoveFileTool::new(trash.clone())))?;
    registry.register(Arc::new(DeleteFileTool::new(trash.clone())))?;
    registry.register(Arc::new(ListTrashTool::new(trash.clone())))?;
    registry.register(Arc::new(RestoreFileTool::new(trash)))?;
    if let Some(todos) = &todos {
        registry.register(Arc::new(TodoTool::new(todos.clone())))?;
    }
    if let Some(path) = &args.plugins {
        let mcp_roots: Vec<PathBuf> = permissions
            .roots()
            .into_iter()
            .map(|scope| scope.path)
            .collect();
        let names = mcp::load(path, &mcp_roots, &mut registry).await?;
        config.visible_tool_names.extend(names);
    }
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
    let (ui_tx, ui_rx) = tokio::sync::mpsc::channel(256);
    let (interaction_port, tui_interactions): (
        Arc<dyn InteractionPort>,
        Option<tokio::sync::mpsc::Receiver<tui::InteractionJob>>,
    ) = if args.tui {
        let (port, receiver) = tui::interaction_pair();
        (Arc::new(port), Some(receiver))
    } else {
        (Arc::new(TerminalInteractionPort), None)
    };
    let interactions = Arc::new(InteractionBroker::new(
        interaction_port,
        config.interaction,
        events.clone(),
    ));
    if args.demo && config.model_route.provider != "mock" {
        return Err(invalid_config(
            "`--demo` needs the mock provider; use a real profile for real runs",
        ));
    }
    let model = build_model(&config, &args.input, args.session, args.demo)?;
    let model = if args.demo {
        model.with_pacing(Duration::from_millis(45))
    } else {
        model
    };
    let model = if args.tui {
        model.with_ui(ui_tx)
    } else {
        model
    };
    let policy: Arc<dyn Policy> = Arc::new(config.clone());
    let options = KernelOptions {
        recall: !args.no_recall,
        memo: !args.no_memo,
        bounded_tools: !args.unbounded_tools,
        capture_context: args.context_snapshots,
    };
    let workspace_root = permissions.root().to_path_buf();
    let roots: Vec<String> = permissions
        .roots()
        .iter()
        .map(|scope| format!("{} [{}]", scope.path.display(), scope.access.as_str()))
        .collect();
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
            todos: todos.clone(),
            options,
        },
    )
    .with_outputs_dir(outputs_dir);

    let driver = if args.no_memo {
        driver
    } else {
        match build_summarizer(&config)? {
            Some(summarizer) => driver.with_summarizer(summarizer),
            None => driver,
        }
    };
    let acceptance_items = match &args.acceptance {
        Some(path) => Some(acceptance::load(path)?),
        None => None,
    };
    let driver = match &acceptance_items {
        Some(items) => driver.with_acceptance(items.clone(), 2),
        None => driver,
    };
    let driver = match restored_session {
        Some(snapshot) => driver.with_session(Session::restore(snapshot)?),
        None => driver,
    };

    if args.tui {
        let interactions = tui_interactions
            .ok_or_else(|| invalid_config("TUI interaction channel was not created"))?;
        let prefill = args.input_set.then(|| args.input.clone());
        return tui::run(tui::TuiHost {
            driver,
            config,
            events,
            todos,
            store,
            session_id: args.session_id,
            workspace: workspace_root,
            roots,
            prefill,
            interactions,
            ui: ui_rx,
        })
        .await;
    }

    if args.session {
        return run_session(SessionHost {
            driver: &driver,
            config: &config,
            events: &events,
            todos: todos.as_ref(),
            turn_input: args.turn_input.as_deref(),
            stop_on_error: args.stop_on_error,
            retry_errors: args.retry_errors,
            acceptance: acceptance_items.as_deref(),
            workspace_root: &workspace_root,
            store: store.as_mut(),
            session_id: &args.session_id,
        })
        .await;
    }

    let outcome = driver.run(&args.input, CancelToken::new()).await;
    print_outcome(&config, &events, &outcome, None);
    print_todos(todos.as_ref());
    if let Some(store) = store.as_mut() {
        save_turn(
            store,
            &args.session_id,
            &driver,
            todos.as_ref(),
            &args.input,
            &outcome,
            &events,
        )?;
    }
    done_or_error(outcome)?;
    Ok(ExitCode::SUCCESS)
}

/// Host services one interactive session needs.
struct SessionHost<'a> {
    driver: &'a TurnDriver<HostModel>,
    config: &'a RuntimeConfig,
    events: &'a Arc<EventLog>,
    todos: Option<&'a Arc<Mutex<TodoList>>>,
    turn_input: Option<&'a std::path::Path>,
    stop_on_error: bool,
    retry_errors: u32,
    acceptance: Option<&'a [AcceptanceItem]>,
    workspace_root: &'a std::path::Path,
    store: Option<&'a mut store::Store>,
    session_id: &'a str,
}

/// Reads inputs from stdin until EOF, keeping one session across turns.
async fn run_session(mut host: SessionHost<'_>) -> Result<ExitCode, KernelError> {
    use std::io::{BufRead, IsTerminal, Write};

    let stdin = std::io::stdin();
    let interactive = stdin.is_terminal();
    let mut line = String::new();
    let mut stopped = false;
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
        // Show the persisted session sequence, not the driver's per-process
        // counter, so a resumed or forked session continues its numbering.
        let session_turn = match host.store.as_ref() {
            Some(store) => Some(store.next_turn_seq(host.session_id)?),
            None => None,
        };
        // A long-run host needs to know what this turn asked for. The file is
        // read by the host.acceptance checker; it is written before the turn runs.
        if let Some(path) = host.turn_input {
            std::fs::write(path, input).map_err(|error| KernelError::Io {
                message: format!("cannot write turn input `{}`: {error}", path.display()),
            })?;
        }
        // The model marks host tasks completed. If the turn fails and its
        // artifact is missing, the list would advance without evidence.
        let task_snapshot = host.todos.map(|list| {
            list.lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        });
        let mut retries = 0u32;
        let mut recovered = false;
        let outcome = loop {
            let outcome = host.driver.run(input, CancelToken::new()).await;
            print_outcome(host.config, host.events, &outcome, session_turn);
            print_todos(host.todos);
            if outcome.is_done() || outcome.is_cancelled() {
                break outcome;
            }
            // A failed turn may already have written the artifact. The
            // checklist is the evidence; re-running the same input after the
            // world moved would write the next chapter for a stale request.
            if recovered_by_checklist(host.acceptance, host.workspace_root).await {
                println!("recovered: acceptance passed after a failed turn");
                recovered = true;
                break outcome;
            }
            revert_tasks(host.todos, task_snapshot.as_ref());
            print_todos(host.todos);
            if retries >= host.retry_errors {
                break outcome;
            }
            retries = retries.saturating_add(1);
            let reason = outcome
                .error
                .as_ref()
                .map(|error| format!("{error}"))
                .unwrap_or_else(|| "turn did not finish".to_owned());
            println!("retry: {retries}/{} after {reason}", host.retry_errors);
        };
        if outcome.is_cancelled() {
            break;
        }
        if let Some(store) = host.store.as_deref_mut() {
            save_turn(
                store,
                host.session_id,
                host.driver,
                host.todos,
                input,
                &outcome,
                host.events,
            )?;
        }
        // The turn is printed and persisted; keeping its events would grow
        // with the session for the life of the process.
        host.events.clear();
        if host.stop_on_error && !outcome.is_done() && !recovered {
            stopped = true;
            break;
        }
    }
    Ok(if stopped {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Restores the pre-turn task list after a failed turn. Task completion is the
/// model's claim; a failed checklist means the evidence is missing.
fn revert_tasks(todos: Option<&Arc<Mutex<TodoList>>>, snapshot: Option<&TodoList>) {
    let (Some(list), Some(snapshot)) = (todos, snapshot) else {
        return;
    };
    *list.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = snapshot.clone();
}

/// Writes the session snapshot, the task list and one transcript row.
fn save_turn(
    store: &mut store::Store,
    session_id: &str,
    driver: &TurnDriver<HostModel>,
    todos: Option<&Arc<Mutex<TodoList>>>,
    input: &str,
    outcome: &TurnOutcome,
    events: &EventLog,
) -> Result<(), KernelError> {
    let todo_snapshot = todos.map(|list| {
        list.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    });
    let usage = format!(
        "{{\"input\":{},\"output\":{},\"cached\":{}}}",
        outcome.usage.input_tokens, outcome.usage.output_tokens, outcome.usage.cached_input_tokens
    );
    // The per-turn answer is structured: body, observed changes, pending items
    // and the verifier report. The body is also in its own column so a reader
    // can list answers without parsing JSON.
    let answer_json = serde_json::json!({
        "revision": outcome.answer.revision,
        "body": &outcome.answer.body,
        "changes": outcome.answer.changes.iter().map(|change| change.path.as_str()).collect::<Vec<_>>(),
        "pending": outcome.answer.pending.iter().map(|item| item.description.as_str()).collect::<Vec<_>>(),
        "verification": outcome.answer.verification.as_ref().map(|report| serde_json::json!({
            "passed": report.passed(),
            "required_failed": &report.required_failed,
            "required_unverified": &report.required_unverified,
        })),
        "notes": &outcome.answer.notes,
        "missing": outcome.answer.missing,
    })
    .to_string();
    let state = outcome.state.to_string();
    let error_text = outcome
        .error
        .as_ref()
        .map(|error| format!("{} ({})", error, error.code().as_str()));
    let event_payloads: Vec<(String, String)> = events
        .for_turn(outcome.turn_id)
        .iter()
        .map(|record| {
            let payload =
                serde_json::to_string(&record.event).map_err(|error| KernelError::Internal {
                    message: format!("cannot encode event: {error}"),
                })?;
            Ok((record.event.name().to_owned(), payload))
        })
        .collect::<Result<Vec<_>, KernelError>>()?;
    let seq = store.next_turn_seq(session_id)?;
    store.save(
        session_id,
        &driver.session_snapshot(),
        todo_snapshot.as_ref(),
        Some(store::StoredTurn {
            seq,
            input,
            state: &state,
            answer: &outcome.answer.body,
            answer_json: &answer_json,
            usage_json: &usage,
            duration_ms: outcome.duration_ms,
            error: error_text.as_deref(),
            context: outcome.context.as_ref(),
            events: &event_payloads,
        }),
    )
}

/// A failed turn that still passes its checklist has done its work. The host
/// continues instead of replaying an input the world has already moved past.
async fn recovered_by_checklist(
    acceptance: Option<&[AcceptanceItem]>,
    workspace_root: &std::path::Path,
) -> bool {
    let Some(items) = acceptance else {
        return false;
    };
    AcceptanceVerifier::new(workspace_root)
        .verify(items)
        .await
        .passed()
}

fn print_todos(todos: Option<&Arc<Mutex<TodoList>>>) {
    let Some(todos) = todos else {
        return;
    };
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
        .current()
        .map(|item| format!("#{}", item.id))
        .unwrap_or_else(|| "-".to_owned());
    println!(
        "tasks: done={} current={} total={}",
        list.completed(),
        current,
        list.items.len()
    );
}

fn print_outcome(
    config: &RuntimeConfig,
    events: &EventLog,
    outcome: &TurnOutcome,
    session_turn: Option<u32>,
) {
    println!("profile: {}", config.profile_name);
    match session_turn {
        Some(turn) => println!("turn: {turn}"),
        None => println!("turn: {}", outcome.turn_id.get()),
    }
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
    if let Some((messages, before, after)) =
        events
            .for_turn(outcome.turn_id)
            .iter()
            .rev()
            .find_map(|record| match &record.event {
                Event::ContextElided {
                    messages,
                    before,
                    after,
                    ..
                } => Some((*messages, *before, *after)),
                _ => None,
            })
    {
        println!("elided: messages={messages} tokens {before}->{after}");
    }
    if let Some((segments, tokens, score_bp, matched, query_terms, known_terms)) = events
        .for_turn(outcome.turn_id)
        .iter()
        .rev()
        .find_map(|record| match &record.event {
            Event::HistoryRecalled {
                segments,
                tokens,
                score_bp,
                matched,
                query_terms,
                known_terms,
                ..
            } => Some((
                segments.clone(),
                *tokens,
                *score_bp,
                *matched,
                *query_terms,
                *known_terms,
            )),
            _ => None,
        })
    {
        println!(
            "recall: score={:.2} hits={}/{} known={} segments={segments:?} tokens={tokens}",
            score_bp as f32 / 10_000.0,
            matched,
            query_terms,
            known_terms
        );
    }
    if let Some((revision, covers, chars, budget, fallback)) = events
        .for_turn(outcome.turn_id)
        .iter()
        .rev()
        .find_map(|record| match &record.event {
            Event::SummaryUpdated {
                revision,
                covers,
                chars,
                budget,
                fallback,
                ..
            } => Some((*revision, covers.len(), *chars, *budget, *fallback)),
            _ => None,
        })
    {
        println!(
            "summary: revision={revision} covers={covers} chars={chars} \
             budget={budget} fallback={fallback}"
        );
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
    let input = outcome.usage.input_tokens;
    let cached = outcome.usage.cached_input_tokens.min(input);
    let hit_rate = if input == 0 {
        0.0
    } else {
        f64::from(cached) * 100.0 / f64::from(input)
    };
    println!(
        "usage: calls={} input={} output={} cached={} hit={:.1}% total={}",
        outcome.model_calls,
        input,
        outcome.usage.output_tokens,
        cached,
        hit_rate,
        input.saturating_add(outcome.usage.output_tokens)
    );
    let mut call_index = 0u32;
    for record in events.for_turn(outcome.turn_id) {
        if let Event::ModelFinished {
            usage, tool_calls, ..
        } = record.event
        {
            call_index = call_index.saturating_add(1);
            let call_hit = if usage.input_tokens == 0 {
                0.0
            } else {
                f64::from(usage.cached_input_tokens.min(usage.input_tokens)) * 100.0
                    / f64::from(usage.input_tokens)
            };
            println!(
                "call: {call_index} input={} cached={} hit={call_hit:.1}% output={} tools={tool_calls}",
                usage.input_tokens, usage.cached_input_tokens, usage.output_tokens
            );
        }
    }
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
    demo: bool,
) -> Result<HostModel, KernelError> {
    match config.model_route.provider.as_str() {
        "mock" => {
            if demo {
                return Ok(HostModel::Demo(DemoModel::new()));
            }
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
    /// True when the input came from `--input` or a job, not the default.
    input_set: bool,
    workspace: PathBuf,
    /// Extra directories the agent may use, as `(path, access)`.
    roots: Vec<(PathBuf, AccessKind)>,
    acceptance: Option<PathBuf>,
    session: bool,
    max_model_calls: Option<u32>,
    todo_file: Option<PathBuf>,
    no_todo: bool,
    no_recall: bool,
    no_memo: bool,
    unbounded_tools: bool,
    /// Debug/eval aid: write the current turn input here before it runs.
    turn_input: Option<PathBuf>,
    /// Long-run host behavior: stop instead of advancing after an error.
    stop_on_error: bool,
    /// Re-run the same input after a failed turn, at most this many times.
    retry_errors: u32,
    /// Host-owned turn deadline override; the policy default is 180 s.
    turn_timeout_ms: Option<u64>,
    /// Host-owned per-tool timeout override; the policy default is 30 s.
    tool_timeout_ms: Option<u64>,
    /// SQLite file for session persistence.
    store: Option<PathBuf>,
    /// Session id inside the store.
    session_id: String,
    /// Keep the turn-start message list in the store.
    context_snapshots: bool,
    /// Plugin file listing MCP servers to load.
    plugins: Option<PathBuf>,
    /// Composition bundle: profile, optional plugins and optional state dir.
    bundle: Option<PathBuf>,
    /// Directory for the session store and the trash. When set, the store
    /// defaults to `<state>/keel.db` and the trash to `<state>/trash`.
    state_dir: Option<PathBuf>,
    /// Probe every configured MCP server and exit.
    plugins_check: bool,
    /// Copy a session checkpoint into `--session-id` before running.
    fork_from: Option<String>,
    /// Delete a session and its derived rows, then exit.
    delete_session: Option<String>,
    /// Keep at most this many branch checkpoints per session. The store keeps
    /// 20 by default; 0 keeps all.
    keep_checkpoints: Option<u32>,
    /// Run the interactive terminal UI.
    tui: bool,
    /// Mock-only demo: a slow tool call and a paced answer for the TUI.
    demo: bool,
    /// Machine-readable output for `--plugins-check`.
    json: bool,
    /// Print a composed frontend view (`sessions`, `session`, `turn` or
    /// `memory`) and exit.
    view: Option<String>,
    /// Turn sequence for `--view turn`.
    turn: Option<u32>,
    /// Turn page size for `--view session`; 0 returns every turn.
    limit: u32,
    /// Exclusive turn cursor for `--view session`.
    before: Option<u32>,
}

fn parse_args() -> Result<Args, KernelError> {
    let mut profile = PathBuf::from("variants/coding/profile.toml");
    let mut input = String::from("hello from keel");
    let mut acceptance = None;
    let mut session = false;
    let mut max_model_calls = None;
    let mut todo_file = None;
    let mut no_todo = false;
    let mut no_recall = false;
    let mut no_memo = false;
    let mut unbounded_tools = false;
    let mut turn_input = None;
    let mut stop_on_error = false;
    let mut retry_errors = 0u32;
    let mut turn_timeout_ms = None;
    let mut tool_timeout_ms = None;
    let mut store = None;
    let mut session_id = "default".to_owned();
    let mut context_snapshots = false;
    let mut plugins = None;
    let mut bundle = None;
    let mut state_dir = None;
    let mut job_path: Option<PathBuf> = None;
    let mut profile_set = false;
    let mut plugins_set = false;
    let mut input_set = false;
    let mut workspace_set = false;
    let mut session_id_set = false;
    let mut plugins_check = false;
    let mut fork_from = None;
    let mut delete_session = None;
    let mut keep_checkpoints = None;
    let mut tui = false;
    let mut demo = false;
    let mut json = false;
    let mut view = None;
    let mut turn = None;
    let mut limit = 0u32;
    let mut before = None;
    let mut workspace = std::env::current_dir().map_err(|error| KernelError::Io {
        message: error.to_string(),
    })?;
    let mut roots = Vec::new();
    let mut args = std::env::args().skip(1);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--profile" => {
                profile = PathBuf::from(
                    args.next()
                        .ok_or_else(|| invalid_config("missing value for `--profile`"))?,
                );
                profile_set = true;
            }
            "--input" => {
                input = args
                    .next()
                    .ok_or_else(|| invalid_config("missing value for `--input`"))?;
                input_set = true;
            }
            "--workspace" => {
                workspace = PathBuf::from(
                    args.next()
                        .ok_or_else(|| invalid_config("missing value for `--workspace`"))?,
                );
                workspace_set = true;
            }
            "--bundle" => {
                bundle =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        invalid_config("missing value for `--bundle`")
                    })?));
            }
            "--state-dir" => {
                state_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        invalid_config("missing value for `--state-dir`")
                    })?));
            }
            "--job" => {
                job_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        invalid_config("missing value for `--job`")
                    })?));
            }
            "--root" => {
                let spec = args
                    .next()
                    .ok_or_else(|| invalid_config("missing value for `--root`"))?;
                roots.push(parse_root_spec(&spec)?);
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
            "--no-todo" => {
                no_todo = true;
            }
            "--no-recall" => {
                no_recall = true;
            }
            "--no-memo" => {
                no_memo = true;
            }
            "--unbounded-tools" => {
                unbounded_tools = true;
            }
            "--tui" => {
                tui = true;
            }
            "--demo" => {
                demo = true;
            }
            "--turn-input" => {
                turn_input =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        invalid_config("missing value for `--turn-input`")
                    })?));
            }
            "--stop-on-error" => {
                stop_on_error = true;
            }
            "--retry-errors" => {
                let value = args
                    .next()
                    .ok_or_else(|| invalid_config("missing value for `--retry-errors`"))?;
                retry_errors = value.parse::<u32>().map_err(|_| {
                    invalid_config("`--retry-errors` must be a non-negative integer")
                })?;
            }
            "--store" => {
                store =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        invalid_config("missing value for `--store`")
                    })?));
            }
            "--session-id" => {
                session_id = args
                    .next()
                    .ok_or_else(|| invalid_config("missing value for `--session-id`"))?;
                session_id_set = true;
            }
            "--context-snapshots" => {
                context_snapshots = true;
            }
            "--plugins" => {
                plugins =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        invalid_config("missing value for `--plugins`")
                    })?));
                plugins_set = true;
            }
            "--plugins-check" => {
                plugins_check = true;
            }
            "--json" => {
                json = true;
            }
            "--view" => {
                view = Some(
                    args.next()
                        .ok_or_else(|| invalid_config("missing value for `--view`"))?,
                );
            }
            "--turn" => {
                let value = args
                    .next()
                    .ok_or_else(|| invalid_config("missing value for `--turn`"))?;
                let seq = value
                    .parse::<u32>()
                    .map_err(|_| invalid_config("`--turn` must be a positive integer"))?;
                if seq == 0 {
                    return Err(invalid_config("`--turn` must be a positive integer"));
                }
                turn = Some(seq);
            }
            "--limit" => {
                let value = args
                    .next()
                    .ok_or_else(|| invalid_config("missing value for `--limit`"))?;
                limit = value
                    .parse::<u32>()
                    .map_err(|_| invalid_config("`--limit` must be a non-negative integer"))?;
            }
            "--before" => {
                let value = args
                    .next()
                    .ok_or_else(|| invalid_config("missing value for `--before`"))?;
                let seq = value
                    .parse::<u32>()
                    .map_err(|_| invalid_config("`--before` must be a positive integer"))?;
                if seq == 0 {
                    return Err(invalid_config("`--before` must be a positive integer"));
                }
                before = Some(seq);
            }
            "--fork-from" => {
                fork_from = Some(
                    args.next()
                        .ok_or_else(|| invalid_config("missing value for `--fork-from`"))?,
                );
            }
            "--delete-session" => {
                delete_session = Some(
                    args.next()
                        .ok_or_else(|| invalid_config("missing value for `--delete-session`"))?,
                );
            }
            "--keep-checkpoints" => {
                let value = args
                    .next()
                    .ok_or_else(|| invalid_config("missing value for `--keep-checkpoints`"))?;
                keep_checkpoints = Some(value.parse::<u32>().map_err(|_| {
                    invalid_config("`--keep-checkpoints` must be a non-negative integer")
                })?);
            }
            "--turn-timeout-ms" => {
                let value = args
                    .next()
                    .ok_or_else(|| invalid_config("missing value for `--turn-timeout-ms`"))?;
                let ms = value.parse::<u64>().map_err(|_| {
                    invalid_config("`--turn-timeout-ms` must be a positive integer")
                })?;
                if ms == 0 {
                    return Err(invalid_config(
                        "`--turn-timeout-ms` must be a positive integer",
                    ));
                }
                turn_timeout_ms = Some(ms);
            }
            "--tool-timeout-ms" => {
                let value = args
                    .next()
                    .ok_or_else(|| invalid_config("missing value for `--tool-timeout-ms`"))?;
                let ms = value.parse::<u64>().map_err(|_| {
                    invalid_config("`--tool-timeout-ms` must be a positive integer")
                })?;
                if ms == 0 {
                    return Err(invalid_config(
                        "`--tool-timeout-ms` must be a positive integer",
                    ));
                }
                tool_timeout_ms = Some(ms);
            }
            other => {
                return Err(invalid_config(format!("unknown argument `{other}`")));
            }
        }
    }
    if bundle.is_some() && profile_set {
        return Err(invalid_config(
            "`--bundle` and `--profile` are mutually exclusive",
        ));
    }
    if bundle.is_some() && plugins_set {
        return Err(invalid_config(
            "`--bundle` and `--plugins` are mutually exclusive",
        ));
    }

    if let Some(path) = &job_path {
        let job = job::Job::load(path)?;
        // Explicit CLI composition wins as a whole; a job fills it only when
        // the CLI said nothing about the profile, plugins or bundle.
        if bundle.is_none() && !profile_set && !plugins_set {
            if let Some(job_bundle) = job.bundle {
                bundle = Some(job_bundle);
            } else if let Some(job_profile) = job.profile {
                profile = job_profile;
            }
            if plugins.is_none() {
                plugins = job.plugins;
            }
        }
        if !workspace_set {
            if let Some(job_workspace) = job.workspace {
                workspace = job_workspace;
            }
        }
        if state_dir.is_none() {
            state_dir = job.state_dir;
        }
        if store.is_none() {
            store = job.store;
        }
        if !session_id_set {
            if let Some(id) = job.session_id {
                session_id = id;
            }
        }
        if !input_set {
            if let Some(job_input) = job.input {
                input = job_input;
                input_set = true;
            }
        }
        roots.extend(job.roots);
    }

    Ok(Args {
        profile,
        input,
        input_set,
        workspace,
        roots,
        acceptance,
        session,
        max_model_calls,
        todo_file,
        no_todo,
        no_recall,
        no_memo,
        unbounded_tools,
        turn_input,
        stop_on_error,
        retry_errors,
        turn_timeout_ms,
        tool_timeout_ms,
        store,
        session_id,
        context_snapshots,
        plugins,
        bundle,
        state_dir,
        plugins_check,
        fork_from,
        delete_session,
        keep_checkpoints,
        tui,
        demo,
        json,
        view,
        turn,
        limit,
        before,
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

/// Prints one composed view as JSON for a frontend or a golden test.
fn print_json<T: serde::Serialize + ?Sized>(value: &T) -> Result<(), KernelError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| invalid_config(format!("cannot encode view: {error}")))?;
    println!("{text}");
    Ok(())
}

/// Parses `<session>:<turn>` from `--fork-from`.
/// `--store` wins; otherwise a state directory supplies `<state>/keel.db`.
fn resolve_store_path(store: &Option<PathBuf>, state_dir: &Option<PathBuf>) -> Option<PathBuf> {
    store
        .clone()
        .or_else(|| state_dir.as_ref().map(|dir| dir.join("keel.db")))
}

/// Parses `--root <path>[:ro|:rw]`. The suffix is checked last, so a Windows
/// drive letter is never mistaken for it.
fn parse_root_spec(spec: &str) -> Result<(PathBuf, AccessKind), KernelError> {
    let (path, access) = if let Some(path) = spec.strip_suffix(":ro") {
        (path, AccessKind::Read)
    } else if let Some(path) = spec.strip_suffix(":rw") {
        (path, AccessKind::Write)
    } else {
        (spec, AccessKind::Write)
    };
    if path.is_empty() {
        return Err(invalid_config("`--root` needs a path"));
    }
    Ok((PathBuf::from(path), access))
}

fn parse_fork(value: &str) -> Result<(String, u32), KernelError> {
    let (source, turn) = value
        .rsplit_once(':')
        .ok_or_else(|| invalid_config("`--fork-from` expects `<session>:<turn>`"))?;
    let turn = turn
        .parse::<u32>()
        .map_err(|_| invalid_config("`--fork-from` needs a positive turn number"))?;
    if source.is_empty() || turn == 0 {
        return Err(invalid_config("`--fork-from` expects `<session>:<turn>`"));
    }
    Ok((source.to_owned(), turn))
}

/// Probes every configured MCP server and reports one line per server.
async fn check_plugins(
    path: &std::path::Path,
    roots: &[PathBuf],
    json: bool,
) -> Result<ExitCode, KernelError> {
    let statuses = mcp::check(path, roots).await?;
    if json {
        let rendered = serde_json::to_string_pretty(&statuses)
            .map_err(|error| invalid_config(format!("cannot encode plugin status: {error}")))?;
        println!("{rendered}");
    } else {
        let mut failed = 0;
        for status in &statuses {
            match &status.error {
                Some(error) => {
                    failed += 1;
                    println!(
                        "server {}: failed after {} ms: {error}",
                        status.name, status.duration_ms
                    );
                }
                None => println!(
                    "server {}: ready, {} tools, {} ms",
                    status.name, status.tools, status.duration_ms
                ),
            }
        }
        println!(
            "{}/{} servers ready",
            statuses.len() - failed,
            statuses.len()
        );
    }
    let failed = statuses
        .iter()
        .filter(|status| status.error.is_some())
        .count();
    Ok(if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use keel_core::VerifySpec;

    #[test]
    fn a_state_directory_supplies_the_default_store() {
        let state = Some(PathBuf::from("state"));
        assert_eq!(
            resolve_store_path(&None, &state),
            Some(PathBuf::from("state").join("keel.db"))
        );
        assert_eq!(
            resolve_store_path(&Some(PathBuf::from("custom.db")), &state),
            Some(PathBuf::from("custom.db"))
        );
        assert_eq!(resolve_store_path(&None, &None), None);
    }

    #[test]
    fn root_specs_parse_read_and_write_suffixes() {
        assert_eq!(
            parse_root_spec("D:/Archive:ro").unwrap(),
            (PathBuf::from("D:/Archive"), AccessKind::Read)
        );
        assert_eq!(
            parse_root_spec("D:/Archive:rw").unwrap(),
            (PathBuf::from("D:/Archive"), AccessKind::Write)
        );
        assert_eq!(
            parse_root_spec("D:/Archive").unwrap(),
            (PathBuf::from("D:/Archive"), AccessKind::Write)
        );
        assert_eq!(
            parse_root_spec(r"C:\Archive:ro").unwrap(),
            (PathBuf::from(r"C:\Archive"), AccessKind::Read)
        );
        assert!(parse_root_spec(":ro").is_err());
    }

    fn compute_item(id: &str, expected: &str) -> AcceptanceItem {
        AcceptanceItem {
            id: id.to_owned(),
            description: "arithmetic check".to_owned(),
            required: true,
            verify: VerifySpec::Compute {
                expression: "2+2".to_owned(),
                expected: expected.to_owned(),
            },
        }
    }

    #[tokio::test]
    async fn a_failed_turn_recovers_when_the_checklist_passes() {
        let items = vec![compute_item("four", "4")];
        assert!(recovered_by_checklist(Some(&items), std::path::Path::new(".")).await);
    }

    #[tokio::test]
    async fn a_failed_turn_does_not_recover_when_the_checklist_fails() {
        let items = vec![compute_item("five", "5")];
        assert!(!recovered_by_checklist(Some(&items), std::path::Path::new(".")).await);
    }

    #[tokio::test]
    async fn a_failed_turn_without_a_checklist_does_not_recover() {
        assert!(!recovered_by_checklist(None, std::path::Path::new(".")).await);
    }

    #[test]
    fn a_fork_argument_is_session_and_turn() {
        assert_eq!(parse_fork("s:3").unwrap(), ("s".to_owned(), 3));
        assert_eq!(parse_fork("a:b:2").unwrap(), ("a:b".to_owned(), 2));
        assert!(parse_fork("s").is_err());
        assert!(parse_fork("s:0").is_err());
        assert!(parse_fork(":2").is_err());
    }

    #[test]
    fn text_chunks_split_on_char_boundaries() {
        assert_eq!(text_chunks("abcdef", 2), vec!["ab", "cd", "ef"]);
        assert_eq!(text_chunks("中文测试", 2), vec!["中文", "测试"]);
        assert!(text_chunks("", 2).is_empty());
        assert_eq!(text_chunks("abc", 8), vec!["abc"]);
    }

    #[test]
    fn the_demo_script_runs_a_sleeping_shell_tool() {
        match demo_tool_step() {
            MockStep::TextAndToolCall {
                name, args, text, ..
            } => {
                assert_eq!(name, "shell");
                assert!(!text.is_empty());
                assert!(matches!(
                    args.get("program"),
                    Some(ArgValue::String(program)) if program == "python"
                ));
                assert!(matches!(
                    args.get("args"),
                    Some(ArgValue::StringList(items))
                        if items.iter().any(|item| {
                            item.contains("sleep(2)") && item.contains("check passed")
                        })
                ));
            }
            other => panic!("unexpected demo step: {other:?}"),
        }
    }

    #[test]
    fn a_failed_turn_restores_the_task_list() {
        let mut list = TodoList::default();
        let first = list.add_host("one").unwrap();
        list.add_host("two").unwrap();
        let snapshot = list.clone();
        list.update_status(first, keel_core::TodoStatus::Completed)
            .unwrap();
        assert_eq!(list.completed(), 1);

        let shared = Arc::new(Mutex::new(list));
        revert_tasks(Some(&shared), Some(&snapshot));
        let list = shared.lock().unwrap();
        assert_eq!(list.completed(), 0);
        assert_eq!(list.current().map(|item| item.id), Some(first));
    }
}
