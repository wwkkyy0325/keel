# Anthropic provider

The spec promised an Anthropic-compatible provider; only the OpenAI adapter
existed. This adds `keel-model-anthropic` and wires it into the host.

## Steps

- [x] `keel-model-anthropic` crate: `build_request` separates the system
      prompt, converts assistant tool calls to `tool_use` blocks and tool
      results to `tool_result` blocks, and emits Anthropic `input_schema`
      tools. Verify: `system_is_separate_and_tool_history_uses_blocks`,
      `tool_schemas_use_anthropic_input_schema`.
- [x] Streaming decoder: `message_start` usage, text/thinking deltas,
      `input_json_delta` accumulation, `message_delta` usage, `message_stop`
      and `error`. Control-tool calls become interactions. Verify:
      `text_and_usage_are_decoded`, `tool_input_is_accumulated_and_parsed`,
      `control_tool_becomes_an_interaction`, `an_error_event_fails_the_stream`.
- [x] Provider: `/v1/messages` (or `/messages` when the base URL already ends
      in `/v1`), `x-api-key` plus `anthropic-version`, retry/backoff and
      cancellation. Verify: `crates/keel-model-anthropic/tests/stream.rs`
      against a local HTTP server, including the request path and headers.
- [x] Host: `provider = "anthropic-compatible"` selects
      `HostModel::Anthropic`, `variants/coding/profile.anthropic.toml` uses
      `claude-sonnet-5-5` and `ANTHROPIC_API_KEY`, and the summarizer uses the
      same adapter. Verify: `anthropic_profile_loads_and_validates`.
- [x] Host smoke: a temporary profile pointed at a local SSE server; the CLI
      returned `state: Done`, `answer: mock anthropic answer`, usage
      `input=5 output=3`, and the server saw `POST /v1/messages` with
      `x-api-key` and `anthropic-version`.
- [ ] A live API run needs `ANTHROPIC_API_KEY`; it was absent here, so only
      wire, local-server and host-mock verification ran.

## Boundaries

- The adapter covers text, thinking deltas, tool use and usage. Prompt-cache
  markers, extended thinking budgets, images and structured tool results are
  not wired.
- Tool input remains flat (string, integer, boolean, string list), matching
  the kernel argument model.
- The control-tool schema and parser are duplicated from the OpenAI adapter.
  Factor them into a shared wire crate when a third provider needs the same
  code.
- The provider never logs or serializes the API key.
