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

impl Event {
    pub fn turn_id(&self) -> TurnId {
        match self {
            Self::TurnStarted { turn_id, .. }
            | Self::StateChanged { turn_id, .. }
            | Self::ContextBuilt { turn_id, .. }
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
            Self::StateChanged { .. } => "state_changed",
            Self::ContextBuilt { .. } => "context_built",
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
