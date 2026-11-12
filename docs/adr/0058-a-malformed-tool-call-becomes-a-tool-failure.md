# ADR 0058 — A malformed tool call becomes a tool failure

## Status

Accepted.

## Context

ADR 0055 retried a malformed tool-arguments payload once with a corrective
note. In practice a long turn can still die on it: a live `deepseek-flash`
turn spent 303 seconds and 30 model calls editing a scene, then emitted an
`edit_file` call whose `new` value was raw code instead of a JSON string. The
retry emitted the same shape and the turn failed, losing the run.

A retry re-runs the whole model call and discards the round; it treats a
recoverable formatting mistake as a boundary failure. The model can usually
fix it if it is shown the failure as a tool result, the same way it fixes a
missing argument.

## Decision

1. The OpenAI and Anthropic decoders stop returning a retryable error for
   invalid tool-argument JSON. They emit
   `ModelChunk::MalformedToolCall { name, message }`, with the parse error and
   a bounded sample of what arrived.
2. The driver turns each into a tool call with empty arguments and a failure
   result. The call is written into the assistant message and the transcript
   (`ToolStarted`/`ToolFinished`), so the model and the user both see it.
   It never reaches the dispatcher.
3. The turn continues. The model sees the failure on its next call and can
   re-issue the call with valid JSON; the rest of the turn's work is kept.
4. Type errors in otherwise valid JSON (a number where a string belongs, an
   object where a scalar belongs) keep ADR 0055's retry with a corrective note.

The echoed assistant call carries `{}` because the provider requires valid
JSON in the history; the tool result carries the error and the sample.

## Consequences

- A formatting mistake no longer kills a long turn.
- The transcript shows a failed tool row with the parser's message.
- A model that repeats the mistake spends tool-call rounds but the turn can
  still finish; the user can stop it.
- The dispatcher never sees an empty-argument call, so a tool with all
  optional arguments cannot run on invented arguments.
- A call whose name is missing is still a decode error; there is nothing to
  show the model.

## Alternatives rejected

- More retries with a stronger note: the same context tends to produce the
  same shape, and each retry discards the round.
- Repairing the JSON: the boundary between the malformed value and the next
  key is a guess, and a tool must not run on guessed arguments.
- Dispatching the call with empty arguments: an optional-only tool would run
  with invented arguments.
