use crate::error::ErrorCode;
use crate::ids::{InteractionId, ToolCallId, TurnId};
use crate::interaction::InteractionOutcome;
use crate::model::TokenUsage;
use crate::state::{TurnEvent, TurnState};
use crate::tool::RiskLevel;

/// Typed event emitted at a fixed kernel boundary.
///
/// The event log is an append-only turn record, not sampled telemetry. It
/// carries the turn input and final text so a turn can be replayed; tool
/// arguments and tool outputs are intentionally excluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    TurnStarted {
        turn_id: TurnId,
        input: String,
    },
    SecretWarning {
        turn_id: TurnId,
        kind: String,
    },
    ReasoningEscalated {
        turn_id: TurnId,
        from: String,
        to: String,
        reason: String,
    },
    VerificationFinished {
        turn_id: TurnId,
        passed: bool,
        required_failed: Vec<String>,
        required_unverified: Vec<String>,
    },
    StateChanged {
        turn_id: TurnId,
        from: TurnState,
        to: TurnState,
        event: TurnEvent,
    },
    ContextBuilt {
        turn_id: TurnId,
        stages: Vec<String>,
        tokens: u32,
        budget: u32,
    },
    ContextPressure {
        turn_id: TurnId,
        used: u32,
        ceiling: u32,
        growth: u32,
        headroom_calls: u32,
        level: String,
    },
    HistoryAssembled {
        turn_id: TurnId,
        detail_percent: u32,
        detail_turns: u32,
        digest_turns: u32,
        dropped_turns: u32,
        tokens: u32,
    },
    WrapUpInjected {
        turn_id: TurnId,
        used: u32,
        ceiling: u32,
    },
    FinalCallForced {
        turn_id: TurnId,
    },
    SegmentOpened {
        turn_id: TurnId,
        segment_id: u32,
        topic: String,
    },
    SegmentSealed {
        turn_id: TurnId,
        segment_id: u32,
        reason: String,
    },
    ModelStarted {
        turn_id: TurnId,
        provider: String,
        model: String,
    },
    ModelFinished {
        turn_id: TurnId,
        tool_calls: usize,
        usage: TokenUsage,
    },
    ToolStarted {
        turn_id: TurnId,
        call_id: ToolCallId,
        tool: String,
        risk: RiskLevel,
    },
    ToolFinished {
        turn_id: TurnId,
        call_id: ToolCallId,
        tool: String,
        ok: bool,
        error: Option<ErrorCode>,
        duration_ms: u64,
    },
    TurnFinished {
        turn_id: TurnId,
        state: TurnState,
        text: String,
    },
    TurnCancelled {
        turn_id: TurnId,
        state: TurnState,
    },
    InteractionRequested {
        turn_id: TurnId,
        interaction_id: InteractionId,
        kind: String,
    },
    InteractionResolved {
        turn_id: TurnId,
        interaction_id: InteractionId,
        outcome: InteractionOutcome,
    },
    TurnReport {
        turn_id: TurnId,
        state: TurnState,
        tool_calls: u32,
        interactions: u32,
        deferred: usize,
    },
}

/// Sink for events emitted at a kernel boundary, such as a context stage.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: Event);
}

impl Event {
    pub fn turn_id(&self) -> TurnId {
        match self {
            Self::TurnStarted { turn_id, .. }
            | Self::SecretWarning { turn_id, .. }
            | Self::ReasoningEscalated { turn_id, .. }
            | Self::VerificationFinished { turn_id, .. }
            | Self::StateChanged { turn_id, .. }
            | Self::ContextBuilt { turn_id, .. }
            | Self::ContextPressure { turn_id, .. }
            | Self::HistoryAssembled { turn_id, .. }
            | Self::WrapUpInjected { turn_id, .. }
            | Self::FinalCallForced { turn_id, .. }
            | Self::SegmentOpened { turn_id, .. }
            | Self::SegmentSealed { turn_id, .. }
            | Self::ModelStarted { turn_id, .. }
            | Self::ModelFinished { turn_id, .. }
            | Self::ToolStarted { turn_id, .. }
            | Self::ToolFinished { turn_id, .. }
            | Self::TurnFinished { turn_id, .. }
            | Self::TurnCancelled { turn_id, .. }
            | Self::InteractionRequested { turn_id, .. }
            | Self::InteractionResolved { turn_id, .. }
            | Self::TurnReport { turn_id, .. } => *turn_id,
        }
    }

    /// Stable event name used in logs and tests.
    pub fn name(&self) -> &'static str {
        match self {
            Self::TurnStarted { .. } => "turn_started",
            Self::SecretWarning { .. } => "secret_warning",
            Self::ReasoningEscalated { .. } => "reasoning_escalated",
            Self::VerificationFinished { .. } => "verification_finished",
            Self::StateChanged { .. } => "state_changed",
            Self::ContextBuilt { .. } => "context_built",
            Self::ContextPressure { .. } => "context_pressure",
            Self::HistoryAssembled { .. } => "history_assembled",
            Self::WrapUpInjected { .. } => "wrap_up_injected",
            Self::FinalCallForced { .. } => "final_call_forced",
            Self::SegmentOpened { .. } => "segment_opened",
            Self::SegmentSealed { .. } => "segment_sealed",
            Self::ModelStarted { .. } => "model_started",
            Self::ModelFinished { .. } => "model_finished",
            Self::ToolStarted { .. } => "tool_started",
            Self::ToolFinished { .. } => "tool_finished",
            Self::TurnFinished { .. } => "turn_finished",
            Self::TurnCancelled { .. } => "turn_cancelled",
            Self::InteractionRequested { .. } => "interaction_requested",
            Self::InteractionResolved { .. } => "interaction_resolved",
            Self::TurnReport { .. } => "turn_report",
        }
    }
}
