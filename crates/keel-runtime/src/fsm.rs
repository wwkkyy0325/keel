use keel_core::{KernelError, TurnEvent, TurnState};

/// Declared transitions of the turn state machine.
///
/// `Cancel` and `Fail` are handled from every non-terminal state in
/// [`transition`]; they are not listed here.
pub const TRANSITIONS: &[(TurnState, TurnEvent, TurnState)] = &[
    (TurnState::Init, TurnEvent::Ready, TurnState::Observe),
    (TurnState::Observe, TurnEvent::Ready, TurnState::Solve),
    (TurnState::Observe, TurnEvent::Overflow, TurnState::Error),
    (TurnState::Solve, TurnEvent::ToolCalls, TurnState::Act),
    (TurnState::Solve, TurnEvent::DoneSignal, TurnState::Verify),
    (TurnState::Solve, TurnEvent::ThinkingOnly, TurnState::Solve),
    (
        TurnState::Solve,
        TurnEvent::SubAgentsPending,
        TurnState::WaitSubAgents,
    ),
    (TurnState::Solve, TurnEvent::StreamFailure, TurnState::Error),
    (TurnState::Solve, TurnEvent::Overflow, TurnState::Error),
    (
        TurnState::Solve,
        TurnEvent::WallClockLow,
        TurnState::Converge,
    ),
    (
        TurnState::Solve,
        TurnEvent::TurnsExhausted,
        TurnState::Converge,
    ),
    (
        TurnState::Solve,
        TurnEvent::InteractionPending,
        TurnState::WaitInteraction,
    ),
    (
        TurnState::WaitInteraction,
        TurnEvent::InteractionResolved,
        TurnState::Solve,
    ),
    (
        TurnState::WaitInteraction,
        TurnEvent::InteractionDeferred,
        TurnState::Solve,
    ),
    (
        TurnState::WaitInteraction,
        TurnEvent::InteractionEndTurn,
        TurnState::Converge,
    ),
    (TurnState::Act, TurnEvent::Ready, TurnState::Solve),
    (
        TurnState::Act,
        TurnEvent::TurnsExhausted,
        TurnState::Converge,
    ),
    (
        TurnState::Verify,
        TurnEvent::DoneSignal,
        TurnState::Converge,
    ),
    (
        TurnState::Verify,
        TurnEvent::UnverifiedHard,
        TurnState::Escalating,
    ),
    (
        TurnState::Verify,
        TurnEvent::VerifyCapReached,
        TurnState::Converge,
    ),
    (
        TurnState::Verify,
        TurnEvent::VerifiedStalled,
        TurnState::Stalled,
    ),
    (TurnState::Converge, TurnEvent::DoneSignal, TurnState::Done),
    (
        TurnState::Converge,
        TurnEvent::BudgetConverge,
        TurnState::Done,
    ),
    (
        TurnState::Converge,
        TurnEvent::BudgetCommit,
        TurnState::TerminalCommit,
    ),
    (
        TurnState::Stalled,
        TurnEvent::DoneSignal,
        TurnState::Converge,
    ),
    (
        TurnState::Stalled,
        TurnEvent::VerifyCapReached,
        TurnState::Converge,
    ),
    (TurnState::Stalled, TurnEvent::Ready, TurnState::Solve),
    (
        TurnState::Escalating,
        TurnEvent::DoneSignal,
        TurnState::Converge,
    ),
    (
        TurnState::Escalating,
        TurnEvent::VerifyCapReached,
        TurnState::Converge,
    ),
    (TurnState::Escalating, TurnEvent::Ready, TurnState::Solve),
    (TurnState::WaitSubAgents, TurnEvent::Ready, TurnState::Solve),
    (
        TurnState::TerminalCommit,
        TurnEvent::DoneSignal,
        TurnState::Done,
    ),
];

/// Applies one event to a state. Invalid transitions are errors, never
/// silent no-ops.
pub fn transition(from: TurnState, event: TurnEvent) -> Result<TurnState, KernelError> {
    if from.is_terminal() {
        return Err(KernelError::InvalidTransition { from, event });
    }

    let next = match (from, event) {
        (TurnState::Init, TurnEvent::Ready) => TurnState::Observe,
        (TurnState::Observe, TurnEvent::Ready) => TurnState::Solve,
        (TurnState::Observe, TurnEvent::Overflow) => TurnState::Error,
        (TurnState::Solve, TurnEvent::ToolCalls) => TurnState::Act,
        (TurnState::Solve, TurnEvent::DoneSignal) => TurnState::Verify,
        (TurnState::Solve, TurnEvent::ThinkingOnly) => TurnState::Solve,
        (TurnState::Solve, TurnEvent::SubAgentsPending) => TurnState::WaitSubAgents,
        (TurnState::Solve, TurnEvent::StreamFailure) => TurnState::Error,
        (TurnState::Solve, TurnEvent::Overflow) => TurnState::Error,
        (TurnState::Solve, TurnEvent::WallClockLow) => TurnState::Converge,
        (TurnState::Solve, TurnEvent::TurnsExhausted) => TurnState::Converge,
        (TurnState::Solve, TurnEvent::InteractionPending) => TurnState::WaitInteraction,
        (TurnState::WaitInteraction, TurnEvent::InteractionResolved) => TurnState::Solve,
        (TurnState::WaitInteraction, TurnEvent::InteractionDeferred) => TurnState::Solve,
        (TurnState::WaitInteraction, TurnEvent::InteractionEndTurn) => TurnState::Converge,
        (TurnState::Act, TurnEvent::Ready) => TurnState::Solve,
        (TurnState::Act, TurnEvent::TurnsExhausted) => TurnState::Converge,
        (TurnState::Verify, TurnEvent::DoneSignal) => TurnState::Converge,
        (TurnState::Verify, TurnEvent::UnverifiedHard) => TurnState::Escalating,
        (TurnState::Verify, TurnEvent::VerifyCapReached) => TurnState::Converge,
        (TurnState::Verify, TurnEvent::VerifiedStalled) => TurnState::Stalled,
        (TurnState::Converge, TurnEvent::DoneSignal) => TurnState::Done,
        (TurnState::Converge, TurnEvent::BudgetConverge) => TurnState::Done,
        (TurnState::Converge, TurnEvent::BudgetCommit) => TurnState::TerminalCommit,
        (TurnState::Stalled, TurnEvent::DoneSignal) => TurnState::Converge,
        (TurnState::Stalled, TurnEvent::VerifyCapReached) => TurnState::Converge,
        (TurnState::Stalled, TurnEvent::Ready) => TurnState::Solve,
        (TurnState::Escalating, TurnEvent::DoneSignal) => TurnState::Converge,
        (TurnState::Escalating, TurnEvent::VerifyCapReached) => TurnState::Converge,
        (TurnState::Escalating, TurnEvent::Ready) => TurnState::Solve,
        (TurnState::WaitSubAgents, TurnEvent::Ready) => TurnState::Solve,
        (TurnState::TerminalCommit, TurnEvent::DoneSignal) => TurnState::Done,
        (_, TurnEvent::Cancel) => TurnState::Cancelled,
        (_, TurnEvent::Fail) => TurnState::Error,
        _ => return Err(KernelError::InvalidTransition { from, event }),
    };

    Ok(next)
}

/// Mutable state holder for one turn.
#[derive(Debug, Clone)]
pub struct TurnFsm {
    state: TurnState,
}

impl TurnFsm {
    pub fn new() -> Self {
        Self {
            state: TurnState::Init,
        }
    }

    pub fn state(&self) -> TurnState {
        self.state
    }

    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    pub fn apply(&mut self, event: TurnEvent) -> Result<TurnState, KernelError> {
        let next = transition(self.state, event)?;
        self.state = next;
        Ok(next)
    }
}

impl Default for TurnFsm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [TurnState; 14] = [
        TurnState::Init,
        TurnState::Observe,
        TurnState::Solve,
        TurnState::Act,
        TurnState::Verify,
        TurnState::Converge,
        TurnState::Done,
        TurnState::Error,
        TurnState::Cancelled,
        TurnState::Stalled,
        TurnState::Escalating,
        TurnState::WaitSubAgents,
        TurnState::WaitInteraction,
        TurnState::TerminalCommit,
    ];

    const ALL_EVENTS: [TurnEvent; 20] = [
        TurnEvent::Ready,
        TurnEvent::ToolCalls,
        TurnEvent::DoneSignal,
        TurnEvent::ThinkingOnly,
        TurnEvent::SubAgentsPending,
        TurnEvent::UnverifiedHard,
        TurnEvent::VerifyCapReached,
        TurnEvent::StreamFailure,
        TurnEvent::Overflow,
        TurnEvent::VerifiedStalled,
        TurnEvent::BudgetConverge,
        TurnEvent::BudgetCommit,
        TurnEvent::TurnsExhausted,
        TurnEvent::WallClockLow,
        TurnEvent::InteractionPending,
        TurnEvent::InteractionResolved,
        TurnEvent::InteractionDeferred,
        TurnEvent::InteractionEndTurn,
        TurnEvent::Cancel,
        TurnEvent::Fail,
    ];

    fn non_terminal_states() -> impl Iterator<Item = TurnState> {
        ALL_STATES.into_iter().filter(|state| !state.is_terminal())
    }

    #[test]
    fn every_declared_transition_is_accepted() {
        for &(from, event, expected) in TRANSITIONS {
            let actual = transition(from, event)
                .unwrap_or_else(|error| panic!("{from} on {event} should be accepted: {error}"));
            assert_eq!(actual, expected, "{from} on {event}");
        }
    }

    #[test]
    fn cancel_reaches_cancelled_from_every_non_terminal_state() {
        for from in non_terminal_states() {
            assert_eq!(
                transition(from, TurnEvent::Cancel).unwrap(),
                TurnState::Cancelled,
                "{from}"
            );
        }
    }

    #[test]
    fn fail_reaches_error_from_every_non_terminal_state() {
        for from in non_terminal_states() {
            assert_eq!(
                transition(from, TurnEvent::Fail).unwrap(),
                TurnState::Error,
                "{from}"
            );
        }
    }

    #[test]
    fn terminal_states_reject_every_event() {
        for from in [TurnState::Done, TurnState::Error, TurnState::Cancelled] {
            for event in ALL_EVENTS {
                assert!(
                    transition(from, event).is_err(),
                    "{from} must reject {event}"
                );
            }
        }
    }

    #[test]
    fn undeclared_transitions_are_rejected() {
        assert!(transition(TurnState::Act, TurnEvent::DoneSignal).is_err());
        assert!(transition(TurnState::Converge, TurnEvent::Ready).is_err());
        assert!(transition(TurnState::Init, TurnEvent::ToolCalls).is_err());
    }

    #[test]
    fn fsm_starts_in_init_and_reaches_terminal() {
        let mut fsm = TurnFsm::new();
        assert_eq!(fsm.state(), TurnState::Init);
        fsm.apply(TurnEvent::Ready).unwrap();
        fsm.apply(TurnEvent::Ready).unwrap();
        fsm.apply(TurnEvent::DoneSignal).unwrap();
        fsm.apply(TurnEvent::DoneSignal).unwrap();
        fsm.apply(TurnEvent::DoneSignal).unwrap();
        assert!(fsm.is_terminal());
        assert_eq!(fsm.state(), TurnState::Done);
    }
}
