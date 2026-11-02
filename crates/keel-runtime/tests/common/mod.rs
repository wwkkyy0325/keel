#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    CancelToken, CompactionStep, ContextRecipe, Event, InteractionPolicy, InteractionPort,
    InteractionRequest, InteractionResponse, MemoryMode, MemoryPolicy, ModelProvider, ModelRoute,
    PermissionLevel, Policy, PromotionMode, TurnId, TurnState, VerificationMode,
};
use keel_runtime::{
    validate_config, CodeSearchTool, ContextBuilder, DeferAllInteractionPort, DeleteFileTool,
    EchoTool, EventLog, HistoryStage, InteractionBroker, LatestStage, ListDirTool, LocalTrashStore,
    MakeDirTool, MemoryStage, MoveFileTool, PermissionEngine, ReadFileTool, RuntimeConfig,
    SystemStage, ToolRegistry, TurnDriver, TurnDriverConfig, TurnLimits, WriteFileTool,
};

pub fn runtime_config(
    visible: &[&str],
    tool_budget: u32,
    permission_level: PermissionLevel,
) -> RuntimeConfig {
    RuntimeConfig {
        profile_name: "test".to_owned(),
        system_prompt: "You are a test agent.".to_owned(),
        visible_tool_names: visible.iter().map(|name| (*name).to_owned()).collect(),
        plan_mode: false,
        tool_budget,
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
        permission_level,
        model_route: ModelRoute {
            provider: "mock".to_owned(),
            model: "mock-test".to_owned(),
        },
        model_base_url: None,
        api_key_env: None,
        api_key_keyring: None,
        supports_reasoning: false,
        max_output_tokens: 1024,
        context_window: 128_000,
        interaction: InteractionPolicy::default(),
    }
}

pub fn standard_registry(root: &Path) -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool::new())).unwrap();
    registry.register(Arc::new(ReadFileTool::new())).unwrap();
    registry.register(Arc::new(ListDirTool::new())).unwrap();
    registry.register(Arc::new(MakeDirTool::new())).unwrap();
    registry.register(Arc::new(CodeSearchTool::new())).unwrap();
    registry
        .register(Arc::new(keel_runtime::ShellTool::coding_defaults()))
        .unwrap();
    let trash: Arc<dyn keel_runtime::TrashStore> =
        Arc::new(LocalTrashStore::new(root.to_path_buf()));
    registry
        .register(Arc::new(WriteFileTool::new(trash.clone())))
        .unwrap();
    registry
        .register(Arc::new(MoveFileTool::new(trash.clone())))
        .unwrap();
    registry
        .register(Arc::new(DeleteFileTool::new(trash.clone())))
        .unwrap();
    registry
        .register(Arc::new(keel_runtime::ListTrashTool::new(trash.clone())))
        .unwrap();
    registry
        .register(Arc::new(keel_runtime::RestoreFileTool::new(trash)))
        .unwrap();
    Arc::new(registry)
}

pub fn standard_context() -> ContextBuilder {
    ContextBuilder::new(vec![
        Arc::new(SystemStage::new()),
        Arc::new(MemoryStage::new()),
        Arc::new(HistoryStage::new()),
        Arc::new(LatestStage::new()),
    ])
}

pub fn test_limits() -> TurnLimits {
    TurnLimits {
        max_model_calls: 8,
        max_tool_calls: 8,
        tool_timeout: Duration::from_millis(200),
        turn_timeout: Duration::from_secs(2),
    }
}

pub fn build_driver<M: ModelProvider>(
    model: M,
    config: RuntimeConfig,
    events: Arc<EventLog>,
    root: &Path,
    limits: TurnLimits,
) -> TurnDriver<M> {
    build_driver_with_port(
        model,
        config,
        events,
        root,
        limits,
        Arc::new(DeferAllInteractionPort),
    )
}

pub fn build_driver_with_port<M: ModelProvider>(
    model: M,
    config: RuntimeConfig,
    events: Arc<EventLog>,
    root: &Path,
    limits: TurnLimits,
    port: Arc<dyn InteractionPort>,
) -> TurnDriver<M> {
    let tools = standard_registry(root);
    let context = standard_context();
    validate_config(&config, &tools, &context).unwrap();

    let permissions = Arc::new(PermissionEngine::new(root, config.permission_level).unwrap());
    let interaction_policy = config.interaction;
    let interactions = Arc::new(InteractionBroker::new(
        port,
        interaction_policy,
        events.clone(),
    ));
    let system_prompt = config.system_prompt.clone();
    let max_output_tokens = config.max_output_tokens;
    let context_window = config.context_window;
    let policy: Arc<dyn Policy> = Arc::new(config);

    TurnDriver::new(
        model,
        tools,
        context,
        policy,
        TurnDriverConfig {
            events,
            permissions,
            interactions,
            system_prompt,
            max_output_tokens,
            context_window,
            limits,
            todos: None,
            options: keel_runtime::KernelOptions::default(),
        },
    )
}

/// Interaction port with a fixed response script.
pub struct ScriptedInteractionPort {
    responses: Mutex<VecDeque<InteractionResponse>>,
}

impl ScriptedInteractionPort {
    pub fn new(responses: Vec<InteractionResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
        }
    }
}

#[async_trait]
impl InteractionPort for ScriptedInteractionPort {
    async fn request(
        &self,
        _request: InteractionRequest,
        _cancel: &CancelToken,
    ) -> InteractionResponse {
        self.responses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .unwrap_or(InteractionResponse::Deferred)
    }
}

/// Interaction port that never answers, for timeout tests.
pub struct HangingInteractionPort;

#[async_trait]
impl InteractionPort for HangingInteractionPort {
    async fn request(
        &self,
        _request: InteractionRequest,
        _cancel: &CancelToken,
    ) -> InteractionResponse {
        tokio::time::sleep(Duration::from_secs(3600)).await;
        InteractionResponse::Deferred
    }
}

pub fn states(events: &EventLog, turn_id: TurnId) -> Vec<TurnState> {
    events
        .for_turn(turn_id)
        .into_iter()
        .filter_map(|record| match record.event {
            Event::StateChanged { to, .. } => Some(to),
            _ => None,
        })
        .collect()
}
