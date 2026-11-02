use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    CancelToken, ErrorCode, ModelChunk, ModelError, ModelProvider, ModelRequest, ModelStream,
};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::wire::{build_request, StreamDecoder};

/// Value required by the Anthropic Messages API.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Configuration for an Anthropic-compatible endpoint. The API key is never
/// logged or serialized by this crate.
pub struct AnthropicConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// Retries for transient failures before a stream starts.
    pub max_retries: u32,
}

pub struct AnthropicProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    fallback_model: String,
    max_retries: u32,
}

impl AnthropicProvider {
    pub fn new(config: AnthropicConfig) -> Result<Self, ModelError> {
        if config.base_url.trim().is_empty() {
            return Err(ModelError::new(
                ErrorCode::InvalidConfig,
                "model base URL must not be empty",
                false,
            ));
        }
        let base = config.base_url.trim_end_matches('/');
        let endpoint = if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            format!("{base}/v1/messages")
        };
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| ModelError::provider(error.to_string()))?;
        Ok(Self {
            client,
            endpoint,
            api_key: config.api_key,
            fallback_model: config.model,
            max_retries: config.max_retries,
        })
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    async fn stream(
        &self,
        request: ModelRequest,
        cancel: &CancelToken,
    ) -> Result<ModelStream, ModelError> {
        let body = build_request(&request, &self.fallback_model);
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
                    .header("x-api-key", &self.api_key)
                    .header("anthropic-version", ANTHROPIC_VERSION)
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
    let mut decoder = StreamDecoder::default();

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
                    match process_event(&event, &tx, &mut decoder).await {
                        EventOutcome::Continue => {}
                        EventOutcome::Done => {
                            emit_finished(&tx, &mut decoder).await;
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
        match process_event(&buffer, &tx, &mut decoder).await {
            EventOutcome::Done => {
                emit_finished(&tx, &mut decoder).await;
                return;
            }
            EventOutcome::Failed => return,
            EventOutcome::Continue => {}
        }
    }

    if decoder.stopped() {
        emit_finished(&tx, &mut decoder).await;
    } else {
        let _ = tx
            .send(Err(ModelError::stream_cut(
                "model stream ended without message_stop",
            )))
            .await;
    }
}

async fn process_event(
    event: &str,
    tx: &mpsc::Sender<Result<ModelChunk, ModelError>>,
    decoder: &mut StreamDecoder,
) -> EventOutcome {
    for line in event.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
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
        let chunks = match decoder.apply(&value) {
            Ok(chunks) => chunks,
            Err(error) => {
                let _ = tx.send(Err(error)).await;
                return EventOutcome::Failed;
            }
        };
        for chunk in chunks {
            if tx.send(Ok(chunk)).await.is_err() {
                return EventOutcome::Failed;
            }
        }
    }
    if decoder.stopped() {
        EventOutcome::Done
    } else {
        EventOutcome::Continue
    }
}

async fn emit_finished(
    tx: &mpsc::Sender<Result<ModelChunk, ModelError>>,
    decoder: &mut StreamDecoder,
) {
    match decoder.finish() {
        Ok(chunks) => {
            for chunk in chunks {
                if tx.send(Ok(chunk)).await.is_err() {
                    return;
                }
            }
        }
        Err(error) => {
            let _ = tx.send(Err(error)).await;
        }
    }
}
