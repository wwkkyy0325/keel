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
    /// Whether this endpoint accepts `reasoning_effort`.
    pub supports_reasoning: bool,
    /// Retries for transient failures before a stream starts.
    pub max_retries: u32,
}

pub struct OpenAiProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    fallback_model: String,
    supports_reasoning: bool,
    max_retries: u32,
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
            supports_reasoning: config.supports_reasoning,
            max_retries: config.max_retries,
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
        let reasoning = self.supports_reasoning.then_some(request.reasoning);
        let body = build_request(&request, &self.fallback_model, reasoning);
        let attempts = self.max_retries.saturating_add(1);

        for attempt in 0..attempts {
            if cancel.is_cancelled() {
                return Err(ModelError::new(
                    ErrorCode::Cancelled,
                    "model request cancelled",
                    false,
                ));
            }

            let response = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(ModelError::new(
                        ErrorCode::Cancelled,
                        "model request cancelled",
                        false,
                    ));
                }
                result = self
                    .client
                    .post(&self.endpoint)
                    .bearer_auth(&self.api_key)
                    .json(&body)
                    .send() => result,
            };

            match response {
                Ok(response) if response.status().is_success() => {
                    let (tx, rx) = mpsc::channel(32);
                    let cancel = cancel.clone();
                    tokio::spawn(async move {
                        read_stream(response, tx, cancel).await;
                    });
                    return Ok(ModelStream::new(rx));
                }
                Ok(response) => {
                    let status = response.status();
                    let retryable = status.is_server_error() || status.as_u16() == 429;
                    let body = response.text().await.unwrap_or_default();
                    let error = ModelError::new(
                        ErrorCode::ModelProvider,
                        format!("provider returned {status}: {body}"),
                        retryable,
                    );
                    if retryable && attempt + 1 < attempts {
                        backoff(cancel, attempt).await?;
                        continue;
                    }
                    return Err(error);
                }
                Err(error) => {
                    let retryable = error.is_timeout() || error.is_connect() || error.is_request();
                    let error = ModelError::provider(error.to_string());
                    if retryable && attempt + 1 < attempts {
                        backoff(cancel, attempt).await?;
                        continue;
                    }
                    return Err(error);
                }
            }
        }

        Err(ModelError::provider("model request failed after retries"))
    }
}

async fn backoff(cancel: &CancelToken, attempt: u32) -> Result<(), ModelError> {
    let delay = Duration::from_millis(200u64.saturating_mul(1u64 << attempt.min(4)));
    tokio::select! {
        _ = cancel.cancelled() => Err(ModelError::new(
            ErrorCode::Cancelled,
            "model request cancelled",
            false,
        )),
        _ = tokio::time::sleep(delay) => Ok(()),
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
    let mut pending: Vec<u8> = Vec::new();
    let mut accumulator = ToolCallAccumulator::default();
    let mut usage = TokenUsage::default();
    let mut saw_finish = false;
    let mut finish_reason: Option<String> = None;

    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return,
            result = response.chunk() => result,
        };
        match chunk {
            Ok(Some(chunk)) => {
                buffer.push_str(&decode_chunk(&mut pending, &chunk));
                if buffer.contains('\r') {
                    buffer = buffer.replace("\r\n", "\n");
                }
                while let Some(index) = buffer.find("\n\n") {
                    let event: String = buffer.drain(..index + 2).collect();
                    match process_event(
                        &event,
                        &tx,
                        &mut accumulator,
                        &mut usage,
                        &mut saw_finish,
                        &mut finish_reason,
                    )
                    .await
                    {
                        EventOutcome::Continue => {}
                        EventOutcome::Done => {
                            emit_finished(&tx, &mut accumulator, usage, finish_reason.as_deref())
                                .await;
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

    if !pending.is_empty() {
        buffer.push_str(&String::from_utf8_lossy(&pending));
        buffer = buffer.replace("\r\n", "\n");
    }

    if !buffer.trim().is_empty() {
        match process_event(
            &buffer,
            &tx,
            &mut accumulator,
            &mut usage,
            &mut saw_finish,
            &mut finish_reason,
        )
        .await
        {
            EventOutcome::Done => {
                emit_finished(&tx, &mut accumulator, usage, finish_reason.as_deref()).await;
                return;
            }
            EventOutcome::Failed => return,
            EventOutcome::Continue => {}
        }
    }

    if saw_finish {
        emit_finished(&tx, &mut accumulator, usage, finish_reason.as_deref()).await;
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
    finish_reason: &mut Option<String>,
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

        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            *saw_finish = true;
            *finish_reason = Some(reason.to_owned());
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
    let cached = reported
        .get("prompt_cache_hit_tokens")
        .and_then(Value::as_u64)
        .or_else(|| {
            reported
                .get("prompt_tokens_details")
                .and_then(|details| details.get("cached_tokens"))
                .and_then(Value::as_u64)
        });
    if let Some(cached) = cached {
        usage.cached_input_tokens = u32::try_from(cached).unwrap_or(u32::MAX);
    }
}

async fn emit_finished(
    tx: &mpsc::Sender<Result<ModelChunk, ModelError>>,
    accumulator: &mut ToolCallAccumulator,
    usage: TokenUsage,
    finish_reason: Option<&str>,
) {
    // A response that hit the output limit is cut wherever it was, usually
    // inside a tool call's JSON. Say so instead of reporting the parse error.
    if finish_reason == Some("length") {
        let _ = tx
            .send(Err(ModelError::new(
                ErrorCode::InvalidResponse,
                "the model response hit the output token limit before it finished;                  the answer or tool call is incomplete",
                true,
            )))
            .await;
        return;
    }
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
            FinishedCall::Malformed { name, message } => {
                ModelChunk::MalformedToolCall { name, message }
            }
            FinishedCall::Interaction(request) => ModelChunk::Interaction(request),
        };
        if tx.send(Ok(chunk)).await.is_err() {
            return;
        }
    }
    let _ = tx.send(Ok(ModelChunk::Finished { usage })).await;
}

/// Decodes a network chunk without corrupting a character whose bytes span
/// two chunks. An incomplete trailing sequence waits for the next chunk;
/// genuinely invalid bytes become U+FFFD.
fn decode_chunk(pending: &mut Vec<u8>, chunk: &[u8]) -> String {
    pending.extend_from_slice(chunk);
    let mut text = String::with_capacity(pending.len());
    loop {
        let error = match std::str::from_utf8(pending) {
            Ok(valid) => {
                text.push_str(valid);
                pending.clear();
                return text;
            }
            Err(error) => error,
        };
        let valid = error.valid_up_to();
        if let Ok(prefix) = std::str::from_utf8(&pending[..valid]) {
            text.push_str(prefix);
        }
        match error.error_len() {
            Some(length) => {
                text.push('\u{FFFD}');
                pending.drain(..valid + length);
            }
            None => {
                pending.drain(..valid);
                return text;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::decode_chunk;

    #[test]
    fn a_character_split_across_chunks_stays_whole() {
        let text = "让方知道";
        let bytes = text.as_bytes();
        let mut pending = Vec::new();
        let first = decode_chunk(&mut pending, &bytes[..4]);
        let second = decode_chunk(&mut pending, &bytes[4..]);
        assert_eq!(format!("{first}{second}"), text);
        assert!(pending.is_empty());
    }

    #[test]
    fn an_invalid_byte_becomes_a_replacement_character() {
        let mut pending = Vec::new();
        let decoded = decode_chunk(&mut pending, &[b'a', 0xff, b'b']);
        assert_eq!(decoded, "a\u{FFFD}b");
        assert!(pending.is_empty());
    }
}
