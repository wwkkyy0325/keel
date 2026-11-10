//! Conformance probes for hosts built on the Keel kernel.
//!
//! A derived host depends on this crate as a dev-dependency and runs the probes
//! after a kernel upgrade. The probes exercise the kernel's contract through a
//! real driver: a turn, a tool call, a model failure, a cancellation and a
//! session snapshot. They do not replace the host's own tests; they catch a
//! kernel change that the host would otherwise only notice in production.
//!
//! ```no_run
//! # async fn example() {
//! use keel_runtime::MockStep;
//! use keel_testkit::{assert_done, assert_event_order, Probe};
//!
//! let probe = Probe::new(vec![MockStep::Text("hello".to_owned())]).unwrap();
//! let outcome = probe.run("hi").await;
//! assert_done(&outcome).unwrap();
//! assert_event_order(
//!     probe.events(),
//!     &["turn_started", "model_started", "model_finished", "turn_finished"],
//! )
//! .unwrap();
//! # }
//! ```

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use keel_core::{
    CancelToken, CompactionStep, ContextRecipe, ErrorCode, Event, InteractionPolicy, KernelError,
    MemoryMode, MemoryPolicy, ModelRoute, PermissionLevel, Policy, PromotionMode, TurnState,
    VerificationMode,
};
use keel_runtime::{
    validate_config, ContextBuilder, DeferAllInteractionPort, EchoTool, EventLog,
    InteractionBroker, LatestStage, ListDirTool, MockModel, MockStep, PermissionEngine,
    ReadFileTool, RuntimeConfig, SystemStage, ToolRegistry, TurnDriver, TurnDriverConfig,
    TurnLimits, TurnOutcome,
};

/// A driver with a mock model, a temporary workspace and the reference tools.
///
/// The probe owns the workspace; dropping it removes the directory.
pub struct Probe {
    _dir: tempfile::TempDir,
    events: Arc<EventLog>,
    driver: TurnDriver<MockModel>,
}

impl Probe {
    /// Builds a probe that answers with `steps` in order.
    pub fn new(steps: Vec<MockStep>) -> Result<Self, KernelError> {
        let dir = tempfile::TempDir::new().map_err(|error| KernelError::Io {
            message: format!("cannot create the probe workspace: {error}"),
        })?;
        let root = dir.path();
        let events = Arc::new(EventLog::new());

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool::new()))?;
        registry.register(Arc::new(ReadFileTool::new()))?;
        registry.register(Arc::new(ListDirTool::new()))?;
        let context = ContextBuilder::new(vec![
            Arc::new(SystemStage::new()),
            Arc::new(LatestStage::new()),
        ]);
        let config = probe_config();
        validate_config(&config, &registry, &context)?;

        let permissions = Arc::new(PermissionEngine::new(root, config.permission_level)?);
        let interactions = Arc::new(InteractionBroker::new(
            Arc::new(DeferAllInteractionPort),
            config.interaction,
            events.clone(),
        ));
        let policy: Arc<dyn Policy> = Arc::new(config);
        let driver = TurnDriver::new(
            MockModel::new(steps),
            Arc::new(registry),
            context,
            policy,
            TurnDriverConfig {
                events: events.clone(),
                permissions,
                interactions,
                system_prompt: "You are a conformance probe.".to_owned(),
                max_output_tokens: 1024,
                context_window: 128_000,
                limits: probe_limits(),
                todos: None,
                background: None,
                options: Default::default(),
            },
        );
        Ok(Self {
            _dir: dir,
            events,
            driver,
        })
    }

    /// The probe's temporary workspace.
    pub fn root(&self) -> &Path {
        self._dir.path()
    }

    pub fn events(&self) -> &Arc<EventLog> {
        &self.events
    }

    pub fn snapshot(&self) -> keel_core::SessionSnapshot {
        self.driver.session_snapshot()
    }

    /// Runs one turn with a fresh cancel token.
    pub async fn run(&self, input: &str) -> TurnOutcome {
        self.driver.run(input, CancelToken::new()).await
    }

    /// Runs one turn and cancels it after twenty milliseconds. Use with
    /// `MockStep::Hang` (or a slow tool) to exercise the cancelled path.
    pub async fn run_cancelled(&self, input: &str) -> TurnOutcome {
        let cancel = CancelToken::new();
        let signal = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            signal.cancel();
        });
        self.driver.run(input, cancel).await
    }
}

fn probe_config() -> RuntimeConfig {
    RuntimeConfig {
        profile_name: "probe".to_owned(),
        system_prompt: "You are a conformance probe.".to_owned(),
        visible_tool_names: vec![
            "echo".to_owned(),
            "read_file".to_owned(),
            "list_dir".to_owned(),
        ],
        plan_mode: false,
        tool_budget: 8,
        context: ContextRecipe {
            stages: vec!["system".to_owned(), "latest".to_owned()],
            budget_tokens: 4096,
            max_budget_tokens: 4096,
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
            model: "probe".to_owned(),
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

fn probe_limits() -> TurnLimits {
    TurnLimits {
        max_model_calls: 8,
        max_tool_calls: 8,
        tool_timeout: Duration::from_secs(2),
        turn_timeout: Duration::from_secs(5),
        max_turn_input_tokens: u64::MAX,
    }
}

/// Event names in emission order.
pub fn event_names(log: &EventLog) -> Vec<String> {
    log.snapshot()
        .into_iter()
        .map(|record| record.event.name().to_owned())
        .collect()
}

/// Asserts that `expected` appears as a subsequence of the emitted events.
/// Extra events between them are allowed.
pub fn assert_event_order(log: &EventLog, expected: &[&str]) -> Result<(), String> {
    let actual = event_names(log);
    let mut position = 0;
    for name in &actual {
        if position < expected.len() && name == expected[position] {
            position += 1;
        }
    }
    if position == expected.len() {
        Ok(())
    } else {
        Err(format!(
            "expected the event subsequence {expected:?}, emitted {actual:?}"
        ))
    }
}

pub fn assert_event_present(log: &EventLog, name: &str) -> Result<(), String> {
    let actual = event_names(log);
    if actual.iter().any(|actual| actual == name) {
        Ok(())
    } else {
        Err(format!("expected event `{name}`, emitted {actual:?}"))
    }
}

pub fn assert_done(outcome: &TurnOutcome) -> Result<(), String> {
    if outcome.state == TurnState::Done && outcome.error.is_none() {
        Ok(())
    } else {
        Err(format!(
            "expected Done, got {:?} ({:?})",
            outcome.state, outcome.error
        ))
    }
}

pub fn assert_cancelled(outcome: &TurnOutcome) -> Result<(), String> {
    if outcome.state == TurnState::Cancelled {
        Ok(())
    } else {
        Err(format!("expected Cancelled, got {:?}", outcome.state))
    }
}

pub fn assert_failed(outcome: &TurnOutcome, code: ErrorCode) -> Result<(), String> {
    match &outcome.error {
        Some(error) if error.code() == code => Ok(()),
        Some(error) => Err(format!("expected {code:?}, got {:?}", error.code())),
        None => Err(format!(
            "expected failure {code:?}, the turn finished {:?}",
            outcome.state
        )),
    }
}

/// Events the kernel is expected to emit for one turn. A host can compare its
/// own trace with this list to spot a missing or reordered boundary.
pub fn text_turn_events() -> Vec<&'static str> {
    vec![
        "turn_started",
        "model_started",
        "model_finished",
        "turn_finished",
    ]
}

/// The event names a host should see when a tool call succeeds.
pub fn tool_turn_events() -> Vec<&'static str> {
    vec![
        "turn_started",
        "model_started",
        "model_finished",
        "tool_started",
        "tool_finished",
        "model_started",
        "model_finished",
        "turn_finished",
    ]
}

/// Every event variant name. A host that switches on the enum can compare
/// this list after an upgrade.
pub fn event_names_contract() -> Vec<String> {
    vec![
        "turn_started",
        "secret_warning",
        "reasoning_escalated",
        "verification_started",
        "verification_finished",
        "state_changed",
        "context_built",
        "context_pressure",
        "history_assembled",
        "history_recalled",
        "context_elided",
        "todo_updated",
        "todo_closed",
        "wrap_up_injected",
        "final_call_forced",
        "segment_opened",
        "segment_sealed",
        "summary_updated",
        "summary_failed",
        "segment_folded",
        "compression_skipped",
        "fact_promoted",
        "fact_forgotten",
        "model_started",
        "model_finished",
        "model_failed",
        "permission_granted",
        "permission_revoked",
        "permission_decided",
        "tool_started",
        "tool_finished",
        "turn_finished",
        "turn_continued",
        "turn_budget_exhausted",
        "turn_cancelled",
        "interaction_requested",
        "interaction_resolved",
        "interjection_queued",
        "interjection_delivered",
        "interjection_dropped",
        "background_started",
        "background_result",
        "sub_agent_started",
        "sub_agent_finished",
        "turn_report",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// The last event of a turn, convenient for assertions.
pub fn last_event(log: &EventLog) -> Option<Event> {
    log.snapshot().pop().map(|record| record.event)
}
