//! Core types, ports and contracts for the Keel agent kernel.
//!
//! `keel-core` depends on no other Keel crate. It defines the data that
//! crosses kernel boundaries: identifiers, messages, the tool protocol, model
//! wire types, policy questions, events and errors.

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
pub mod tool;
pub mod verification;

pub use answer::{Answer, Change, PendingItem};
pub use cancel::CancelToken;
pub use context::{estimate_tokens, BuildCtx, ContextStage, Fragment};
pub use error::{ErrorCode, KernelError, ToolError};
pub use event::{Event, EventSink};
pub use ids::{InteractionId, ToolCallId, TurnId};
pub use interaction::{
    GrantScope, InteractionKind, InteractionOption, InteractionOutcome, InteractionPolicy,
    InteractionPort, InteractionRequest, InteractionResponse, OnTimeout,
};
pub use message::{Message, MessageRole};
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
    CompressionSkip, Entry, Segment, SegmentChange, SegmentRecord, SegmentState, Session,
    ToolSummary, TurnRecord,
};
pub use state::{TurnEvent, TurnState};
pub use tool::{
    ArgValue, ParamKind, ParamSpec, RiskLevel, Tool, ToolArgs, ToolCtx, ToolOutcome, ToolResult,
    ToolSchema,
};
pub use verification::{AcceptanceItem, Evidence, EvidenceOutcome, VerificationReport, VerifySpec};
