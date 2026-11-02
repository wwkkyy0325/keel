//! Anthropic Messages wire mapping.
//!
//! The wire layer turns kernel messages into the Messages API shape and
//! decodes its streaming events; the provider owns the HTTP client and the
//! retry policy. No kernel crate depends on this adapter.

pub mod provider;
pub mod wire;

pub use provider::{AnthropicConfig, AnthropicProvider, ANTHROPIC_VERSION};
pub use wire::{
    build_request, parse_interaction_json, parse_tool_args, StreamDecoder, CONTROL_TOOL_NAME,
};
