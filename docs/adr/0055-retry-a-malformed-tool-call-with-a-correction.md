# ADR 0055 — Retry a malformed tool call with a correction

## Status

Accepted.

## Context

ADR 0054 retried a malformed tool-arguments payload once by resending the
same request and messages. In practice that retry is weak: with the same
context, a model that emitted invalid JSON once often emits it again. A live
`deepseek-flash` turn lost eleven minutes of work to
`invalid tool arguments: expected value at line 1 column 13`, and the retry
had already been spent reproducing the same class of output.

Two further facts matter:

- The error carried no sample of what the model sent, so a model-side JSON
  error and a host-side accumulation bug looked identical.
- Some OpenAI-compatible endpoints stream `function.arguments` in cumulative
  mode: each delta resends the full arguments-so-far. Appending every delta
  builds `{...}{...}{...}`, which cannot parse. The accumulator assumed
  incremental fragments.

## Decision

1. `ToolCallAccumulator` treats a delta that extends the accumulated buffer
   as a cumulative resend and replaces the buffer; an exact repeat leaves one
   copy. Genuine incremental fragments still append. The Anthropic adapter is
   unchanged: its wire is always incremental.
2. The parse error names a bounded sample of the received arguments, so the
   next occurrence can be diagnosed from the message alone.
3. When the driver retries an `InvalidResponse` whose message starts with
   `invalid tool arguments`, it appends a user note naming the failure and
   asking for a complete, valid call. The note goes into the request and into
   the turn's message list; it is not persisted as a transcript turn.

Cumulative and incremental deltas are told apart by "the delta starts with
the whole buffer". In incremental mode that would require the model to repeat
every argument byte emitted so far, which does not happen in practice.

## Consequences

- A malformed call is now corrected with information instead of by chance,
  so the retry is a second attempt rather than a coin flip.
- A cumulative endpoint no longer corrupts every multi-chunk tool call.
- The retry costs one extra model call and a short note in the context.
- The error message is longer; a tool-argument sample is shown with the
  failure. It is bounded to 160 characters.

## Alternatives rejected

- Failing the turn on a malformed call: the work before the call is already
  done, and the failure is a model-output flake.
- Repairing the JSON in place: a guess can invent arguments, and a tool must
  not run on invented arguments.
- Turning the malformed call into a tool result: the wire has no tool call to
  attach it to, and inventing one would let a name run without arguments.
