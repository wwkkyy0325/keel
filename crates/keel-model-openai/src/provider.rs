use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    CancelToken, ErrorCode, ModelChunk, ModelError, ModelProvider, ModelRequest, ModelStream,
    TokenUsage,
};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::wire::{build_request, FinishedCall, ToolCallAccumulator};

/// Configuration for an OpenAI-compatible endpoint. The API key is never
/// logged or serialized by this crate.
pub struct OpenAiConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

pub struct OpenAiProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    fallback_model: String,
}

impl OpenAiProvider {
    pub fn new(config: OpenAiConfig) -> Result<Self, ModelError> {
        if config.base_url.trim().is_empty() {
            return Err(ModelError::new(
                ErrorCode::InvalidConfig,
                "model base URL must not be empty",
                false,
            ));
        }
        let endpoint = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| ModelError::provider(error.to_string()))?;
        Ok(Self {
            client,
            endpoint,
            api_key: config.api_key,
            fallback_model: config.model,
        })
    }
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        let body = build_request(&request, &self.fallback_model);
        let response = tokio::select! {
            _ = cancel.cancelled() => {
                return Err(ModelError::new(ErrorCode::Cancelled, "model request cancelled", false));
            }
            result = self
                .client
                .post(&self.endpoint)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send() => result.map_err(|error| ModelError::provider(error.to_string()))?,
        };

        let status = response.status();
        if !status.is_success() {
            let retryable = status.is_server_error() || status.as_u16() == 429;
            let body = response.text().await.unwrap_or_default();
            return Err(ModelError::new(
                ErrorCode::ModelProvider,
                format!("provider returned {status}: {body}"),
                retryable,
            ));
        }

        let (tx, rx) = mpsc::channel(32);
        let cancel = cancel.clone();
        tokio::spawn(async move {
            read_stream(response, tx, cancel).await;
        });
        Ok(ModelStream::new(rx))
    }
}

enum EventOutcome {
    Continue,
    Done,
    Failed,
}

async fn read_stream(
    mut response: reqwest::Response,
    tx: mpsc::Sender<Result<ModelChunk, ModelError>>,
    cancel: CancelToken,
) {
    let mut buffer = String::new();
    let mut accumulator = ToolCallAccumulator::default();
    let mut usage = TokenUsage::default();
    let mut saw_finish = false;

    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return,
            result = response.chunk() => result,
        };
        match chunk {
            Ok(Some(chunk)) => {
                let text = String::from_utf8_lossy(&chunk).replace("\r\n", "\n");
                buffer.push_str(&text);
                while let Some(index) = buffer.find("\n\n") {
                    let event: String = buffer.drain(..index + 2).collect();
                    match process_event(&event, &tx, &mut accumulator, &mut usage, &mut saw_finish)
                        .await
                    {
                        EventOutcome::Continue => {}
                        EventOutcome::Done => {
                            emit_finished(&tx, &mut accumulator, usage).await;
                            return;
                        }
                        EventOutcome::Failed => return,
                    }
                }
            }
            Ok(None) => break,
            Err(error) => {
                let _ = tx
                    .send(Err(ModelError::stream_cut(error.to_string())))
                    .await;
                return;
            }
        }
    }

    if !buffer.trim().is_empty() {
        match process_event(&buffer, &tx, &mut accumulator, &mut usage, &mut saw_finish).await {
            EventOutcome::Done => {
                emit_finished(&tx, &mut accumulator, usage).await;
                return;
            }
            EventOutcome::Failed => return,
            EventOutcome::Continue => {}
        }
    }

    if saw_finish {
        emit_finished(&tx, &mut accumulator, usage).await;
    } else {
        let _ = tx
            .send(Err(ModelError::stream_cut(
                "model stream ended without [DONE] or a finish reason",
            )))
            .await;
    }
}

async fn process_event(
    event: &str,
    tx: &mpsc::Sender<Result<ModelChunk, ModelError>>,
    accumulator: &mut ToolCallAccumulator,
    usage: &mut TokenUsage,
    saw_finish: &mut bool,
) -> EventOutcome {
    for line in event.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            return EventOutcome::Done;
        }

        let value: Value = match serde_json::from_str(data) {
            Ok(value) => value,
            Err(error) => {
                let _ = tx
                    .send(Err(ModelError::new(
                        ErrorCode::InvalidResponse,
                        format!("invalid streaming JSON: {error}"),
                        false,
                    )))
                    .await;
                return EventOutcome::Failed;
            }
        };

        update_usage(&value, usage);

        let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            continue;
        };

        if let Some(delta) = choice.get("delta") {
            if let Some(content) = delta.get("content").and_then(Value::as_str) {
                if tx
                    .send(Ok(ModelChunk::TextDelta(content.to_owned())))
                    .await
                    .is_err()
                {
                    return EventOutcome::Failed;
                }
            }
            for key in ["reasoning_content", "reasoning"] {
                if let Some(thinking) = delta.get(key).and_then(Value::as_str) {
                    if tx
                        .send(Ok(ModelChunk::ThinkingDelta(thinking.to_owned())))
                        .await
                        .is_err()
                    {
                        return EventOutcome::Failed;
                    }
                }
            }
            if let Some(calls) = delta.get("tool_calls") {
                if let Err(error) = accumulator.apply(calls) {
                    let _ = tx.send(Err(error)).await;
                    return EventOutcome::Failed;
                }
            }
        }

        if choice
            .get("finish_reason")
            .is_some_and(|reason| !reason.is_null())
        {
            *saw_finish = true;
        }
    }
    EventOutcome::Continue
}

fn update_usage(value: &Value, usage: &mut TokenUsage) {
    let Some(reported) = value.get("usage") else {
        return;
    };
    if let Some(input) = reported.get("prompt_tokens").and_then(Value::as_u64) {
        usage.input_tokens = u32::try_from(input).unwrap_or(u32::MAX);
    }
    if let Some(output) = reported.get("completion_tokens").and_then(Value::as_u64) {
        usage.output_tokens = u32::try_from(output).unwrap_or(u32::MAX);
    }
}

async fn emit_finished(
    tx: &mpsc::Sender<Result<ModelChunk, ModelError>>,
    accumulator: &mut ToolCallAccumulator,
    usage: TokenUsage,
) {
    let calls = match std::mem::take(accumulator).finish() {
        Ok(calls) => calls,
        Err(error) => {
            let _ = tx.send(Err(error)).await;
            return;
        }
    };
    for call in calls {
        let chunk = match call {
            FinishedCall::Tool(call) => ModelChunk::ToolCall(call),
            FinishedCall::Interaction(request) => ModelChunk::Interaction(request),
        };
        if tx.send(Ok(chunk)).await.is_err() {
            return;
        }
    }
    let _ = tx.send(Ok(ModelChunk::Finished { usage })).await;
}
