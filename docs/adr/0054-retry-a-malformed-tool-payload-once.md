# ADR 0054 — Retry a malformed tool payload once

## Status

Accepted.

## Context

A live `deepseek-flash` run wrote its dossier, then failed the turn after
366 seconds with `invalid tool input` (`expected , or }`). The model had
streamed a tool call whose accumulated JSON was syntactically invalid; the
provider rejected it, and the driver treated that as a terminal model error.
The same model produces valid JSON on a re-sample in most such cases, and the
turn had no side effects from the malformed call (the tool never ran).

Retrying every `retryable` error would change existing semantics: a provider
error or a cut stream must still end the turn, and a cut stream keeps the
partial text it produced.

## Decision

1. The Anthropic and OpenAI decoders mark only a malformed tool-arguments
   payload as `InvalidResponse` with `retryable: true`.
2. The driver retries a model call once when the error is `InvalidResponse`
   and retryable. It discards the failed attempt's streamed text (truncating
   the accumulated answer to its pre-attempt length) and reuses the same
   request and messages.
3. Other retryable errors — provider failures, timeouts, stream cuts — keep
   their previous path: the turn fails, and a cut stream's partial text stays.
4. The retry is not visible as a failure event; the attempt's started and
   finished events remain, so usage and duration are still attributed to the
   turn.

## Consequences

- A transient malformed tool call no longer loses a long turn.
- Live views may show the failed attempt's streamed draft before the retry;
  the persisted answer comes from the successful attempt only.
- A model that repeatedly emits invalid JSON still fails after one retry,
  with the same error as before.

## Alternatives rejected

- Retrying all retryable errors at the driver: it changes provider-failure
  and stream-cut semantics and broke their tests.
- Repairing the JSON in place: a guess can invent arguments, and a tool call
  must not run on invented arguments.
- Failing the turn and asking the user to repeat: the work before the call is
  already done, and the failure is transient.
