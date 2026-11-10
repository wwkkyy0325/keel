# ADR 0050 — A response cut by the output limit says so

## Status

Accepted and implemented in the model adapters. Released as 0.2.9.

## Context

A live turn failed with `invalid tool arguments: EOF while parsing a string
at line 1 column 7170`. The model had emitted a large tool call; the
provider stopped at the profile's `max_output_tokens` (8192) and cut the
argument JSON mid-string. The adapters only checked that a finish reason was
present, then parsed the accumulated arguments, so the JSON error was
reported and the actual cause — the output limit — was invisible.

A text-only answer cut the same way was worse: the partial text was
delivered as a finished answer, with no sign that it was incomplete.

## Decision

1. The OpenAI-compatible reader records `finish_reason`; `"length"` produces
   a `ModelError` (`invalid_response`) saying the response hit the output
   token limit before it finished, instead of parsing a truncated call.
2. The Anthropic reader records `stop_reason`; `"max_tokens"` produces the
   same error from `finish`.
3. A model error after text was streamed keeps that text in the turn's
   answer body, as a cancelled turn already does, so the partial output is
   visible next to the error.
4. The streamed chunks before the error are still forwarded; a host shows
   the partial answer.

## Consequences

- A cut-off turn says why it stopped; the user can retry or split the work.
- Providers that report other stop reasons keep the previous behavior.
- The reference coding profile raises its output cap separately; the floor
  belongs to the host, not the kernel.
