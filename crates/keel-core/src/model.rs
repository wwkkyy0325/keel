use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::cancel::CancelToken;
use crate::error::ErrorCode;
use crate::ids::ToolCallId;
use crate::interaction::InteractionRequest;
use crate::message::Message;
use crate::policy::ModelRoute;
use crate::tool::{ToolArgs, ToolSchema};

/// Token usage reported by a model call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// One model request. Messages are shared with `Arc` so the driver does not
/// clone the full history per call.
#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub messages: Vec<Arc<Message>>,
    pub tools: Vec<ToolSchema>,
    pub thinking_budget: u32,
    pub max_output_tokens: u32,
    pub route: ModelRoute,
}

/// One tool call requested by the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallRequest {
    pub id: ToolCallId,
    pub name: String,
    pub args: ToolArgs,
}

/// One chunk of a model stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelChunk {
    TextDelta(String),
    ThinkingDelta(String),
    ToolCall(ToolCallRequest),
    /// The model asks for user input. This is a control-plane chunk, never a
    /// domain tool call.
    Interaction(InteractionRequest),
    Finished {
        usage: TokenUsage,
    },
}

/// Model boundary failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl ModelError {
    pub fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }

    pub fn provider(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ModelProvider, message, true)
    }

    pub fn stream_cut(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::StreamCut, message, true)
    }

    pub fn overflow(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ContextOverflow, message, false)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Timeout, message, true)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message, false)
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

/// Streaming model output. The driver pulls chunks until `Finished` or the
/// stream closes.
pub struct ModelStream {
    rx: mpsc::Receiver<Result<ModelChunk, ModelError>>,
}

impl fmt::Debug for ModelStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelStream").finish_non_exhaustive()
    }
}

impl ModelStream {
    pub fn new(rx: mpsc::Receiver<Result<ModelChunk, ModelError>>) -> Self {
        Self { rx }
    }

    pub async fn recv(&mut self) -> Option<Result<ModelChunk, ModelError>> {
        self.rx.recv().await
    }
}

/// Model provider port. Streaming is the primary path.
///
/// The cancellation token is part of the contract so a provider can abort its
/// own connection or background work; dropping the stream is not enough for
/// every wire protocol.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError>;
}
