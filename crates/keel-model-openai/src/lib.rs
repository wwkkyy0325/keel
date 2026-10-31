//! OpenAI-compatible model wire mapping.
//!
//! This crate is the wire boundary: it turns typed kernel data into JSON and
//! parses streaming JSON back into typed chunks. It contains no HTTP client in
//! the wire layer.

pub mod provider;
pub mod wire;

pub use provider::{OpenAiConfig, OpenAiProvider};
pub use wire::{
    build_request, parse_interaction_json, parse_tool_args, FinishedCall, ToolCallAccumulator,
    CONTROL_TOOL_NAME,
};
