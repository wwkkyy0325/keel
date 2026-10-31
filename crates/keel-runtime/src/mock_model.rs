use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use keel_core::{
    estimate_tokens, CancelToken, InteractionRequest, ModelChunk, ModelError, ModelProvider,
    ModelRequest, ModelStream, TokenUsage, ToolArgs,
};
use tokio::sync::mpsc;

/// One scripted model turn.
#[derive(Debug, Clone)]
pub enum MockStep {
    /// Emit one text delta and finish.
    Text(String),
    /// Emit one thinking delta and finish.
    Thinking(String),
    /// Emit one tool call and finish.
    ToolCall { name: String, args: ToolArgs },
    /// Emit one interaction request and finish.
    Interaction(InteractionRequest),
    /// Fail the stream request.
    Error(ModelError),
    /// Emit text, then close the stream without a finish chunk.
    CutAfter(String),
    /// Keep the stream open forever, to exercise timeouts and cancellation.
    Hang,
}

/// Scripted model used by M1. No network and no real inference.
pub struct MockModel {
    steps: Mutex<VecDeque<MockStep>>,
    repeat: bool,
    calls: AtomicU64,
    held: Mutex<Vec<mpsc::Sender<Result<ModelChunk, ModelError>>>>,
}

impl MockModel {
    pub fn new(steps: Vec<MockStep>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            repeat: false,
            calls: AtomicU64::new(0),
            held: Mutex::new(Vec::new()),
        }
    }

    /// Repeats the same script for every model call.
    pub fn repeating(steps: Vec<MockStep>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            repeat: true,
            calls: AtomicU64::new(0),
            held: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl ModelProvider for MockModel {
    async fn stream(
        &self,
        request: ModelRequest,
        _cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        let step = {
            let mut steps = self
                .steps
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if self.repeat {
                steps.front().cloned()
            } else {
                steps.pop_front()
            }
        };
        let Some(step) = step else {
            return Err(ModelError::stream_cut("mock script is exhausted"));
        };
        self.calls.fetch_add(1, Ordering::Relaxed);

        match step {
            MockStep::Error(error) => Err(error),
            MockStep::Hang => {
                let (tx, rx) = mpsc::channel(1);
                self.held
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(tx);
                Ok(ModelStream::new(rx))
            }
            MockStep::Text(text) => {
                let usage = usage_for(&request, &text);
                let (tx, rx) = mpsc::channel(4);
                send(&tx, ModelChunk::TextDelta(text)).await?;
                send(&tx, ModelChunk::Finished { usage }).await?;
                Ok(ModelStream::new(rx))
            }
            MockStep::Thinking(thought) => {
                let usage = usage_for(&request, "");
                let (tx, rx) = mpsc::channel(4);
                send(&tx, ModelChunk::ThinkingDelta(thought)).await?;
                send(&tx, ModelChunk::Finished { usage }).await?;
                Ok(ModelStream::new(rx))
            }
            MockStep::ToolCall { name, args } => {
                let usage = usage_for(&request, "");
                let (tx, rx) = mpsc::channel(4);
                send(
                    &tx,
                    ModelChunk::ToolCall(keel_core::ToolCallRequest {
                        id: keel_core::ToolCallId::next(),
                        name,
                        args,
                    }),
                )
                .await?;
                send(&tx, ModelChunk::Finished { usage }).await?;
                Ok(ModelStream::new(rx))
            }
            MockStep::Interaction(interaction) => {
                let usage = usage_for(&request, "");
                let (tx, rx) = mpsc::channel(4);
                send(&tx, ModelChunk::Interaction(interaction)).await?;
                send(&tx, ModelChunk::Finished { usage }).await?;
                Ok(ModelStream::new(rx))
            }
            MockStep::CutAfter(text) => {
                let (tx, rx) = mpsc::channel(4);
                send(&tx, ModelChunk::TextDelta(text)).await?;
                // Dropping the sender closes the stream without `Finished`.
                drop(tx);
                Ok(ModelStream::new(rx))
            }
        }
    }
}

async fn send(
    tx: &mpsc::Sender<Result<ModelChunk, ModelError>>,
    chunk: ModelChunk,
) -> Result<(), ModelError> {
    tx.send(Ok(chunk))
        .await
        .map_err(|_| ModelError::internal("mock stream receiver dropped"))
}

fn usage_for(request: &ModelRequest, output: &str) -> TokenUsage {
    let input_tokens = request.messages.iter().fold(0u32, |total, message| {
        total.saturating_add(estimate_tokens(&message.content))
    });
    TokenUsage {
        input_tokens,
        output_tokens: estimate_tokens(output),
    }
}
