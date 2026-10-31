mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    ArgValue, CancelToken, ErrorCode, GrantScope, InteractionPolicy, InteractionResponse,
    ParamSpec, PermissionLevel, PermissionRequest, RiskLevel, Tool, ToolArgs, ToolCallId,
    ToolCallRequest, ToolCtx, ToolError, ToolResult, ToolSchema, TurnId,
};
use keel_runtime::{
    EchoTool, EventLog, InteractionBroker, PermissionEngine, ToolDispatcher, ToolRegistry,
};

#[derive(Default)]
struct Probe {
    current: AtomicUsize,
    max: AtomicUsize,
}

impl Probe {
    fn enter(&self) {
        let current = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        self.max.fetch_max(current, Ordering::SeqCst);
    }

    fn leave(&self) {
        self.current.fetch_sub(1, Ordering::SeqCst);
    }

    fn max(&self) -> usize {
        self.max.load(Ordering::SeqCst)
    }
}

struct ProbeTool {
    name: &'static str,
    risk: RiskLevel,
    delay: Duration,
    probe: Arc<Probe>,
}

#[async_trait]
impl Tool for ProbeTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name.to_owned(),
            description: "probe".to_owned(),
            risk: self.risk,
            params: Vec::<ParamSpec>::new(),
        }
    }

    fn permission_requests(&self, _args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        Ok(Vec::new())
    }

    async fn call(&self, _args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        self.probe.enter();
        tokio::time::sleep(self.delay).await;
        self.probe.leave();
        ToolResult::success(ctx.call_id, self.name, "ok")
    }
}

struct GuardedTool {
    path: PathBuf,
    called: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for GuardedTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "guarded".to_owned(),
            description: "writes a guarded path".to_owned(),
            risk: RiskLevel::Mutating,
            params: Vec::<ParamSpec>::new(),
        }
    }

    fn permission_requests(&self, _args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        Ok(vec![PermissionRequest {
            tool: "guarded".to_owned(),
            access: keel_core::AccessKind::Write,
            path: Some(self.path.clone()),
        }])
    }

    async fn call(&self, _args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        self.called.fetch_add(1, Ordering::SeqCst);
        ToolResult::success(ctx.call_id, "guarded", "ok")
    }
}

fn registry(tools: Vec<Arc<dyn Tool>>) -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    for tool in tools {
        registry.register(tool).unwrap();
    }
    Arc::new(registry)
}

fn dispatcher(
    registry: Arc<ToolRegistry>,
    root: &Path,
    timeout: Duration,
    level: PermissionLevel,
) -> (ToolDispatcher, Arc<EventLog>) {
    let events = Arc::new(EventLog::new());
    let permission = Arc::new(PermissionEngine::new(root, level).unwrap());
    (
        ToolDispatcher::new(registry, events.clone(), permission, timeout),
        events,
    )
}

fn call(id: u64, name: &str, args: ToolArgs) -> ToolCallRequest {
    ToolCallRequest {
        id: ToolCallId::new(id),
        name: name.to_owned(),
        args,
    }
}

fn visible(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[tokio::test]
async fn read_only_calls_run_concurrently() {
    let dir = tempfile::tempdir().unwrap();
    let probe = Arc::new(Probe::default());
    let registry = registry(vec![
        Arc::new(ProbeTool {
            name: "read_a",
            risk: RiskLevel::ReadOnly,
            delay: Duration::from_millis(60),
            probe: probe.clone(),
        }),
        Arc::new(ProbeTool {
            name: "read_b",
            risk: RiskLevel::ReadOnly,
            delay: Duration::from_millis(60),
            probe: probe.clone(),
        }),
    ]);
    let (dispatcher, _events) = dispatcher(
        registry,
        dir.path(),
        Duration::from_secs(1),
        PermissionLevel::FullAuto,
    );

    let results = dispatcher
        .dispatch(
            TurnId::new(1),
            vec![
                call(1, "read_a", ToolArgs::new()),
                call(2, "read_b", ToolArgs::new()),
            ],
            &visible(&["read_a", "read_b"]),
            &CancelToken::new(),
        )
        .await;

    assert!(results.iter().all(ToolResult::is_success));
    assert_eq!(probe.max(), 2);
}

#[tokio::test]
async fn mutating_calls_are_serialized() {
    let dir = tempfile::tempdir().unwrap();
    let probe = Arc::new(Probe::default());
    let registry = registry(vec![
        Arc::new(ProbeTool {
            name: "write_a",
            risk: RiskLevel::Mutating,
            delay: Duration::from_millis(20),
            probe: probe.clone(),
        }),
        Arc::new(ProbeTool {
            name: "write_b",
            risk: RiskLevel::Mutating,
            delay: Duration::from_millis(20),
            probe: probe.clone(),
        }),
    ]);
    let (dispatcher, _events) = dispatcher(
        registry,
        dir.path(),
        Duration::from_secs(1),
        PermissionLevel::FullAuto,
    );

    let results = dispatcher
        .dispatch(
            TurnId::new(1),
            vec![
                call(1, "write_a", ToolArgs::new()),
                call(2, "write_b", ToolArgs::new()),
            ],
            &visible(&["write_a", "write_b"]),
            &CancelToken::new(),
        )
        .await;

    assert!(results.iter().all(ToolResult::is_success));
    assert_eq!(probe.max(), 1);
}

#[tokio::test]
async fn tool_timeout_returns_a_timeout_error() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(vec![Arc::new(ProbeTool {
        name: "slow",
        risk: RiskLevel::ReadOnly,
        delay: Duration::from_millis(200),
        probe: Arc::new(Probe::default()),
    })]);
    let (dispatcher, _events) = dispatcher(
        registry,
        dir.path(),
        Duration::from_millis(20),
        PermissionLevel::FullAuto,
    );

    let results = dispatcher
        .dispatch(
            TurnId::new(1),
            vec![call(1, "slow", ToolArgs::new())],
            &visible(&["slow"]),
            &CancelToken::new(),
        )
        .await;

    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].error().map(|error| error.code),
        Some(ErrorCode::Timeout)
    );
}

#[tokio::test]
async fn unknown_tool_returns_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let (dispatcher, _events) = dispatcher(
        Arc::new(ToolRegistry::new()),
        dir.path(),
        Duration::from_secs(1),
        PermissionLevel::FullAuto,
    );

    let results = dispatcher
        .dispatch(
            TurnId::new(1),
            vec![call(1, "missing", ToolArgs::new())],
            &visible(&["missing"]),
            &CancelToken::new(),
        )
        .await;

    assert_eq!(
        results[0].error().map(|error| error.code),
        Some(ErrorCode::NotFound)
    );
}

#[tokio::test]
async fn invisible_tool_is_permission_denied() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(vec![Arc::new(EchoTool::new())]);
    let (dispatcher, _events) = dispatcher(
        registry,
        dir.path(),
        Duration::from_secs(1),
        PermissionLevel::FullAuto,
    );

    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hi".to_owned()));
    let results = dispatcher
        .dispatch(
            TurnId::new(1),
            vec![call(1, "echo", args)],
            &BTreeSet::new(),
            &CancelToken::new(),
        )
        .await;

    assert_eq!(
        results[0].error().map(|error| error.code),
        Some(ErrorCode::PermissionDenied)
    );
}

#[tokio::test]
async fn invalid_arguments_are_rejected_before_the_tool_runs() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(vec![Arc::new(EchoTool::new())]);
    let (dispatcher, _events) = dispatcher(
        registry,
        dir.path(),
        Duration::from_secs(1),
        PermissionLevel::FullAuto,
    );

    let results = dispatcher
        .dispatch(
            TurnId::new(1),
            vec![call(1, "echo", ToolArgs::new())],
            &visible(&["echo"]),
            &CancelToken::new(),
        )
        .await;

    assert_eq!(
        results[0].error().map(|error| error.code),
        Some(ErrorCode::InvalidArguments)
    );
}

#[tokio::test]
async fn dispatcher_enforces_permissions_before_calling_a_tool() {
    let dir = tempfile::tempdir().unwrap();

    let called_inside = Arc::new(AtomicUsize::new(0));
    let inside_registry = registry(vec![Arc::new(GuardedTool {
        path: dir.path().join("inside.txt"),
        called: called_inside.clone(),
    })]);
    let (inside_dispatcher, _events) = dispatcher(
        inside_registry,
        dir.path(),
        Duration::from_secs(1),
        PermissionLevel::WorkspaceAuto,
    );
    let results = inside_dispatcher
        .dispatch(
            TurnId::new(1),
            vec![call(1, "guarded", ToolArgs::new())],
            &visible(&["guarded"]),
            &CancelToken::new(),
        )
        .await;
    assert!(results[0].is_success());
    assert_eq!(called_inside.load(Ordering::SeqCst), 1);

    let called_outside = Arc::new(AtomicUsize::new(0));
    let outside_registry = registry(vec![Arc::new(GuardedTool {
        path: dir.path().join("..").join("outside.txt"),
        called: called_outside.clone(),
    })]);
    let (outside_dispatcher, _events) = dispatcher(
        outside_registry,
        dir.path(),
        Duration::from_secs(1),
        PermissionLevel::WorkspaceAuto,
    );
    let results = outside_dispatcher
        .dispatch(
            TurnId::new(2),
            vec![call(2, "guarded", ToolArgs::new())],
            &visible(&["guarded"]),
            &CancelToken::new(),
        )
        .await;
    assert_eq!(
        results[0].error().map(|error| error.code),
        Some(ErrorCode::PermissionDenied)
    );
    assert_eq!(called_outside.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn permission_confirmation_uses_the_interaction_port() {
    let dir = tempfile::tempdir().unwrap();
    let called = Arc::new(AtomicUsize::new(0));
    let outside_registry = registry(vec![Arc::new(GuardedTool {
        path: dir.path().join("..").join("outside.txt"),
        called: called.clone(),
    })]);
    let (dispatcher, events) = dispatcher(
        outside_registry,
        dir.path(),
        Duration::from_secs(1),
        PermissionLevel::WorkspaceAuto,
    );
    let port = Arc::new(common::ScriptedInteractionPort::new(vec![
        InteractionResponse::Approved {
            scope: GrantScope::Once,
        },
    ]));
    let broker = Arc::new(InteractionBroker::new(
        port,
        InteractionPolicy::default(),
        events,
    ));
    let dispatcher = dispatcher.with_interactions(broker);

    let results = dispatcher
        .dispatch(
            TurnId::new(1),
            vec![call(1, "guarded", ToolArgs::new())],
            &visible(&["guarded"]),
            &CancelToken::new(),
        )
        .await;

    assert!(results[0].is_success());
    assert_eq!(called.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn approved_full_auto_escalation_skips_later_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let called = Arc::new(AtomicUsize::new(0));
    let outside_registry = registry(vec![Arc::new(GuardedTool {
        path: dir.path().join("..").join("outside.txt"),
        called: called.clone(),
    })]);
    let (dispatcher, events) = dispatcher(
        outside_registry,
        dir.path(),
        Duration::from_secs(1),
        PermissionLevel::WorkspaceAuto,
    );
    let port = Arc::new(common::ScriptedInteractionPort::new(vec![
        InteractionResponse::Approved {
            scope: GrantScope::FullAutoSession {
                ttl: Duration::from_secs(60),
            },
        },
    ]));
    let broker = Arc::new(InteractionBroker::new(
        port,
        InteractionPolicy::default(),
        events,
    ));
    let dispatcher = dispatcher.with_interactions(broker);

    for turn in [1, 2] {
        let results = dispatcher
            .dispatch(
                TurnId::new(turn),
                vec![call(turn, "guarded", ToolArgs::new())],
                &visible(&["guarded"]),
                &CancelToken::new(),
            )
            .await;
        assert!(results[0].is_success());
    }
    assert_eq!(called.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn deferred_permission_confirmation_denies_the_call() {
    let dir = tempfile::tempdir().unwrap();
    let called = Arc::new(AtomicUsize::new(0));
    let outside_registry = registry(vec![Arc::new(GuardedTool {
        path: dir.path().join("..").join("outside.txt"),
        called: called.clone(),
    })]);
    let (dispatcher, events) = dispatcher(
        outside_registry,
        dir.path(),
        Duration::from_secs(1),
        PermissionLevel::WorkspaceAuto,
    );
    let port = Arc::new(common::ScriptedInteractionPort::new(vec![
        InteractionResponse::Deferred,
    ]));
    let broker = Arc::new(InteractionBroker::new(
        port,
        InteractionPolicy::default(),
        events,
    ));
    let dispatcher = dispatcher.with_interactions(broker);

    let results = dispatcher
        .dispatch(
            TurnId::new(1),
            vec![call(1, "guarded", ToolArgs::new())],
            &visible(&["guarded"]),
            &CancelToken::new(),
        )
        .await;

    assert_eq!(
        results[0].error().map(|error| error.code),
        Some(ErrorCode::PermissionDenied)
    );
    assert_eq!(called.load(Ordering::SeqCst), 0);
}
