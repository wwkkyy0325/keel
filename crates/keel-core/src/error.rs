use std::fmt;
use std::time::Duration;

use thiserror::Error;

use crate::model::ModelError;
use crate::state::{TurnEvent, TurnState};

/// Stable error codes used at every kernel boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidArguments,
    NotFound,
    PermissionDenied,
    Timeout,
    Cancelled,
    Io,
    ModelProvider,
    StreamCut,
    InvalidResponse,
    ContextOverflow,
    InvalidProfile,
    InvalidTransition,
    InvalidConfig,
    Unsupported,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArguments => "invalid_arguments",
            Self::NotFound => "not_found",
            Self::PermissionDenied => "permission_denied",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Io => "io",
            Self::ModelProvider => "model_provider",
            Self::StreamCut => "stream_cut",
            Self::InvalidResponse => "invalid_response",
            Self::ContextOverflow => "context_overflow",
            Self::InvalidProfile => "invalid_profile",
            Self::InvalidTransition => "invalid_transition",
            Self::InvalidConfig => "invalid_config",
            Self::Unsupported => "unsupported",
            Self::Internal => "internal",
        }
    }
}

/// Typed tool error returned inside a tool result envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl ToolError {
    pub fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }

    pub fn invalid_arguments(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArguments, message, false)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message, false)
    }

    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::PermissionDenied, message, false)
    }

    pub fn timeout(limit: Duration) -> Self {
        Self::new(
            ErrorCode::Timeout,
            format!("tool call exceeded {} ms", limit.as_millis()),
            true,
        )
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorCode::Cancelled, "tool call cancelled", false)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Io, message, true)
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

/// Turn-level failure with a stable code, a retryable flag and a user message.
#[derive(Debug, Clone, Error)]
pub enum KernelError {
    #[error("invalid transition from {from} on {event}")]
    InvalidTransition { from: TurnState, event: TurnEvent },
    #[error("tool failure: {0}")]
    Tool(ToolError),
    #[error("model failure: {0}")]
    Model(ModelError),
    #[error("timeout during {stage} after {limit_ms} ms")]
    Timeout { stage: &'static str, limit_ms: u64 },
    #[error("cancelled during {stage}")]
    Cancelled { stage: &'static str },
    #[error("context overflow: used {used} tokens, budget {budget}")]
    ContextOverflow { used: u32, budget: u32 },
    #[error("invalid profile: {message}")]
    InvalidProfile { message: String },
    #[error("invalid configuration: {message}")]
    InvalidConfig { message: String },
    #[error("unsupported feature: {feature}")]
    Unsupported { feature: String },
    #[error("i/o failure: {message}")]
    Io { message: String },
    #[error("internal failure: {message}")]
    Internal { message: String },
}

impl KernelError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidTransition { .. } => ErrorCode::InvalidTransition,
            Self::Tool(error) => error.code,
            Self::Model(error) => error.code,
            Self::Timeout { .. } => ErrorCode::Timeout,
            Self::Cancelled { .. } => ErrorCode::Cancelled,
            Self::ContextOverflow { .. } => ErrorCode::ContextOverflow,
            Self::InvalidProfile { .. } => ErrorCode::InvalidProfile,
            Self::InvalidConfig { .. } => ErrorCode::InvalidConfig,
            Self::Unsupported { .. } => ErrorCode::Unsupported,
            Self::Io { .. } => ErrorCode::Io,
            Self::Internal { .. } => ErrorCode::Internal,
        }
    }

    pub fn retryable(&self) -> bool {
        match self {
            Self::Tool(error) => error.retryable,
            Self::Model(error) => error.retryable,
            Self::Timeout { .. } | Self::Io { .. } => true,
            Self::InvalidTransition { .. }
            | Self::Cancelled { .. }
            | Self::ContextOverflow { .. }
            | Self::InvalidProfile { .. }
            | Self::InvalidConfig { .. }
            | Self::Unsupported { .. }
            | Self::Internal { .. } => false,
        }
    }

    pub fn user_message(&self) -> String {
        self.to_string()
    }
}
