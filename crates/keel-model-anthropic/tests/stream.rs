use std::sync::Arc;
use std::time::Duration;

use keel_core::{
    CancelToken, Message, ModelChunk, ModelProvider, ModelRequest, ModelRoute, ReasoningLevel,
};
use keel_model_anthropic::{AnthropicConfig, AnthropicProvider};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_server(body: String, captured: Arc<tokio::sync::Mutex<Vec<String>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        let headers_end = loop {
            let read = match socket.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(read) => read,
            };
            request.extend_from_slice(&buffer[..read]);
            if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..headers_end]).into_owned();
        captured.lock().await.push(headers.clone());

        let lower = headers.to_ascii_lowercase();
        let content_length = lower
            .lines()
            .find_map(|line| {
                line.strip_prefix("content-length:")
                    .map(|value| value.trim().parse::<usize>().unwrap_or(0))
            })
            .unwrap_or(0);
        let mut remaining =
            content_length.saturating_sub(request.len().saturating_sub(headers_end));
        while remaining > 0 {
            let read = match socket.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            remaining = remaining.saturating_sub(read);
        }

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
        let _ = socket.shutdown().await;
    });
    format!("http://{address}")
}

fn request() -> ModelRequest {
    ModelRequest {
        messages: vec![Arc::new(Message::new(
            keel_core::MessageRole::User,
            "hello",
        ))],
        tools: Vec::new(),
        control_tool: false,
        thinking_budget: 0,
        max_output_tokens: 64,
        route: ModelRoute {
            provider: "anthropic-compatible".to_owned(),
            model: "claude-test".to_owned(),
        },
        reasoning: ReasoningLevel::Low,
    }
}

#[tokio::test]
async fn the_provider_streams_text_usage_and_headers() {
    let body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12,\"cache_read_input_tokens\":3}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    )
    .to_owned();
    let captured = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let url = spawn_server(body, captured.clone()).await;
    let provider = AnthropicProvider::new(AnthropicConfig {
        base_url: url,
        api_key: "test-key".to_owned(),
        model: "fallback".to_owned(),
        max_retries: 0,
    })
    .unwrap();

    let mut stream = provider
        .stream(request(), &CancelToken::new())
        .await
        .unwrap();
    let mut chunks = Vec::new();
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(5), stream.recv())
        .await
        .unwrap()
    {
        chunks.push(chunk.unwrap());
    }

    assert_eq!(chunks[0], ModelChunk::TextDelta("Hello".to_owned()));
    match &chunks[1] {
        ModelChunk::Finished { usage } => {
            assert_eq!(usage.input_tokens, 12);
            assert_eq!(usage.output_tokens, 1);
            assert_eq!(usage.cached_input_tokens, 3);
        }
        other => panic!("unexpected chunk: {other:?}"),
    }
    assert_eq!(chunks.len(), 2);

    let headers = captured.lock().await.join("\n");
    assert!(headers.starts_with("POST /v1/messages HTTP/1.1"));
    assert!(headers.to_ascii_lowercase().contains("x-api-key: test-key"));
    assert!(headers
        .to_ascii_lowercase()
        .contains("anthropic-version: 2023-06-01"));
}

#[tokio::test]
async fn an_output_limit_stop_is_reported_as_such() {
    let body = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"output_tokens\":2}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    )
    .to_owned();
    let captured = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let url = spawn_server(body, captured).await;
    let provider = AnthropicProvider::new(AnthropicConfig {
        base_url: url,
        api_key: "test-key".to_owned(),
        model: "fallback".to_owned(),
        max_retries: 0,
    })
    .unwrap();

    let mut stream = provider
        .stream(request(), &CancelToken::new())
        .await
        .unwrap();
    let mut chunks = Vec::new();
    while let Ok(Some(chunk)) = tokio::time::timeout(Duration::from_secs(5), stream.recv()).await {
        chunks.push(chunk);
    }

    assert!(chunks.iter().any(|chunk| matches!(
        chunk,
        Ok(ModelChunk::TextDelta(text)) if text == "Hel"
    )));
    let error = chunks
        .iter()
        .find_map(|chunk| chunk.as_ref().err())
        .expect("a truncation error");
    assert!(
        error.message.contains("output token limit"),
        "{}",
        error.message
    );
}
