//! Rehearse a long task against the mock provider: sixty file edits in one
//! turn, one early stop, no user input.
//!
//! ```bash
//! cargo run -p keel-runtime --example long_task
//! ```

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use keel_core::{
    ArgValue, CancelToken, Event, ModelError, ModelProvider, ModelRequest, ModelStream, Policy,
    TodoList, TodoStatus, ToolArgs,
};
use keel_runtime::{
    from_profile_str, ContextBuilder, DeferAllInteractionPort, EventLog, HistoryStage,
    InteractionBroker, KernelOptions, LatestStage, LocalTrashStore, MockModel, MockStep,
    PermissionEngine, SystemStage, ToolRegistry, TrashStore, TurnDriver, TurnDriverConfig,
    WriteFileTool,
};

const PROFILE: &str = r#"
[profile]
name = "long-task"

[model]
provider = "mock"
model = "mock"
thinking_budget = 0
max_output_tokens = 256

[context]
stages = ["system", "latest"]
budget_tokens = 8192

[tools]
visible = ["write_file"]
budget = 1000

[verification]
mode = "none"

[permission]
level = "workspace_auto"

[persona]
system = "prompt.md"
"#;

const MODULES: usize = 60;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("prompt.md"),
        "You are the long-task harness.",
    )?;
    let config = from_profile_str(PROFILE, dir.path())?;

    // One write per module, one early report halfway, one final answer.
    let mut steps = Vec::new();
    for index in 0..MODULES {
        let mut args = ToolArgs::new();
        args.insert("path", ArgValue::String(format!("svc_{index:02}.py")));
        args.insert("content", ArgValue::String("LIMIT = 25\n".to_owned()));
        steps.push(MockStep::ToolCall {
            name: "write_file".to_owned(),
            args,
        });
        if index + 1 == MODULES / 2 {
            steps.push(MockStep::Text(format!(
                "{} modules are done; continuing with the rest.",
                index + 1
            )));
        }
    }
    steps.push(MockStep::Text("all modules updated".to_owned()));

    let todos = Arc::new(Mutex::new(TodoList::new()));
    {
        let mut list = todos
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        list.add_model("update every service module")
            .map_err(|error| error.to_string())?;
    }
    let events = Arc::new(EventLog::new());
    let total_steps = steps.len();
    let model = LongModel {
        inner: MockModel::new(steps),
        todos: todos.clone(),
        calls: AtomicUsize::new(0),
        total_steps,
    };

    let mut registry = ToolRegistry::new();
    let trash: Arc<dyn TrashStore> = Arc::new(LocalTrashStore::new(dir.path().to_path_buf()));
    registry.register(Arc::new(WriteFileTool::new(trash)))?;
    let context = ContextBuilder::new(vec![
        Arc::new(SystemStage::new()),
        Arc::new(LatestStage::new()),
        Arc::new(HistoryStage::new()),
    ]);
    let permissions = Arc::new(PermissionEngine::new(dir.path(), config.permission_level)?);
    let interactions = Arc::new(InteractionBroker::new(
        Arc::new(DeferAllInteractionPort),
        config.interaction,
        events.clone(),
    ));
    let policy: Arc<dyn Policy> = Arc::new(config.clone());
    let driver = TurnDriver::new(
        model,
        Arc::new(registry),
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
            todos: Some(todos),
            options: KernelOptions::default(),
        },
    );

    let outcome = driver.run("update every module", CancelToken::new()).await;

    let written = std::fs::read_dir(dir.path())?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("svc_"))
        })
        .filter(|entry| {
            std::fs::read_to_string(entry.path()).is_ok_and(|content| content == "LIMIT = 25\n")
        })
        .count();
    let tool_calls = events
        .for_turn(outcome.turn_id)
        .iter()
        .filter(|record| matches!(record.event, Event::ToolStarted { .. }))
        .count();
    let continuations = events
        .for_turn(outcome.turn_id)
        .iter()
        .filter(|record| matches!(record.event, Event::TurnContinued { .. }))
        .count();
    let exhausted = events
        .for_turn(outcome.turn_id)
        .iter()
        .any(|record| matches!(record.event, Event::TurnBudgetExhausted { .. }));

    println!("state: {}", outcome.state);
    println!("answer: {}", outcome.answer.body);
    println!("model calls: {}", outcome.model_calls);
    println!("tool calls: {tool_calls}");
    println!("continuations: {continuations}");
    println!("files written: {written}/{MODULES}");
    println!("duration ms: {}", outcome.duration_ms);
    println!(
        "usage: input={} output={}",
        outcome.usage.input_tokens, outcome.usage.output_tokens
    );

    let ok = outcome.is_done()
        && outcome.answer.body == "all modules updated"
        && written == MODULES
        && tool_calls == MODULES
        && continuations == 1
        && !exhausted;
    if ok {
        println!("\nlong task: one turn, no user input, every module updated");
        Ok(())
    } else {
        Err("long task did not complete as expected".into())
    }
}

struct LongModel {
    inner: MockModel,
    todos: Arc<Mutex<TodoList>>,
    calls: AtomicUsize,
    total_steps: usize,
}

#[async_trait]
impl ModelProvider for LongModel {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call + 1 == self.total_steps {
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
