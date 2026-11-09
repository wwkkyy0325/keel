# ADR 0033 — A model interaction names its recommended option

## Status

Accepted and implemented in `keel-core`, `keel-model-openai` and
`keel-model-anthropic`.

## Context

A `choice` interaction renders every option as a button. The model that asks
usually knows which option it would pick, but the request had no field to say
so, and the user had to read every label to answer. `InteractionRequest`
already carried a `default` field that no provider parsed and no host read.

## Decision

1. `InteractionRequest::default` becomes `recommended: Option<String>`, the id
   of the option the model recommends.
2. The control tool accepts `recommended` (a string). Both providers parse it;
   both advertise it in the tool schema.
3. The kernel never answers for the user. The field is advisory: a host marks
   the option, and the user still chooses.
4. A `recommended` id that names no option is not an error. The host simply
   marks nothing, so a model that guesses still produces a usable card.

## Consequences

- The desktop marks the option with a badge and a stronger border; the model
  names its pick in a field rather than in a label's wording.
- Permission approvals and escalations have no recommendation; the field stays
  `None`.
- `recommended` is a string id, so a provider cannot validate it against the
  options without the host's help; the host compares it to the option ids it
  renders.
