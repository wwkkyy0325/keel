use std::time::Duration;

use keel_core::{
    CommitCtx, CommitDecision, ContextRecipe, InteractionPolicy, MemoryPolicy, ModelRole,
    ModelRoute, Policy, RiskLevel, ToolSchema, VerificationMode,
};

/// Immutable policy for one turn, resolved from a profile.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub profile_name: String,
    pub system_prompt: String,
    pub visible_tool_names: Vec<String>,
    pub plan_mode: bool,
    pub tool_budget: u32,
    pub context: ContextRecipe,
    pub thinking_budget: u32,
    pub verification: VerificationMode,
    pub memory: MemoryPolicy,
    pub permission_level: keel_core::PermissionLevel,
    pub model_route: ModelRoute,
    pub model_base_url: Option<String>,
    pub api_key_env: Option<String>,
    /// Optional `service:account` keyring entry, used when the env var is
    /// absent. The value is a name, never the secret itself.
    pub api_key_keyring: Option<String>,
    /// Whether the endpoint accepts the kernel's `reasoning_effort` hint.
    pub supports_reasoning: bool,
    pub max_output_tokens: u32,
    /// Model hard capacity. `context.budget_tokens` is the working set and
    /// may be far below this.
    pub context_window: u32,
    pub interaction: InteractionPolicy,
}

impl RuntimeConfig {
    /// Kernel limits derived from host configuration. These are not policy
    /// questions; the host owns timeouts and call budgets.
    ///
    /// The defaults are runaway protection, not a per-task budget: one turn is
    /// expected to finish a multi-step task without the user typing
    /// "continue". `tools.budget` in the profile narrows the tool calls.
    pub fn limits(&self) -> TurnLimits {
        TurnLimits {
            max_model_calls: 500,
            max_tool_calls: self.tool_budget,
            // Inactivity windows, not wall-clock limits: a command or stream
            // that keeps producing output is not cut off.
            tool_timeout: Duration::from_secs(300),
            turn_timeout: Duration::from_secs(24 * 60 * 60),
            // Cost scales with the working set, not the model's capacity:
            // about a hundred full-context rounds, never below a million
            // tokens. The model never sees this number.
            max_turn_input_tokens: u64::from(self.context.budget_tokens)
                .saturating_mul(100)
                .max(1_000_000),
        }
    }
}

impl Policy for RuntimeConfig {
    fn visible_tools(&self, all: &[ToolSchema]) -> Vec<ToolSchema> {
        all.iter()
            .filter(|schema| {
                let allowed_by_profile = self
                    .visible_tool_names
                    .iter()
                    .any(|name| name == &schema.name);
                let allowed_by_plan_mode = !self.plan_mode || schema.risk == RiskLevel::ReadOnly;
                allowed_by_profile && allowed_by_plan_mode
            })
            .cloned()
            .collect()
    }

    fn context_recipe(&self) -> ContextRecipe {
        self.context.clone()
    }

    fn thinking_budget(&self) -> u32 {
        self.thinking_budget
    }

    fn verification_mode(&self) -> VerificationMode {
        self.verification.clone()
    }

    fn memory_policy(&self) -> MemoryPolicy {
        self.memory.clone()
    }

    fn model_route(&self, _role: ModelRole) -> ModelRoute {
        self.model_route.clone()
    }

    fn before_commit(&self, _ctx: &CommitCtx<'_>) -> CommitDecision {
        CommitDecision::Commit
    }
}

/// Resource limits enforced by the turn driver and tool dispatcher.
#[derive(Debug, Clone, Copy)]
pub struct TurnLimits {
    pub max_model_calls: u32,
    pub max_tool_calls: u32,
    pub tool_timeout: Duration,
    pub turn_timeout: Duration,
    /// Total input tokens this turn may spend. When it is reached the driver
    /// forces the final answer call; the limit is host-side and never appears
    /// in a prompt or an event payload the model can see.
    pub max_turn_input_tokens: u64,
}
