use std::collections::BTreeMap;

use keel_core::{
    ArgValue, ErrorCode, InteractionKind, InteractionOption, InteractionRequest, Message,
    MessageRole, ModelError, ModelRequest, ParamKind, ToolArgs, ToolCallId, ToolCallRequest,
    ToolSchema, TurnId,
};
use serde_json::{json, Map, Value};

/// Reserved control tool used to request a user interaction. It is not a
/// domain capability.
pub const CONTROL_TOOL_NAME: &str = "keel_interact";

/// Builds the JSON body for an OpenAI-compatible chat completion request.
pub fn build_request(
    request: &ModelRequest,
    fallback_model: &str,
    reasoning_effort: Option<&str>,
) -> Value {
    let model = if request.route.model.is_empty() {
        fallback_model
    } else {
        &request.route.model
    };
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|message| message_json(message))
        .collect();
    let mut tools: Vec<Value> = request.tools.iter().map(tool_json).collect();
    tools.push(control_tool_json());

    let mut body = json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "stream": true,
        "stream_options": { "include_usage": true },
        "max_tokens": request.max_output_tokens,
    });
    if let Some(effort) = reasoning_effort {
        body["reasoning_effort"] = Value::String(effort.to_owned());
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

fn message_json(message: &Message) -> Value {
    let mut value = Map::new();
    value.insert("role".to_owned(), json!(role_str(message.role)));
    value.insert("content".to_owned(), json!(message.content));
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
