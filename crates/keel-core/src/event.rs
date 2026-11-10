use serde::Serialize;

use crate::error::ErrorCode;
use crate::ids::{InteractionId, ToolCallId, TurnId};
use crate::interaction::InteractionOutcome;
use crate::model::TokenUsage;
use crate::permission::AccessKind;
use crate::state::{TurnEvent, TurnState};
use crate::tool::RiskLevel;

/// Typed event emitted at a fixed kernel boundary.
///
/// The event log is an append-only turn record, not sampled telemetry. It
/// carries the turn input and final text so a turn can be replayed; tool
/// arguments are intentionally excluded. `ToolFinished` carries a bounded
/// output preview for a live UI, but that field is skipped during
/// serialization, so tool output never reaches the store.
/// One tool call a sub-agent made, kept in the parent's audit event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubAgentStep {
    pub tool: String,
    pub ok: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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
        summary_tokens: u32,
        recall_candidates: u32,
        recall_candidate_tokens: u32,
        recall_segments: u32,
        recall_tokens: u32,
        record_candidates: u32,
        record_segments: u32,
        record_tokens: u32,
        detail_tokens: u32,
        digest_tokens: u32,
        tokens: u32,
    },
    HistoryRecalled {
        turn_id: TurnId,
        candidates: u32,
        segments: Vec<u32>,
        tokens: u32,
        /// Top IDF-weighted score in basis points.
        score_bp: u32,
        /// Query terms found in the top segment.
        matched: u32,
        /// Query terms after stopword filtering.
        query_terms: u32,
        /// Query terms that occur somewhere in the session.
        known_terms: u32,
    },
    /// The working set grew because this session's content no longer fit the
    /// profile's base budget. Monotonic within the engine's lifetime.
    ContextEscalated {
        turn_id: TurnId,
        from: u32,
        to: u32,
        reason: String,
    },
    ContextElided {
        turn_id: TurnId,
        messages: u32,
        before: u32,
        after: u32,
    },
    TodoUpdated {
        turn_id: TurnId,
        revision: u32,
        items: u32,
        in_progress: Option<u32>,
        state: String,
    },
    TodoClosed {
        turn_id: TurnId,
        outcome: String,
        completed: u32,
        failed: u32,
        total: u32,
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
    SummaryUpdated {
        turn_id: TurnId,
        revision: u32,
        covers: Vec<u32>,
        chars: u32,
        budget: u32,
        /// True when the model failed and the kernel wrote a structured memo.
        fallback: bool,
    },
    SummaryFailed {
        turn_id: TurnId,
        reason: String,
    },
    SegmentFolded {
        turn_id: TurnId,
        segment_id: u32,
    },
    CompressionSkipped {
        turn_id: TurnId,
        segment_id: u32,
        missing: Vec<String>,
    },
    FactPromoted {
        turn_id: TurnId,
        key: String,
    },
    FactForgotten {
        turn_id: TurnId,
        key: String,
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
        duration_ms: u64,
    },
    /// A model call ended in an error or a timeout. Emitted at every failure
    /// exit so failure rate and latency are measurable from the stream.
    ModelFailed {
        turn_id: TurnId,
        provider: String,
        model: String,
        code: ErrorCode,
        message: String,
        duration_ms: u64,
    },
    ToolStarted {
        turn_id: TurnId,
        call_id: ToolCallId,
        tool: String,
        risk: RiskLevel,
        /// Bounded, redacted arguments for a live UI. Skipped during
        /// serialization so values stay out of the persisted stream.
        #[serde(skip)]
        args_preview: Option<String>,
    },
    ToolFinished {
        turn_id: TurnId,
        call_id: ToolCallId,
        tool: String,
        ok: bool,
        error: Option<ErrorCode>,
        duration_ms: u64,
        /// Bounded, redacted output for a live UI. Serialization skips it so
        /// file contents and command output stay out of the persisted event
        /// stream.
        #[serde(skip)]
        output_preview: Option<String>,
    },
    /// One permission request and the engine's decision. A confirmation also
    /// produces `InteractionRequested`/`InteractionResolved`; the final
    /// outcome is visible in `ToolFinished`.
    /// A session grant was created: a path scope or a full-auto escalation.
    /// `turn_id` is 0 when the host created it outside a turn (`/add-dir`,
    /// the mode toggle); `ttl_ms` is None for a scope that lives until exit.
    PermissionGranted {
        turn_id: TurnId,
        scope: String,
        path: Option<String>,
        access: Option<AccessKind>,
        ttl_ms: Option<u64>,
    },
    /// Session grants were dropped (the full-auto toggle was turned off).
    PermissionRevoked {
        turn_id: TurnId,
        scope: String,
    },
    PermissionDecided {
        turn_id: TurnId,
        call_id: ToolCallId,
        tool: String,
        access: AccessKind,
        path: Option<String>,
        /// `allow`, `deny` or `confirm`.
        decision: String,
        reason: Option<String>,
    },
    TurnFinished {
        turn_id: TurnId,
        state: TurnState,
        text: String,
    },
    /// The model stopped while its task list still had unfinished items, so
    /// the driver continued the same turn instead of ending it.
    TurnContinued {
        turn_id: TurnId,
        remaining: u32,
    },
    /// The turn spent its input-token budget; the next call is the answer.
    TurnBudgetExhausted {
        turn_id: TurnId,
        input_tokens: u32,
    },
    /// A delegated sub-agent started; the parent turn, the role and its cap.
    SubAgentStarted {
        turn_id: TurnId,
        role: String,
        max_calls: u32,
    },
    /// A delegated sub-agent finished; the parent turn and its usage.
    SubAgentFinished {
        turn_id: TurnId,
        role: String,
        state: TurnState,
        model_calls: u32,
        tool_calls: u32,
        input_tokens: u32,
        output_tokens: u32,
        error: Option<String>,
        /// Bounded trace of the child's tool calls.
        steps: Vec<SubAgentStep>,
    },
    /// A background command started; the handle matches its completion.
    BackgroundStarted {
        turn_id: TurnId,
        id: String,
        program: String,
        pid: u32,
    },
    /// A background command finished and its result entered the turn.
    BackgroundResult {
        turn_id: TurnId,
        id: String,
        program: String,
        output_path: String,
        exit_code: Option<i32>,
        duration_ms: u64,
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
    InterjectionQueued {
        turn_id: TurnId,
        id: u64,
        text: String,
    },
    InterjectionDelivered {
        turn_id: TurnId,
        id: u64,
        text: String,
    },
    InterjectionDropped {
        turn_id: TurnId,
        id: u64,
        text: String,
        reason: String,
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
            | Self::HistoryRecalled { turn_id, .. }
            | Self::ContextEscalated { turn_id, .. }
            | Self::ContextElided { turn_id, .. }
            | Self::TodoUpdated { turn_id, .. }
            | Self::TodoClosed { turn_id, .. }
            | Self::WrapUpInjected { turn_id, .. }
            | Self::FinalCallForced { turn_id, .. }
            | Self::SegmentOpened { turn_id, .. }
            | Self::SegmentSealed { turn_id, .. }
            | Self::SummaryUpdated { turn_id, .. }
            | Self::SummaryFailed { turn_id, .. }
            | Self::SegmentFolded { turn_id, .. }
            | Self::CompressionSkipped { turn_id, .. }
            | Self::FactPromoted { turn_id, .. }
            | Self::FactForgotten { turn_id, .. }
            | Self::ModelStarted { turn_id, .. }
            | Self::ModelFinished { turn_id, .. }
            | Self::ModelFailed { turn_id, .. }
            | Self::PermissionGranted { turn_id, .. }
            | Self::PermissionRevoked { turn_id, .. }
            | Self::SubAgentStarted { turn_id, .. }
            | Self::ToolStarted { turn_id, .. }
            | Self::ToolFinished { turn_id, .. }
            | Self::PermissionDecided { turn_id, .. }
            | Self::TurnFinished { turn_id, .. }
            | Self::TurnContinued { turn_id, .. }
            | Self::TurnBudgetExhausted { turn_id, .. }
            | Self::BackgroundStarted { turn_id, .. }
            | Self::BackgroundResult { turn_id, .. }
            | Self::SubAgentFinished { turn_id, .. }
            | Self::TurnCancelled { turn_id, .. }
            | Self::InteractionRequested { turn_id, .. }
            | Self::InteractionResolved { turn_id, .. }
            | Self::InterjectionQueued { turn_id, .. }
            | Self::InterjectionDelivered { turn_id, .. }
            | Self::InterjectionDropped { turn_id, .. }
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
            Self::HistoryRecalled { .. } => "history_recalled",
            Self::ContextEscalated { .. } => "context_escalated",
            Self::ContextElided { .. } => "context_elided",
            Self::TodoUpdated { .. } => "todo_updated",
            Self::TodoClosed { .. } => "todo_closed",
            Self::WrapUpInjected { .. } => "wrap_up_injected",
            Self::FinalCallForced { .. } => "final_call_forced",
            Self::SegmentOpened { .. } => "segment_opened",
            Self::SegmentSealed { .. } => "segment_sealed",
            Self::SummaryUpdated { .. } => "summary_updated",
            Self::SummaryFailed { .. } => "summary_failed",
            Self::SegmentFolded { .. } => "segment_folded",
            Self::CompressionSkipped { .. } => "compression_skipped",
            Self::FactPromoted { .. } => "fact_promoted",
            Self::FactForgotten { .. } => "fact_forgotten",
            Self::ModelStarted { .. } => "model_started",
            Self::ModelFinished { .. } => "model_finished",
            Self::ModelFailed { .. } => "model_failed",
            Self::PermissionGranted { .. } => "permission_granted",
            Self::PermissionRevoked { .. } => "permission_revoked",
            Self::SubAgentStarted { .. } => "sub_agent_started",
            Self::ToolStarted { .. } => "tool_started",
            Self::ToolFinished { .. } => "tool_finished",
            Self::PermissionDecided { .. } => "permission_decided",
            Self::TurnFinished { .. } => "turn_finished",
            Self::TurnContinued { .. } => "turn_continued",
            Self::TurnBudgetExhausted { .. } => "turn_budget_exhausted",
            Self::BackgroundStarted { .. } => "background_started",
            Self::BackgroundResult { .. } => "background_result",
            Self::SubAgentFinished { .. } => "sub_agent_finished",
            Self::TurnCancelled { .. } => "turn_cancelled",
            Self::InteractionRequested { .. } => "interaction_requested",
            Self::InteractionResolved { .. } => "interaction_resolved",
            Self::InterjectionQueued { .. } => "interjection_queued",
            Self::InterjectionDelivered { .. } => "interjection_delivered",
            Self::InterjectionDropped { .. } => "interjection_dropped",
            Self::TurnReport { .. } => "turn_report",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialize_for_persistence() {
        let event = Event::PermissionDecided {
            turn_id: TurnId::new(1),
            call_id: ToolCallId::new(2),
            tool: "write_file".to_owned(),
            access: AccessKind::Write,
            path: Some("note.txt".to_owned()),
            decision: "allow".to_owned(),
            reason: None,
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["permission_decided"]["turn_id"], 1);
        assert_eq!(value["permission_decided"]["access"], "write");
        assert_eq!(value["permission_decided"]["decision"], "allow");
        assert_eq!(event.name(), "permission_decided");
    }

    #[test]
    fn a_tool_output_preview_is_not_persisted() {
        let event = Event::ToolFinished {
            turn_id: TurnId::new(1),
            call_id: ToolCallId::new(2),
            tool: "shell".to_owned(),
            ok: true,
            error: None,
            duration_ms: 3,
            output_preview: Some("file contents".to_owned()),
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["tool_finished"]["tool"], "shell");
        assert!(value["tool_finished"].get("output_preview").is_none());
    }
}
