use std::fmt;

use serde::Serialize;

/// States of the turn state machine.
///
/// `Done`, `Error` and `Cancelled` are terminal. The remaining variants are
/// either active or auxiliary states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    Init,
    Observe,
    Solve,
    Act,
    Verify,
    Converge,
    Done,
    Error,
    Cancelled,
    Stalled,
    Escalating,
    WaitSubAgents,
    WaitInteraction,
    TerminalCommit,
}

impl TurnState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Error | Self::Cancelled)
    }
}

impl fmt::Display for TurnState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Init => "Init",
            Self::Observe => "Observe",
            Self::Solve => "Solve",
            Self::Act => "Act",
            Self::Verify => "Verify",
            Self::Converge => "Converge",
            Self::Done => "Done",
            Self::Error => "Error",
            Self::Cancelled => "Cancelled",
            Self::Stalled => "Stalled",
            Self::Escalating => "Escalating",
            Self::WaitSubAgents => "WaitSubAgents",
            Self::WaitInteraction => "WaitInteraction",
            Self::TerminalCommit => "TerminalCommit",
        };
        f.write_str(name)
    }
}

/// Events accepted by the turn state machine.
///
/// `Cancel` and `Fail` apply from every non-terminal state. `Fail` is a
/// kernel-internal event: it guarantees that an unexpected error still reaches
/// a terminal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnEvent {
    Ready,
    ToolCalls,
    DoneSignal,
    ThinkingOnly,
    /// The model stopped while the task list still had unfinished items.
    Continued,
    SubAgentsPending,
    UnverifiedHard,
    VerifyCapReached,
    StreamFailure,
    Overflow,
    VerifiedStalled,
    BudgetConverge,
    BudgetCommit,
    TurnsExhausted,
    WallClockLow,
    InteractionPending,
    InteractionResolved,
    InteractionDeferred,
    InteractionEndTurn,
    Cancel,
    Fail,
}

impl fmt::Display for TurnEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Ready => "Ready",
            Self::ToolCalls => "ToolCalls",
            Self::DoneSignal => "DoneSignal",
            Self::ThinkingOnly => "ThinkingOnly",
            Self::Continued => "Continued",
            Self::SubAgentsPending => "SubAgentsPending",
            Self::UnverifiedHard => "UnverifiedHard",
            Self::VerifyCapReached => "VerifyCapReached",
            Self::StreamFailure => "StreamFailure",
            Self::Overflow => "Overflow",
            Self::VerifiedStalled => "VerifiedStalled",
            Self::BudgetConverge => "BudgetConverge",
            Self::BudgetCommit => "BudgetCommit",
            Self::TurnsExhausted => "TurnsExhausted",
            Self::WallClockLow => "WallClockLow",
            Self::InteractionPending => "InteractionPending",
            Self::InteractionResolved => "InteractionResolved",
            Self::InteractionDeferred => "InteractionDeferred",
            Self::InteractionEndTurn => "InteractionEndTurn",
            Self::Cancel => "Cancel",
            Self::Fail => "Fail",
        };
        f.write_str(name)
    }
}
