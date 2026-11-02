//! Verify the kernel's runtime behaviors with the mock provider.
//!
//! No network and no API key: every scenario drives the real driver,
//! dispatcher, permission engine and command runner with a scripted model.
//!
//! ```bash
//! cargo run -p keel-runtime --example behaviors
//! ```

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    AccessKind, ArgValue, CancelToken, Event, ModelError, ModelProvider, ModelRequest, ModelStream,
    PermissionDecision, PermissionGate, PermissionLevel, PermissionRequest, Policy, RiskLevel,
    TodoList, TodoStatus, Tool, ToolArgs, ToolCallId, ToolCallRequest, ToolCtx, ToolError,
    ToolResult, ToolSchema, TurnId,
};
use keel_runtime::{
    command, from_profile_str, ContextBuilder, DeferAllInteractionPort, EchoTool, EventLog,
    HistoryStage, InteractionBroker, KernelOptions, LatestStage, LocalTrashStore, MockModel,
    MockStep, PermissionEngine, ReadFileTool, ShellTool, SystemStage, ToolDispatcher, ToolRegistry,
    TrashStore, TurnDriver, TurnDriverConfig, TurnLimits, WriteFileTool,
};

const PROFILE: &str = r#"
[profile]
name = "behaviors"

[model]
provider = "mock"
model = "mock"
thinking_budget = 0
max_output_tokens = 256

[context]
stages = ["system", "latest"]
budget_tokens = 4096

[tools]
visible = ["echo", "read_file", "write_file", "shell"]
budget = 100

[verification]
mode = "none"

[permission]
level = "workspace_auto"

[persona]
system = "prompt.md"
"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("prompt.md"),
        "You are the behaviors harness.",
    )?;
    let config = from_profile_str(PROFILE, dir.path())?;

    let mut checker = Checker::default();
    continuation(&config, dir.path(), &mut checker).await?;
    token_budget(&config, dir.path(), &mut checker).await?;
    command_activity(dir.path(), &mut checker).await?;
    permission_matrix(dir.path(), &mut checker)?;
    read_truncation(&config, dir.path(), &mut checker).await?;
    shell_marker(&mut checker)?;
    parallel_reads(dir.path(), &mut checker).await?;

    println!("\n{} passed, {} failed", checker.passed, checker.failed);
    if checker.failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

#[derive(Default)]
struct Checker {
    passed: u32,
    failed: u32,
}

impl Checker {
    fn check(&mut self, name: &str, ok: bool, detail: impl std::fmt::Display) {
        if ok {
            self.passed += 1;
            println!("[ok]   {name}");
        } else {
            self.failed += 1;
            println!("[FAIL] {name} — {detail}");
        }
    }
}

/// A model that clears the shared task list on its second call.
struct CompletingModel {
    inner: MockModel,
    todos: Arc<Mutex<TodoList>>,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for CompletingModel {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
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

/// Records the tool count and whether any message mentions a token budget.
struct RecordingModel {
    inner: MockModel,
    seen: Arc<Mutex<Vec<(usize, bool)>>>,
}

#[async_trait]
impl ModelProvider for RecordingModel {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        let mentions_token = request
            .messages
            .iter()
            .any(|message| message.content.to_lowercase().contains("token"));
        self.seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((request.tools.len(), mentions_token));
        self.inner.stream(request, cancel).await
    }
}

async fn continuation(
    config: &keel_runtime::RuntimeConfig,
    root: &Path,
    checker: &mut Checker,
) -> Result<(), Box<dyn std::error::Error>> {
    let todos = Arc::new(Mutex::new(TodoList::new()));
    {
        let mut list = todos
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        list.add_model("inspect the workspace")
            .map_err(|error| error.to_string())?;
        list.add_model("run the tests")
            .map_err(|error| error.to_string())?;
    }
    let events = Arc::new(EventLog::new());
    let model = CompletingModel {
        inner: MockModel::new(vec![
            MockStep::Text("I will start with the first item.".to_owned()),
            MockStep::Text("all items are done".to_owned()),
        ]),
        todos: todos.clone(),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let driver = build(
        model,
        config,
        root,
        events.clone(),
        Some(todos),
        config.limits(),
    )?;
    let outcome = driver.run("do both items", CancelToken::new()).await;
    let continued = events
        .for_turn(outcome.turn_id)
        .iter()
        .any(|record| matches!(record.event, Event::TurnContinued { .. }));
    checker.check(
        "a partial report continues while the task list is open",
        outcome.is_done() && outcome.answer.body == "all items are done" && continued,
        format!(
            "state={} answer={:?} continued={continued}",
            outcome.state, outcome.answer.body
        ),
    );
    Ok(())
}

async fn token_budget(
    config: &keel_runtime::RuntimeConfig,
    root: &Path,
    checker: &mut Checker,
) -> Result<(), Box<dyn std::error::Error>> {
    let events = Arc::new(EventLog::new());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hi".to_owned()));
    let model = RecordingModel {
        inner: MockModel::new(vec![
            MockStep::ToolCall {
                name: "echo".to_owned(),
                args,
            },
            MockStep::Text("final answer".to_owned()),
        ]),
        seen: seen.clone(),
    };
    let mut limits = config.limits();
    // Any call spends at least one input token.
    limits.max_turn_input_tokens = 1;
    let driver = build(model, config, root, events.clone(), None, limits)?;
    let outcome = driver.run("do it", CancelToken::new()).await;
    let exhausted = events
        .for_turn(outcome.turn_id)
        .iter()
        .any(|record| matches!(record.event, Event::TurnBudgetExhausted { .. }));
    let requests = seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let final_call = requests.get(1);
    let ok = outcome.is_done()
        && outcome.answer.body == "final answer"
        && exhausted
        && final_call.is_some_and(|(tools, mentions)| *tools == 0 && !*mentions);
    checker.check(
        "the token budget forces the answer call without a number",
        ok,
        format!(
            "exhausted={exhausted} requests={:?} answer={:?}",
            *requests, outcome.answer.body
        ),
    );
    Ok(())
}

async fn command_activity(
    root: &Path,
    checker: &mut Checker,
) -> Result<(), Box<dyn std::error::Error>> {
    if !python_available() {
        println!("[skip] command activity — python is not installed");
        return Ok(());
    }
    let printing = [
        "-c".to_owned(),
        "import time\nfor _ in range(10):\n    print('tick', flush=True)\n    time.sleep(0.1)"
            .to_owned(),
    ];
    let started = std::time::Instant::now();
    let output = command::run(
        command::CommandSpec {
            program: "python",
            args: &printing,
            cwd: root,
            timeout: Duration::from_millis(400),
        },
        &CancelToken::new(),
    )
    .await;
    checker.check(
        "output keeps a long command alive",
        output
            .as_ref()
            .is_ok_and(|output| output.exit_code == Some(0))
            && started.elapsed() > Duration::from_millis(500),
        format!(
            "elapsed={:?} result={:?}",
            started.elapsed(),
            output.map(|o| o.exit_code)
        ),
    );

    let silent = ["-c".to_owned(), "import time; time.sleep(2)".to_owned()];
    let started = std::time::Instant::now();
    let output = command::run(
        command::CommandSpec {
            program: "python",
            args: &silent,
            cwd: root,
            timeout: Duration::from_millis(300),
        },
        &CancelToken::new(),
    )
    .await;
    checker.check(
        "a silent command is stopped by the inactivity window",
        output.is_err() && started.elapsed() < Duration::from_secs(1),
        format!(
            "elapsed={:?} result={:?}",
            started.elapsed(),
            output.map(|o| o.exit_code)
        ),
    );
    Ok(())
}

fn permission_matrix(root: &Path, checker: &mut Checker) -> Result<(), Box<dyn std::error::Error>> {
    let engine = PermissionEngine::new(root, PermissionLevel::WorkspaceAuto)?;
    let request = |access: AccessKind, path: &Path| PermissionRequest {
        tool: "harness".to_owned(),
        access,
        path: Some(path.to_path_buf()),
        detail: None,
    };

    let template = engine.check(&request(AccessKind::Write, &root.join(".env.example")));
    checker.check(
        ".env.example is an ordinary file",
        template == PermissionDecision::Allow,
        format!("{template:?}"),
    );

    let secret = engine.check(&request(AccessKind::Read, &root.join(".env")));
    checker.check(
        "a credential inside the workspace asks",
        matches!(secret, PermissionDecision::Confirm { .. }),
        format!("{secret:?}"),
    );

    let outside = root.join("..").join("id_rsa");
    let outside = engine.check(&request(AccessKind::Read, &outside));
    checker.check(
        "a credential outside every root is refused",
        matches!(outside, PermissionDecision::Deny { .. }),
        format!("{outside:?}"),
    );

    let state = engine.check(&request(
        AccessKind::Read,
        &root.join(".keel").join("trash").join("item"),
    ));
    checker.check(
        "the agent's own state stays denied",
        matches!(state, PermissionDecision::Deny { .. }),
        format!("{state:?}"),
    );
    Ok(())
}

async fn read_truncation(
    config: &keel_runtime::RuntimeConfig,
    root: &Path,
    checker: &mut Checker,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = config;
    let path = root.join("huge.log");
    let file = std::fs::File::create(&path)?;
    file.set_len(9 * 1024 * 1024)?;
    drop(file);
    let mut args = ToolArgs::new();
    args.insert("path", ArgValue::String("huge.log".to_owned()));
    let result = ReadFileTool::new().call(args, &tool_ctx(root)).await;
    let content = result.content().unwrap_or_default();
    checker.check(
        "a file over 8 MiB is truncated, not refused",
        content.contains("[truncated:"),
        format!("error={:?}", result.error().map(|error| error.code)),
    );
    Ok(())
}

fn shell_marker(checker: &mut Checker) -> Result<(), Box<dyn std::error::Error>> {
    let tool = ShellTool::coding_defaults();
    let mut args = ToolArgs::new();
    args.insert("program", ArgValue::String("docker".to_owned()));
    let request = tool
        .permission_requests(&args)
        .map_err(|error| error.to_string())?;
    let detail = request
        .first()
        .and_then(|request| request.detail.clone())
        .unwrap_or_default();
    checker.check(
        "an unknown program is confirmed, not rejected",
        detail.contains("not in the standard program list"),
        detail,
    );
    Ok(())
}

async fn parallel_reads(
    root: &Path,
    checker: &mut Checker,
) -> Result<(), Box<dyn std::error::Error>> {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ProbeTool {
        active: active.clone(),
        peak: peak.clone(),
    }))?;
    let events = Arc::new(EventLog::new());
    let permissions = Arc::new(PermissionEngine::new(root, PermissionLevel::FullAuto)?);
    let dispatcher = ToolDispatcher::new(
        Arc::new(registry),
        events,
        permissions,
        Duration::from_secs(5),
    );
    let calls: Vec<ToolCallRequest> = (0..20)
        .map(|id| ToolCallRequest {
            id: ToolCallId::new(id),
            name: "probe".to_owned(),
            args: ToolArgs::new(),
        })
        .collect();
    let visible: BTreeSet<String> = ["probe".to_owned()].into();
    let results = dispatcher
        .dispatch(TurnId::new(1), calls, &visible, &CancelToken::new())
        .await;
    checker.check(
        "twenty reads run at most eight at a time and all return",
        results.len() == 20
            && results.iter().all(ToolResult::is_success)
            && peak.load(Ordering::SeqCst) <= 8,
        format!(
            "results={} peak={}",
            results.len(),
            peak.load(Ordering::SeqCst)
        ),
    );
    Ok(())
}

struct ProbeTool {
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for ProbeTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "probe".to_owned(),
            description: "records how many reads run at once".to_owned(),
            risk: RiskLevel::ReadOnly,
            params: Vec::new(),
        }
    }

    fn permission_requests(&self, _args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        Ok(Vec::new())
    }

    async fn call(&self, _args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(20)).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        ToolResult::success(ctx.call_id, "probe", "ok")
    }
}

fn tool_ctx(root: &Path) -> ToolCtx {
    ToolCtx {
        turn_id: TurnId::new(1),
        call_id: ToolCallId::new(1),
        workspace_root: root.to_path_buf(),
        cancel: CancelToken::new(),
    }
}

fn python_available() -> bool {
    std::process::Command::new("python")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
fn build<M: ModelProvider>(
    model: M,
    config: &keel_runtime::RuntimeConfig,
    root: &Path,
    events: Arc<EventLog>,
    todos: Option<Arc<Mutex<TodoList>>>,
    limits: TurnLimits,
) -> Result<TurnDriver<M>, Box<dyn std::error::Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool::new()))?;
    registry.register(Arc::new(ReadFileTool::new()))?;
    let trash: Arc<dyn TrashStore> = Arc::new(LocalTrashStore::new(root.to_path_buf()));
    registry.register(Arc::new(WriteFileTool::new(trash)))?;
    registry.register(Arc::new(ShellTool::coding_defaults()))?;
    let registry = Arc::new(registry);

    let context = ContextBuilder::new(vec![
        Arc::new(SystemStage::new()),
        Arc::new(HistoryStage::new()),
        Arc::new(LatestStage::new()),
    ]);
    let permissions = Arc::new(PermissionEngine::new(root, config.permission_level)?);
    let interactions = Arc::new(InteractionBroker::new(
        Arc::new(DeferAllInteractionPort),
        config.interaction,
        events.clone(),
    ));
    let policy: Arc<dyn Policy> = Arc::new(config.clone());
    Ok(TurnDriver::new(
        model,
        registry,
        context,
        policy,
        TurnDriverConfig {
            events,
            permissions,
            interactions,
            system_prompt: config.system_prompt.clone(),
            max_output_tokens: config.max_output_tokens,
            context_window: config.context_window,
            limits,
            todos,
            options: KernelOptions::default(),
        },
    ))
}
