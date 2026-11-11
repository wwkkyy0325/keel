# ADR 0053 — A signed thinking block rides with the tool call

## Status

Accepted.

## Context

Thinking-mode providers such as DeepSeek's Anthropic-compatible endpoint
stream a `thinking` content block before the tool call and sign it. The API
requires that block, signature included, to be sent back on the next request
of the same turn. The kernel already carried reasoning text for OpenAI-style
providers (`Message.reasoning_content`), but the Anthropic adapter dropped
the block when serialising history, so every tool follow-up failed with
`400 The content[].thinking in the thinking mode must be passed back to the
API`.

Two details mattered. The signature arrives in a separate
`signature_delta` event, not with the thinking text. And a round may stream
an empty thinking string while still carrying a signature; skipping the block
because the text is empty still fails.

## Decision

1. `Message` gains `thinking_signature: Option<String>`, next to
   `reasoning_content`. Both are set on the assistant message that requested
   tools.
2. `ModelChunk` gains `ThinkingSignature`, emitted by the Anthropic decoder
   on `signature_delta`. The driver accumulates it per model call and replays
   it with the reasoning on the tool-call message (and on a continuation
   message).
3. The Anthropic adapter serialises an assistant thinking block first, before
   text and tool calls, with its signature. The block is emitted whenever the
   provider signed it, even if its text is empty.
4. The OpenAI adapter keeps echoing `reasoning_content` and ignores the
   signature.

## Consequences

- Thinking-mode tool loops complete, including rounds with empty reasoning.
- The signature travels in the transcript's message, so a host that persists
  a session stores it with the rest of the snapshot; it is not a secret.
- A provider that signs a block but expects it to be dropped is not
  supported; no such provider is configured.

## Alternatives rejected

- Encoding the signature inside `reasoning_content`: it would leak into the
  OpenAI wire and out of the typed contract.
- Emitting the thinking block only when text is non-empty: the 400 persists
  on empty-thinking rounds, which DeepSeek produces regularly.
- A provider-specific flag on the message: the signature itself is the
  signal.
