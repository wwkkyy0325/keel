# M2 — OpenAI-compatible provider

## Steps

- [x] 1. Wire mapping: request JSON, message/tool mapping, tool-call accumulator, interaction control tool — verify: `cargo test -p keel-model-openai`
- [x] 2. Streaming HTTP provider: SSE parsing, error mapping, cancellation — verify: local HTTP server tests
- [ ] 3. Host wiring: profile selects mock or OpenAI-compatible provider — verify: CLI smoke test
- [ ] 4. Commit each step separately

## Decisions

- `keel-model-openai` depends on `keel-core` only; it implements the
  `ModelProvider` port.
- The wire adapter normalizes tool-call ids to `call-{n}` in both assistant
  tool calls and tool results, so history stays consistent regardless of the
  provider's original id.
- A reserved control tool (`keel_interact`) requests clarify, choice or plan
  approval. Permission approval and escalation are kernel-initiated; the model
  cannot request them.
- No secrets in the adapter; the host reads the base URL and key.
- SSE requires `[DONE]` or a finish reason; an abrupt body end is a stream cut.
- Tool arguments are typed at the kernel boundary and parsed once at the wire
  edge.

## Result

- 13 tests in `keel-model-openai`: 7 wire + 6 local-server streaming tests.
- Workspace tests, clippy and `cargo deny check` pass.
- The provider handles text deltas, reasoning deltas, accumulated tool calls,
  control-tool interactions, usage, HTTP errors, stream cuts and cancellation.
- Host wiring is still pending: the CLI always uses the mock provider.
