use std::sync::Arc;

use keel_core::{
    ArgValue, InteractionKind, Message, ModelRequest, ModelRoute, ParamKind, ParamSpec, RiskLevel,
    ToolArgs, ToolCallId, ToolCallRequest, ToolSchema,
};
use keel_model_openai::{
    build_request, parse_interaction_json, parse_tool_args, FinishedCall, ToolCallAccumulator,
    CONTROL_TOOL_NAME,
};
use serde_json::json;

fn echo_schema() -> ToolSchema {
    ToolSchema {
        name: "echo".to_owned(),
        description: "Echo text.".to_owned(),
        risk: RiskLevel::ReadOnly,
        params: vec![ParamSpec {
            name: "text".to_owned(),
            kind: ParamKind::String,
            required: true,
        }],
    }
}

fn request() -> ModelRequest {
    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hello".to_owned()));
    let call = ToolCallRequest {
        id: ToolCallId::new(7),
        name: "echo".to_owned(),
        args,
    };
    ModelRequest {
        messages: vec![
            Arc::new(Message::system("system")),
            Arc::new(Message::user("hi")),
            Arc::new(Message::assistant_tool_calls(vec![call])),
            Arc::new(Message::tool_result(ToolCallId::new(7), "echo", "hello")),
        ],
        tools: vec![echo_schema()],
        thinking_budget: 0,
        max_output_tokens: 128,
        route: ModelRoute {
            provider: "openai".to_owned(),
            model: "test-model".to_owned(),
        },
    }
}

#[test]
fn request_preserves_structured_tool_calls_and_control_tool() {
    let body = build_request(&request(), "fallback");
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["max_tokens"], 128);

    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert!(tools
        .iter()
        .any(|tool| tool["function"]["name"] == CONTROL_TOOL_NAME));
    assert!(tools.iter().any(|tool| tool["function"]["name"] == "echo"));

    let assistant = &body["messages"][2];
    assert_eq!(assistant["role"], "assistant");
    assert_eq!(assistant["tool_calls"][0]["id"], "call-7");
    assert_eq!(assistant["tool_calls"][0]["function"]["name"], "echo");
    assert_eq!(
        assistant["tool_calls"][0]["function"]["arguments"],
        serde_json::to_string(&json!({"text": "hello"})).unwrap()
    );

    let tool_result = &body["messages"][3];
    assert_eq!(tool_result["role"], "tool");
    assert_eq!(tool_result["tool_call_id"], "call-7");
}

#[test]
fn tool_arguments_round_trip() {
    let args = parse_tool_args(&json!({
        "text": "hello",
        "count": 3,
        "enabled": true,
        "items": ["a", "b"]
    }))
    .unwrap();
    assert_eq!(
        args.get("text"),
        Some(&ArgValue::String("hello".to_owned()))
    );
    assert_eq!(args.get("count"), Some(&ArgValue::Integer(3)));
    assert_eq!(args.get("enabled"), Some(&ArgValue::Boolean(true)));
    assert_eq!(
        args.get("items"),
        Some(&ArgValue::StringList(vec!["a".to_owned(), "b".to_owned()]))
    );
}

#[test]
fn invalid_tool_arguments_are_rejected() {
    assert!(parse_tool_args(&json!({ "count": 1.5 })).is_err());
    assert!(parse_tool_args(&json!({ "items": [1, 2] })).is_err());
    assert!(parse_tool_args(&json!("not an object")).is_err());
}

#[test]
fn control_payload_maps_to_an_interaction() {
    let interaction = parse_interaction_json(&json!({
        "kind": "clarify",
        "prompt": "Which number?",
        "options": [
            { "id": "one", "label": "One" },
            { "id": "two", "label": "Two", "description": "the second" }
        ],
        "timeout_ms": 5000
    }))
    .unwrap();
    assert_eq!(interaction.kind, InteractionKind::Clarify);
    assert_eq!(interaction.prompt, "Which number?");
    assert_eq!(interaction.options.len(), 2);
    assert_eq!(
        interaction.options[1].description.as_deref(),
        Some("the second")
    );
    assert_eq!(interaction.timeout_ms, Some(5000));
}

#[test]
fn model_cannot_request_kernel_permission_interactions() {
    assert!(parse_interaction_json(&json!({
        "kind": "permission_approval",
        "prompt": "allow"
    }))
    .is_err());
    assert!(parse_interaction_json(&json!({
        "kind": "escalation",
        "prompt": "escalate"
    }))
    .is_err());
}

#[test]
fn streamed_tool_arguments_are_accumulated() {
    let mut accumulator = ToolCallAccumulator::default();
    accumulator
        .apply(&json!([{
            "index": 0,
            "function": { "name": "echo", "arguments": "{\"text\":\"he" }
        }]))
        .unwrap();
    accumulator
        .apply(&json!([{
            "index": 0,
            "function": { "arguments": "llo\"}" }
        }]))
        .unwrap();

    let calls = accumulator.finish().unwrap();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        FinishedCall::Tool(call) => {
            assert_eq!(call.name, "echo");
            assert_eq!(
                call.args.get("text"),
                Some(&ArgValue::String("hello".to_owned()))
            );
        }
        FinishedCall::Interaction(_) => panic!("expected a tool call"),
    }
}

#[test]
fn control_tool_call_becomes_an_interaction() {
    let mut accumulator = ToolCallAccumulator::default();
    accumulator
        .apply(&json!([{
            "index": 0,
            "function": {
                "name": CONTROL_TOOL_NAME,
                "arguments": "{\"kind\":\"choice\",\"prompt\":\"pick\"}"
            }
        }]))
        .unwrap();
    let calls = accumulator.finish().unwrap();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        FinishedCall::Interaction(interaction) => {
            assert_eq!(interaction.kind, InteractionKind::Choice);
            assert_eq!(interaction.prompt, "pick");
        }
        FinishedCall::Tool(_) => panic!("expected an interaction"),
    }
}
