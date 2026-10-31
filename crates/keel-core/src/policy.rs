use crate::tool::ToolSchema;

/// Model role used for routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRole {
    Main,
    Compact,
    Subagent,
    Verifier,
    Commit,
}

/// Provider and model selected for a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRoute {
    pub provider: String,
    pub model: String,
}

/// Compaction ladder steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionStep {
    Trim,
    RollingSummary,
    LlmCompact,
    TaskBoundary,
}

/// Resolved context recipe for one turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRecipe {
    pub stages: Vec<String>,
    pub budget_tokens: u32,
    pub compaction: Vec<CompactionStep>,
}

/// Kernel-owned reasoning effort for one turn.
///
/// The ladder is monotonic within a turn and starts at `Low`. The model cannot
/// choose the level; the driver escalates it on real failure signals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReasoningLevel {
    #[default]
    Low,
    High,
    Max,
}

impl ReasoningLevel {
    pub fn next(self) -> Self {
        match self {
            Self::Low => Self::High,
            Self::High | Self::Max => Self::Max,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::High => "high",
            Self::Max => "max",
        }
    }
}

/// Verification policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationMode {
    None,
    Evidence,
    Tests,
    Custom(String),
}

/// Memory mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMode {
    Off,
    Facts,
    Episodic,
}

/// Memory promotion rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotionMode {
    Strict,
    Soft,
}

/// Memory policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryPolicy {
    pub mode: MemoryMode,
    pub promotion: PromotionMode,
    pub recall_limit: usize,
}

/// Context passed to the commit decision.
pub struct CommitCtx<'a> {
    pub text: &'a str,
    pub tool_rounds: u32,
}

/// Decision returned before a turn converges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitDecision {
    Commit,
    Reprompt,
    Ask,
}

/// The only questions the kernel asks the middle layer at runtime.
pub trait Policy: Send + Sync {
    fn visible_tools(&self, all: &[ToolSchema]) -> Vec<ToolSchema>;
    fn context_recipe(&self) -> ContextRecipe;
    fn thinking_budget(&self) -> u32;
    fn verification_mode(&self) -> VerificationMode;
    fn memory_policy(&self) -> MemoryPolicy;
    fn model_route(&self, role: ModelRole) -> ModelRoute;
    fn before_commit(&self, ctx: &CommitCtx<'_>) -> CommitDecision;
}
