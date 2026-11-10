//! Core types, ports and contracts for the Keel agent kernel.
//!
//! `keel-core` depends on no other Keel crate. It defines the data that
//! crosses kernel boundaries: identifiers, messages, the tool protocol, model
//! wire types, policy questions, events and errors.

/// Kernel version, pinned by a product through the release tag.
pub const KERNEL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Version of the persisted event shape.
///
/// Adding a field keeps the version; renaming or removing one, or changing a
/// field's meaning, bumps it. A host that stores events records this value so
/// a later reader can tell which shape it is looking at.
pub const EVENT_FORMAT_VERSION: u32 = 1;

pub mod answer;
pub mod cancel;
pub mod context;
pub mod error;
pub mod event;
pub mod ids;
pub mod interaction;
pub mod message;
pub mod model;
pub mod permission;
pub mod policy;
pub mod secret;
pub mod session;
pub mod state;
pub mod todo;
pub mod tool;
pub mod verification;

pub use answer::{Answer, Change, PendingItem};
pub use cancel::CancelToken;
pub use context::{
    estimate_tokens, BuildCtx, ContextMessage, ContextSnapshot, ContextStage, Fragment,
};
pub use error::{ErrorCode, KernelError, ToolError};
pub use event::{Event, EventSink, SubAgentStep};
pub use ids::{InteractionId, ToolCallId, TurnId};
pub use interaction::{
    GrantScope, InteractionKind, InteractionOption, InteractionOutcome, InteractionPolicy,
    InteractionPort, InteractionRequest, InteractionResponse, OnTimeout,
};
pub use message::{ImagePart, Message, MessageRole, ESTIMATED_IMAGE_TOKENS};
pub use model::{
    ModelChunk, ModelError, ModelProvider, ModelRequest, ModelStream, TokenUsage, ToolCallRequest,
};
pub use permission::{
    resolve_path, simplify_path, AccessKind, PermissionDecision, PermissionGate, PermissionLevel,
    PermissionRequest,
};
pub use policy::{
    CommitCtx, CommitDecision, CompactionStep, ContextRecipe, MemoryMode, MemoryPolicy, ModelRole,
    ModelRoute, Policy, PromotionMode, ReasoningLevel, VerificationMode,
};
pub use secret::{detect_secret, redact_secrets, SecretKind};
pub use session::{
    CompressionSkip, Entry, EntrySnapshot, Fact, FactScope, RecalledSegment, Segment,
    SegmentChange, SegmentRecord, SegmentSnapshot, SegmentState, Session, SessionSnapshot,
    SessionSummary, ToolSummary, TurnRecord, SNAPSHOT_VERSION,
};
pub use state::{TurnEvent, TurnState};
pub use todo::{
    TodoError, TodoItem, TodoList, TodoSource, TodoState, TodoStatus, TODO_MAX_ITEMS,
    TODO_MAX_TEXT_CHARS,
};
pub use tool::{
    ArgValue, ParamKind, ParamSpec, RiskLevel, Tool, ToolArgs, ToolCtx, ToolOutcome, ToolResult,
    ToolSchema,
};
pub use verification::{AcceptanceItem, Evidence, EvidenceOutcome, VerificationReport, VerifySpec};
