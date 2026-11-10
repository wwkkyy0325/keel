use std::collections::BTreeMap;

use base64::Engine as _;
use keel_core::{
    ArgValue, ErrorCode, ImagePart, InteractionKind, InteractionOption, InteractionRequest,
    Message, MessageRole, ModelError, ModelRequest, ParamKind, ReasoningLevel, ToolArgs,
    ToolCallId, ToolCallRequest, ToolSchema, TurnId,
};
use serde_json::{json, Map, Value};

/// Reserved control tool used to request a user interaction. It is not a
/// domain capability.
pub const CONTROL_TOOL_NAME: &str = "keel_interact";

/// Builds the JSON body for an OpenAI-compatible chat completion request.
pub fn build_request(
    request: &ModelRequest,
    fallback_model: &str,
    reasoning: Option<ReasoningLevel>,
) -> Value {
    let model = if request.route.model.is_empty() {
        fallback_model
    } else {
        &request.route.model
    };
    // Thinking-mode providers require the assistant's reasoning text on the
    // next request even when `reasoning_effort` is not advertised. The budget
    // is one signal that the turn may have produced it; a message that already
    // carries reasoning is the other, which covers providers that think by
    // default while the profile advertises no reasoning support.
    let include_reasoning = reasoning.is_some() || request.thinking_budget > 0;
    // A tool message carries text only in this wire format, so images from a
    // tool result follow the whole group of tool messages as one user message.
    // Inserting it between tool results would break the rule that every
    // `tool_calls` message is answered by its tool messages first.
    let mut messages: Vec<Value> = Vec::with_capacity(request.messages.len());
    let mut pending_images: Vec<&ImagePart> = Vec::new();
    for message in &request.messages {
        if message.role != MessageRole::Tool && !pending_images.is_empty() {
            messages.push(image_message(std::mem::take(&mut pending_images)));
        }
        messages.push(message_json(message, include_reasoning));
        if message.role == MessageRole::Tool {
            pending_images.extend(message.images.iter());
        }
    }
    if !pending_images.is_empty() {
        messages.push(image_message(pending_images));
    }
    let mut tools: Vec<Value> = request.tools.iter().map(tool_json).collect();
    if request.control_tool {
        tools.push(control_tool_json());
    }

    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
        "max_tokens": request.max_output_tokens,
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if let Some(level) = reasoning {
        body["reasoning_effort"] = Value::String(level.as_str().to_owned());
    }
    body
}

/// Parses the JSON string carried in a tool-call `arguments` field.
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

/// Parses a control-tool payload into an interaction request.
///
/// Only clarify, choice and plan approval are model-initiated. Permission
/// approval and escalation are kernel-initiated and rejected here.
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
    if let Some(recommended) = value.get("recommended") {
        request.recommended = Some(
            recommended
                .as_str()
                .ok_or_else(|| invalid("interaction recommendation must be a string"))?
                .to_owned(),
        );
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

/// One completed call from an accumulated stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishedCall {
    Tool(ToolCallRequest),
    Interaction(InteractionRequest),
}

#[derive(Default)]
struct PartialCall {
    name: String,
    arguments: String,
}

/// Accumulates streamed tool-call deltas. OpenAI splits one call across
/// several chunks; arguments must be concatenated before parsing.
#[derive(Default)]
pub struct ToolCallAccumulator {
    calls: BTreeMap<usize, PartialCall>,
}

impl ToolCallAccumulator {
    pub fn apply(&mut self, calls: &Value) -> Result<(), ModelError> {
        let calls = calls
            .as_array()
            .ok_or_else(|| invalid("tool_calls delta must be an array"))?;
        for call in calls {
            let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let partial = self.calls.entry(index).or_default();
            if let Some(name) = call
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
            {
                if partial.name.is_empty() {
                    partial.name = name.to_owned();
                }
            }
            if let Some(arguments) = call
                .get("function")
                .and_then(|function| function.get("arguments"))
                .and_then(Value::as_str)
            {
                partial.arguments.push_str(arguments);
            }
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Vec<FinishedCall>, ModelError> {
        let mut finished = Vec::with_capacity(self.calls.len());
        for (_, partial) in self.calls {
            if partial.name.is_empty() {
                return Err(invalid("tool call is missing a function name"));
            }
            let arguments = if partial.arguments.trim().is_empty() {
                Value::Object(Map::new())
            } else {
                serde_json::from_str(&partial.arguments)
                    .map_err(|error| invalid(format!("invalid tool arguments: {error}")))?
            };
            if partial.name == CONTROL_TOOL_NAME {
                finished.push(FinishedCall::Interaction(parse_interaction_json(
                    &arguments,
                )?));
            } else {
                finished.push(FinishedCall::Tool(ToolCallRequest {
                    id: ToolCallId::next(),
                    name: partial.name,
                    args: parse_tool_args(&arguments)?,
                }));
            }
        }
        Ok(finished)
    }
}

fn message_json(message: &Message, include_reasoning: bool) -> Value {
    let mut value = Map::new();
    value.insert("role".to_owned(), json!(role_str(message.role)));
    // A user or assistant message with images becomes a content array. A tool
    // message keeps a string here; its images follow as a separate message.
    if message.role != MessageRole::Tool && !message.images.is_empty() {
        let mut parts = vec![json!({"type": "text", "text": message.content})];
        parts.extend(message.images.iter().map(image_json));
        value.insert("content".to_owned(), Value::Array(parts));
    } else {
        value.insert("content".to_owned(), json!(message.content));
    }
    if message.role == MessageRole::Assistant
        && (include_reasoning || message.reasoning_content.is_some())
    {
        // Thinking-mode providers require the field even when the model
        // produced no reasoning text. A message that already carries the text
        // keeps it even when the profile advertises no reasoning support.
        value.insert(
            "reasoning_content".to_owned(),
            json!(message.reasoning_content.as_deref().unwrap_or("")),
        );
    }
    if !message.tool_calls.is_empty() {
        let calls: Vec<Value> = message
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "id": wire_call_id(call.id),
                    "type": "function",
                    "function": {
                        "name": call.name,
                        "arguments": arguments_string(&call.args),
                    }
                })
            })
            .collect();
        value.insert("tool_calls".to_owned(), json!(calls));
    }
    if let Some(call_id) = message.tool_call_id {
        value.insert("tool_call_id".to_owned(), json!(wire_call_id(call_id)));
    }
    Value::Object(value)
}

/// Images from tool results follow the whole group of tool messages as one
/// user message; this wire format has no image content on a `tool` message.
fn image_message(images: Vec<&ImagePart>) -> Value {
    let mut parts = vec![json!({
        "type": "text",
        "text": "Images returned with the tool results above."
    })];
    parts.extend(images.into_iter().map(image_json));
    json!({ "role": "user", "content": Value::Array(parts) })
}

fn image_json(image: &ImagePart) -> Value {
    json!({
        "type": "image_url",
        "image_url": { "url": data_url(image) }
    })
}

fn data_url(image: &ImagePart) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(&image.data);
    format!("data:{};base64,{}", image.media_type, encoded)
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
        "type": "function",
        "function": {
            "name": schema.name,
            "description": schema.description,
            "parameters": {
                "type": "object",
                "properties": properties,
                "required": required,
            }
        }
    })
}

fn control_tool_json() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": CONTROL_TOOL_NAME,
            "description": "Control primitive: ask a clarifying question, present a choice, or request plan approval. It performs no work.",
            "parameters": {
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
                    "recommended": {
                        "type": "string",
                        "description": "id of the option you recommend, when you have one"
                    },
                    "timeout_ms": { "type": "integer" }
                },
                "required": ["kind", "prompt"]
            }
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

fn role_str(role: MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}

fn wire_call_id(id: ToolCallId) -> String {
    format!("call-{}", id.get())
}

fn arguments_string(args: &ToolArgs) -> String {
    serde_json::to_string(&args_json(args)).unwrap_or_else(|_| "{}".to_owned())
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

fn invalid(message: impl Into<String>) -> ModelError {
    ModelError::new(ErrorCode::InvalidResponse, message, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keel_core::ModelRoute;
    use std::sync::Arc;

    #[test]
    fn thinking_budget_keeps_reasoning_content_on_the_followup() {
        let request = ModelRequest {
            messages: vec![Arc::new(
                Message::assistant("done").with_reasoning("checked the files"),
            )],
            tools: Vec::new(),
            control_tool: false,
            thinking_budget: 1024,
            max_output_tokens: 128,
            route: ModelRoute {
                provider: "openai".to_owned(),
                model: "m".to_owned(),
            },
            reasoning: ReasoningLevel::Low,
        };

        let body = build_request(&request, "fallback", None);

        assert_eq!(
            body["messages"][0]["reasoning_content"],
            "checked the files"
        );
    }

    #[test]
    fn captured_reasoning_is_echoed_without_a_budget_or_effort() {
        let request = ModelRequest {
            messages: vec![Arc::new(
                Message::assistant_tool_calls(Vec::new()).with_reasoning("looked around"),
            )],
            tools: Vec::new(),
            control_tool: false,
            thinking_budget: 0,
            max_output_tokens: 128,
            route: ModelRoute {
                provider: "openai-compatible".to_owned(),
                model: "m".to_owned(),
            },
            reasoning: ReasoningLevel::Low,
        };

        // A provider that thinks by default: the profile advertises neither a
        // budget nor `reasoning_effort`, but the message already carries the
        // text the follow-up must echo.
        let body = build_request(&request, "fallback", None);

        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(body["messages"][0]["reasoning_content"], "looked around");
    }

    fn image_request(messages: Vec<Arc<Message>>) -> ModelRequest {
        ModelRequest {
            messages,
            tools: Vec::new(),
            control_tool: false,
            thinking_budget: 0,
            max_output_tokens: 128,
            route: ModelRoute {
                provider: "openai-compatible".to_owned(),
                model: "m".to_owned(),
            },
            reasoning: ReasoningLevel::Low,
        }
    }

    #[test]
    fn a_user_image_becomes_a_content_array() {
        let request = image_request(vec![Arc::new(
            Message::user("what is this?").with_images(vec![ImagePart::png(vec![1, 2, 3])]),
        )]);

        let body = build_request(&request, "fallback", None);

        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["messages"][0]["content"][0]["text"], "what is this?");
        assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
        assert_eq!(
            body["messages"][0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AQID"
        );
    }

    #[test]
    fn tool_images_follow_the_whole_group_of_tool_messages() {
        let request = image_request(vec![
            Arc::new(
                Message::tool_result(ToolCallId::new(10), "screen_capture", "captured")
                    .with_images(vec![ImagePart::png(vec![1, 2, 3])]),
            ),
            Arc::new(Message::tool_result(ToolCallId::new(11), "echo", "ok")),
        ]);

        let body = build_request(&request, "fallback", None);

        assert_eq!(body["messages"][0]["role"], "tool");
        assert_eq!(body["messages"][1]["role"], "tool");
        assert_eq!(body["messages"][2]["role"], "user");
        assert_eq!(body["messages"][2]["content"][1]["type"], "image_url");
    }

    #[test]
    fn a_tool_image_follows_the_tool_message() {
        let request = image_request(vec![Arc::new(
            Message::tool_result(ToolCallId::new(9), "screen_capture", "captured 800x600")
                .with_images(vec![ImagePart::png(vec![1, 2, 3])]),
        )]);

        let body = build_request(&request, "fallback", None);

        assert_eq!(body["messages"][0]["role"], "tool");
        assert_eq!(
            body["messages"][0]["content"],
            "screen_capture: captured 800x600"
        );
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"][1]["type"], "image_url");
        assert_eq!(
            body["messages"][1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AQID"
        );
    }
}
