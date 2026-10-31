use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TURN_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TOOL_CALL_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_INTERACTION_ID: AtomicU64 = AtomicU64::new(1);

/// Identifier for one turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TurnId(u64);

impl TurnId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Allocates a process-wide unique turn identifier.
    pub fn next() -> Self {
        Self(NEXT_TURN_ID.fetch_add(1, Ordering::Relaxed))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TurnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "turn-{}", self.0)
    }
}

/// Identifier for one tool call inside a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolCallId(u64);

impl ToolCallId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Allocates a process-wide unique tool-call identifier.
    pub fn next() -> Self {
        Self(NEXT_TOOL_CALL_ID.fetch_add(1, Ordering::Relaxed))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ToolCallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "call-{}", self.0)
    }
}

/// Identifier for one user interaction inside a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InteractionId(u64);

impl InteractionId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Allocates a process-wide unique interaction identifier.
    pub fn next() -> Self {
        Self(NEXT_INTERACTION_ID.fetch_add(1, Ordering::Relaxed))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for InteractionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "interaction-{}", self.0)
    }
}
