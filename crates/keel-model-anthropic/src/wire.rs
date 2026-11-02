//! Anthropic Messages wire mapping.
//!
//! The kernel keeps one flat text-plus-tool-call model. Anthropic splits the
//! system prompt into its own field, represents tool calls as `tool_use`
//! content blocks and returns tool results as `tool_result` blocks inside a
//! user message. This module is the only place that translates between them.

use std::collections::BTreeMap;
use std::sync::Arc;

use keel_core::{
    ArgValue, ErrorCode, InteractionKind, InteractionOption, InteractionRequest, Message,
    MessageRole, ModelChunk, ModelError, ModelRequest, ParamKind, TokenUsage, ToolArgs, ToolCallId,
    ToolCallRequest, ToolSchema, TurnId,
};
use serde_json::{json, Map, Value};

/// Reserved control tool used to request a user interaction.
pub const CONTROL_TOOL_NAME: &str = "keel_interact";

/// Builds the JSON body for `POST /v1/messages`.
pub fn build_request(request: &ModelRequest, fallback_model: &str) -> Value {
    let model = if request.route.model.is_empty() {
        fallback_model
    } else {
        &request.route.model
    };
    let system: Vec<&str> = request
        .messages
        .iter()
        .filter(|message| message.role == MessageRole::System)
        .map(|message| message.content.as_str())
        .filter(|content| !content.trim().is_empty())
        .collect();
    let messages = messages_json(&request.messages);

    let mut tools: Vec<Value> = request.tools.iter().map(tool_json).collect();
    if request.control_tool {
        tools.push(control_tool_json());
    }

    let mut body = json!({
        "model": model,
        "max_tokens": request.max_output_tokens.max(1),
        "messages": messages,
        "stream": true,
    });
    if !system.is_empty() {
        body["system"] = Value::String(system.join("\n\n"));
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    body
}

fn messages_json(messages: &[Arc<Message>]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for message in messages {
        match message.role {
            MessageRole::System => {}
            MessageRole::User => {
                if !message.content.is_empty() {
                    out.push(json!({"role": "user", "content": message.content}));
                }
            }
            MessageRole::Assistant => {
                let mut blocks = Vec::new();
                if !message.content.is_empty() {
                    blocks.push(json!({"type": "text", "text": message.content}));
                }
                for call in &message.tool_calls {
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": wire_call_id(call.id),
                        "name": call.name,
                        "input": args_json(&call.args),
                    }));
                }
                if !blocks.is_empty() {
                    out.push(json!({"role": "assistant", "content": blocks}));
                }
            }
            MessageRole::Tool => {
                let block = json!({
                    "type": "tool_result",
                    "tool_use_id": message.tool_call_id.map(wire_call_id).unwrap_or_default(),
                    "content": message.content,
                });
                match out.last_mut() {
                    Some(previous)
                        if previous.get("role").and_then(Value::as_str) == Some("user")
                            && previous.get("content").is_some_and(Value::is_array) =>
                    {
                        if let Some(blocks) =
                            previous.get_mut("content").and_then(Value::as_array_mut)
                        {
                            blocks.push(block);
                        }
                    }
                    _ => out.push(json!({"role": "user", "content": [block]})),
                }
            }
        }
    }
    out
}

fn tool_json(schema: &ToolSchema) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for param in &schema.params {
        properties.insert(param.name.clone(), param_json(param.kind));
        if param.required {
            required.push(param.name.clone());
        }
    }
    json!({
        "name": schema.name,
        "description": schema.description,
        "input_schema": {
            "type": "object",
            "properties": properties,
            "required": required,
        }
    })
}

fn control_tool_json() -> Value {
    json!({
        "name": CONTROL_TOOL_NAME,
        "description": "Control primitive: ask a clarifying question, present a choice, or request plan approval. It performs no work.",
        "input_schema": {
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["clarify", "choice", "plan_approval"]
                },
                "prompt": { "type": "string" },
                "options": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "label": { "type": "string" },
                            "description": { "type": "string" }
                        },
                        "required": ["id", "label"]
                    }
                },
                "timeout_ms": { "type": "integer" }
            },
            "required": ["kind", "prompt"]
        }
    })
}

fn param_json(kind: ParamKind) -> Value {
    match kind {
        ParamKind::String => json!({ "type": "string" }),
        ParamKind::Integer => json!({ "type": "integer" }),
        ParamKind::Boolean => json!({ "type": "boolean" }),
        ParamKind::StringList => json!({ "type": "array", "items": { "type": "string" } }),
    }
}

/// Parses flat tool input into the kernel's typed arguments.
pub fn parse_tool_args(value: &Value) -> Result<ToolArgs, ModelError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("tool arguments must be a JSON object"))?;
    let mut args = ToolArgs::new();
    for (key, value) in object {
        args.insert(key.clone(), parse_arg(value)?);
    }
    Ok(args)
}

fn parse_arg(value: &Value) -> Result<ArgValue, ModelError> {
    match value {
        Value::String(value) => Ok(ArgValue::String(value.clone())),
        Value::Number(value) => value
            .as_i64()
            .map(ArgValue::Integer)
            .ok_or_else(|| invalid("tool argument numbers must be integers")),
        Value::Bool(value) => Ok(ArgValue::Boolean(*value)),
        Value::Array(values) => {
            let mut strings = Vec::with_capacity(values.len());
            for value in values {
                strings.push(
                    value
                        .as_str()
                        .ok_or_else(|| invalid("string-list arguments must contain strings"))?
                        .to_owned(),
                );
            }
            Ok(ArgValue::StringList(strings))
        }
        Value::Null | Value::Object(_) => Err(invalid("unsupported tool argument type")),
    }
}

/// Parses a control-tool payload into an interaction request.
pub fn parse_interaction_json(value: &Value) -> Result<InteractionRequest, ModelError> {
    let kind = match value.get("kind").and_then(Value::as_str) {
        Some("clarify") => InteractionKind::Clarify,
        Some("choice") => InteractionKind::Choice,
        Some("plan_approval") => InteractionKind::PlanApproval,
        Some(other) => {
            return Err(invalid(format!(
                "unsupported model interaction kind `{other}`"
            )));
        }
        None => return Err(invalid("interaction kind is missing")),
    };
    let prompt = value
        .get("prompt")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("interaction prompt is missing"))?;

    let mut request = InteractionRequest::new(TurnId::new(0), kind, prompt);
    if let Some(options) = value.get("options") {
        let options = options
            .as_array()
            .ok_or_else(|| invalid("interaction options must be an array"))?;
        for option in options {
            let id = option
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("interaction option is missing an id"))?;
            let label = option
                .get("label")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("interaction option is missing a label"))?;
            let mut parsed = InteractionOption::new(id, label);
            parsed.description = option
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned);
            request.options.push(parsed);
        }
    }
    if let Some(timeout) = value.get("timeout_ms") {
        request.timeout_ms = Some(
            timeout
                .as_u64()
                .ok_or_else(|| invalid("interaction timeout must be an integer"))?,
        );
    }
    Ok(request)
}

fn wire_call_id(id: ToolCallId) -> String {
    format!("call-{}", id.get())
}

fn args_json(args: &ToolArgs) -> Value {
    let mut object = Map::new();
    for key in args.keys() {
        if let Some(value) = args.get(key) {
            object.insert(key.clone(), arg_json(value));
        }
    }
    Value::Object(object)
}

fn arg_json(value: &ArgValue) -> Value {
    match value {
        ArgValue::String(value) => Value::String(value.clone()),
        ArgValue::Integer(value) => json!(value),
        ArgValue::Boolean(value) => json!(value),
        ArgValue::StringList(values) => json!(values),
    }
}

fn invalid(message: impl Into<String>) -> ModelError {
    ModelError::new(ErrorCode::InvalidResponse, message, false)
}

#[derive(Default)]
struct PartialTool {
    name: String,
    input: String,
}

/// Decodes the Anthropic streaming events into kernel chunks.
///
/// Text and thinking deltas are forwarded immediately; a tool call is emitted
/// from `finish` once its JSON input is complete. `message_stop` (or a
/// `stop_reason` on `message_delta`) marks the stream as complete.
#[derive(Default)]
pub struct StreamDecoder {
    tools: BTreeMap<u64, PartialTool>,
    usage: TokenUsage,
    stopped: bool,
}

impl StreamDecoder {
    pub fn apply(&mut self, event: &Value) -> Result<Vec<ModelChunk>, ModelError> {
        let mut chunks = Vec::new();
        match event.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                if let Some(usage) = event
                    .get("message")
                    .and_then(|message| message.get("usage"))
                {
                    self.update_usage(usage);
                }
            }
            Some("content_block_start") => {
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0);
                if let Some(block) = event.get("content_block") {
                    match block.get("type").and_then(Value::as_str) {
                        Some("tool_use") => {
                            let name = block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned();
                            self.tools.insert(
                                index,
                                PartialTool {
                                    name,
                                    input: String::new(),
                                },
                            );
                        }
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                if !text.is_empty() {
                                    chunks.push(ModelChunk::TextDelta(text.to_owned()));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some("content_block_delta") => {
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0);
                if let Some(delta) = event.get("delta") {
                    match delta.get("type").and_then(Value::as_str) {
                        Some("text_delta") => {
                            if let Some(text) = delta.get("text").and_then(Value::as_str) {
                                if !text.is_empty() {
                                    chunks.push(ModelChunk::TextDelta(text.to_owned()));
                                }
                            }
                        }
                        Some("thinking_delta") => {
                            if let Some(text) = delta.get("thinking").and_then(Value::as_str) {
                                if !text.is_empty() {
                                    chunks.push(ModelChunk::ThinkingDelta(text.to_owned()));
                                }
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(partial) = delta.get("partial_json").and_then(Value::as_str)
                            {
                                if let Some(tool) = self.tools.get_mut(&index) {
                                    tool.input.push_str(partial);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some("message_delta") => {
                if let Some(usage) = event.get("usage") {
                    self.update_usage(usage);
                }
                if event
                    .get("delta")
                    .and_then(|delta| delta.get("stop_reason"))
                    .is_some_and(|reason| !reason.is_null())
                {
                    self.stopped = true;
                }
            }
            Some("message_stop") => self.stopped = true,
            Some("error") => {
                let message = event
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("stream error");
                return Err(ModelError::provider(message.to_owned()));
            }
            _ => {}
        }
        Ok(chunks)
    }

    pub fn stopped(&self) -> bool {
        self.stopped
    }

    pub fn finish(&mut self) -> Result<Vec<ModelChunk>, ModelError> {
        let mut chunks = Vec::new();
        for (_, partial) in std::mem::take(&mut self.tools) {
            if partial.name.is_empty() {
                return Err(invalid("tool use is missing a name"));
            }
            let input = if partial.input.trim().is_empty() {
                Value::Object(Map::new())
            } else {
                serde_json::from_str(&partial.input)
                    .map_err(|error| invalid(format!("invalid tool input: {error}")))?
            };
            if partial.name == CONTROL_TOOL_NAME {
                chunks.push(ModelChunk::Interaction(parse_interaction_json(&input)?));
            } else {
                chunks.push(ModelChunk::ToolCall(ToolCallRequest {
                    id: ToolCallId::next(),
                    name: partial.name,
                    args: parse_tool_args(&input)?,
                }));
            }
        }
        chunks.push(ModelChunk::Finished { usage: self.usage });
        Ok(chunks)
    }

    fn update_usage(&mut self, usage: &Value) {
        if let Some(input) = usage.get("input_tokens").and_then(Value::as_u64) {
            self.usage.input_tokens = u32::try_from(input).unwrap_or(u32::MAX);
        }
        if let Some(output) = usage.get("output_tokens").and_then(Value::as_u64) {
            self.usage.output_tokens = u32::try_from(output).unwrap_or(u32::MAX);
        }
        if let Some(cached) = usage.get("cache_read_input_tokens").and_then(Value::as_u64) {
            self.usage.cached_input_tokens = u32::try_from(cached).unwrap_or(u32::MAX);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keel_core::{ModelRoute, ParamSpec, ReasoningLevel};

    fn request() -> ModelRequest {
        ModelRequest {
            messages: Vec::new(),
            tools: Vec::new(),
            control_tool: false,
            thinking_budget: 0,
            max_output_tokens: 1024,
            route: ModelRoute {
                provider: "anthropic".to_owned(),
                model: "claude-test".to_owned(),
            },
            reasoning: ReasoningLevel::Low,
        }
    }

    fn message(role: MessageRole, content: &str) -> Arc<Message> {
        Arc::new(Message::new(role, content))
    }

    #[test]
    fn system_is_separate_and_tool_history_uses_blocks() {
        let mut call = Message::new(MessageRole::Assistant, "calling");
        call.tool_calls.push(ToolCallRequest {
            id: ToolCallId::new(3),
            name: "echo".to_owned(),
            args: {
                let mut args = ToolArgs::new();
                args.insert("text", ArgValue::String("hi".to_owned()));
                args
            },
        });
        let mut result = Message::new(MessageRole::Tool, "ok");
        result.tool_call_id = Some(ToolCallId::new(3));

        let mut request = request();
        request.messages = vec![
            message(MessageRole::System, "be brief"),
            message(MessageRole::User, "hello"),
            Arc::new(call),
            Arc::new(result),
        ];

        let body = build_request(&request, "fallback");

        assert_eq!(body["system"], "be brief");
        assert_eq!(body["messages"][0]["content"], "hello");
        assert_eq!(body["messages"][1]["role"], "assistant");
        assert_eq!(body["messages"][1]["content"][1]["type"], "tool_use");
        assert_eq!(body["messages"][1]["content"][1]["id"], "call-3");
        assert_eq!(body["messages"][2]["role"], "user");
        assert_eq!(body["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "call-3");
    }

    #[test]
    fn tool_schemas_use_anthropic_input_schema() {
        let mut request = request();
        request.tools.push(ToolSchema {
            name: "echo".to_owned(),
            description: "echo text".to_owned(),
            risk: keel_core::RiskLevel::ReadOnly,
            params: vec![ParamSpec {
                name: "text".to_owned(),
                kind: ParamKind::String,
                required: true,
            }],
        });
        request.control_tool = true;

        let body = build_request(&request, "fallback");

        assert_eq!(body["tools"][0]["name"], "echo");
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(
            body["tools"][0]["input_schema"]["properties"]["text"]["type"],
            "string"
        );
        assert_eq!(body["tools"][1]["name"], CONTROL_TOOL_NAME);
    }

    #[test]
    fn text_and_usage_are_decoded() {
        let mut decoder = StreamDecoder::default();
        let chunks = decoder
            .apply(&json!({
                "type": "message_start",
                "message": {"usage": {"input_tokens": 10, "cache_read_input_tokens": 4}}
            }))
            .unwrap();
        assert!(chunks.is_empty());

        let chunks = decoder
            .apply(&json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "Hi"}
            }))
            .unwrap();
        assert_eq!(chunks, vec![ModelChunk::TextDelta("Hi".to_owned())]);

        decoder
            .apply(&json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 2}
            }))
            .unwrap();
        decoder.apply(&json!({"type": "message_stop"})).unwrap();

        let chunks = decoder.finish().unwrap();
        assert_eq!(
            chunks,
            vec![ModelChunk::Finished {
                usage: TokenUsage {
                    input_tokens: 10,
                    output_tokens: 2,
                    cached_input_tokens: 4,
                }
            }]
        );
    }

    #[test]
    fn tool_input_is_accumulated_and_parsed() {
        let mut decoder = StreamDecoder::default();
        decoder
            .apply(&json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "tool_use", "id": "toolu_1", "name": "echo"}
            }))
            .unwrap();
        decoder
            .apply(&json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "{\"text\":"}
            }))
            .unwrap();
        decoder
            .apply(&json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "\"hi\"}"}
            }))
            .unwrap();
        decoder.apply(&json!({"type": "message_stop"})).unwrap();

        let chunks = decoder.finish().unwrap();
        assert_eq!(chunks.len(), 2);
        match &chunks[0] {
            ModelChunk::ToolCall(call) => {
                assert_eq!(call.name, "echo");
                assert_eq!(
                    call.args.get("text"),
                    Some(&ArgValue::String("hi".to_owned()))
                );
            }
            other => panic!("unexpected chunk: {other:?}"),
        }
        assert!(matches!(chunks[1], ModelChunk::Finished { .. }));
    }

    #[test]
    fn control_tool_becomes_an_interaction() {
        let mut decoder = StreamDecoder::default();
        decoder
            .apply(&json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "tool_use", "id": "toolu_2", "name": CONTROL_TOOL_NAME}
            }))
            .unwrap();
        decoder
            .apply(&json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": "{\"kind\":\"clarify\",\"prompt\":\"which?\"}"
                }
            }))
            .unwrap();
        decoder.apply(&json!({"type": "message_stop"})).unwrap();

        let chunks = decoder.finish().unwrap();
        match &chunks[0] {
            ModelChunk::Interaction(request) => {
                assert_eq!(request.kind, InteractionKind::Clarify);
                assert_eq!(request.prompt, "which?");
            }
            other => panic!("unexpected chunk: {other:?}"),
        }
    }

    #[test]
    fn an_error_event_fails_the_stream() {
        let mut decoder = StreamDecoder::default();
        let error = decoder
            .apply(&json!({"type": "error", "error": {"message": "overloaded"}}))
            .unwrap_err();
        assert!(error.message.contains("overloaded"));
        assert!(error.retryable);
    }
}
