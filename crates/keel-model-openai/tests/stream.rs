use std::sync::Arc;
use std::time::Duration;

use keel_core::{
    CancelToken, ErrorCode, Message, ModelChunk, ModelError, ModelProvider, ModelRequest,
    ModelRoute, ModelStream, ReasoningLevel,
};
use keel_model_openai::{OpenAiConfig, OpenAiProvider};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_server(
    status: &'static str,
    content_type: &'static str,
    body: &'static str,
) -> String {
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
        let headers = String::from_utf8_lossy(&request[..headers_end]).to_ascii_lowercase();
        let content_length = headers
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
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
        let _ = socket.shutdown().await;
    });
    format!("http://{address}/v1")
}

async fn spawn_sequence(responses: Vec<(&'static str, &'static str, &'static str)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for (status, content_type, body) in responses {
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
            let headers = String::from_utf8_lossy(&request[..headers_end]).to_ascii_lowercase();
            let content_length = headers
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
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    format!("http://{address}/v1")
}

fn provider(url: String) -> OpenAiProvider {
    OpenAiProvider::new(OpenAiConfig {
        base_url: url,
        api_key: "test-key".to_owned(),
        model: "fallback-model".to_owned(),
        supports_reasoning: false,
        max_retries: 0,
    })
    .unwrap()
}

fn provider_with_retries(url: String, max_retries: u32) -> OpenAiProvider {
    OpenAiProvider::new(OpenAiConfig {
        base_url: url,
        api_key: "test-key".to_owned(),
        model: "fallback-model".to_owned(),
        supports_reasoning: false,
        max_retries,
    })
    .unwrap()
}

fn request() -> ModelRequest {
    ModelRequest {
        messages: vec![Arc::new(Message::user("hi"))],
        tools: Vec::new(),
        control_tool: true,
        thinking_budget: 0,
        max_output_tokens: 32,
        route: ModelRoute {
            provider: "openai".to_owned(),
            model: "test-model".to_owned(),
        },
        reasoning: ReasoningLevel::Low,
    }
}

async fn collect(mut stream: ModelStream) -> Vec<Result<ModelChunk, ModelError>> {
    let mut chunks = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(5), stream.recv()).await {
            Ok(Some(chunk)) => {
                let terminal = matches!(&chunk, Ok(ModelChunk::Finished { .. }) | Err(_));
                chunks.push(chunk);
                if terminal {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    chunks
}

#[tokio::test]
async fn text_deltas_and_usage_are_forwarded() {
    let url = spawn_server(
        "200 OK",
        "text/event-stream",
        "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n\
         data: {\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}]}\n\n\
         data: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n\
         data: [DONE]\n\n",
    )
    .await;
    let stream = provider(url)
        .stream(request(), &CancelToken::new())
        .await
        .unwrap();
    let chunks = collect(stream).await;

    let text: String = chunks
        .iter()
        .filter_map(|chunk| match chunk {
            Ok(ModelChunk::TextDelta(text)) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "hello");
    let usage = chunks.iter().find_map(|chunk| match chunk {
        Ok(ModelChunk::Finished { usage }) => Some(*usage),
        _ => None,
    });
    assert_eq!(usage.map(|usage| usage.input_tokens), Some(3));
    assert_eq!(usage.map(|usage| usage.output_tokens), Some(2));
}

#[tokio::test]
async fn streamed_tool_call_is_completed() {
    let url = spawn_server(
        "200 OK",
        "text/event-stream",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"x\",\"function\":{\"name\":\"echo\",\"arguments\":\"{\\\"text\\\":\\\"he\"}}]}}]}\n\n\
         data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"llo\\\"}\"}}]}}]}\n\n\
         data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n\
         data: [DONE]\n\n",
    )
    .await;
    let stream = provider(url)
        .stream(request(), &CancelToken::new())
        .await
        .unwrap();
    let chunks = collect(stream).await;

    let call = chunks.iter().find_map(|chunk| match chunk {
        Ok(ModelChunk::ToolCall(call)) => Some(call),
        _ => None,
    });
    let call = call.expect("tool call");
    assert_eq!(call.name, "echo");
    assert_eq!(call.args.require_str("text").unwrap(), "hello");
}

#[tokio::test]
async fn control_tool_becomes_an_interaction() {
    let url = spawn_server(
        "200 OK",
        "text/event-stream",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"keel_interact\",\"arguments\":\"{\\\"kind\\\":\\\"clarify\\\",\\\"prompt\\\":\\\"which?\\\"}\"}}]}}]}\n\n\
         data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n\
         data: [DONE]\n\n",
    )
    .await;
    let stream = provider(url)
        .stream(request(), &CancelToken::new())
        .await
        .unwrap();
    let chunks = collect(stream).await;

    let interaction = chunks.iter().find_map(|chunk| match chunk {
        Ok(ModelChunk::Interaction(interaction)) => Some(interaction),
        _ => None,
    });
    let interaction = interaction.expect("interaction");
    assert_eq!(interaction.prompt, "which?");
}

#[tokio::test]
async fn transient_500_is_retried_before_streaming() {
    let url = spawn_sequence(vec![
        ("500 Internal Server Error", "text/plain", "boom"),
        ("500 Internal Server Error", "text/plain", "boom"),
        (
            "200 OK",
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":\"stop\"}]}\n\n\
             data: [DONE]\n\n",
        ),
    ])
    .await;

    let stream = provider_with_retries(url, 2)
        .stream(request(), &CancelToken::new())
        .await
        .unwrap();
    let chunks = collect(stream).await;

    assert!(chunks.iter().any(|chunk| matches!(
        chunk.as_ref(),
        Ok(ModelChunk::TextDelta(text)) if text == "hello"
    )));
}

#[tokio::test]
async fn http_error_is_a_provider_error() {
    let url = spawn_server("500 Internal Server Error", "text/plain", "boom").await;
    let error = provider(url)
        .stream(request(), &CancelToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ModelProvider);
    assert!(error.retryable);
}

#[tokio::test]
async fn missing_done_and_finish_reason_is_a_stream_cut() {
    let url = spawn_server(
        "200 OK",
        "text/event-stream",
        "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
    )
    .await;
    let stream = provider(url)
        .stream(request(), &CancelToken::new())
        .await
        .unwrap();
    let chunks = collect(stream).await;
    let error = chunks
        .iter()
        .find_map(|chunk| chunk.as_ref().err())
        .expect("stream cut error");
    assert_eq!(error.code, ErrorCode::StreamCut);
}

#[tokio::test]
async fn cancellation_before_send_is_cancelled() {
    let url = spawn_server("200 OK", "text/event-stream", "data: [DONE]\n\n").await;
    let cancel = CancelToken::new();
    cancel.cancel();
    let error = provider(url).stream(request(), &cancel).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
}
