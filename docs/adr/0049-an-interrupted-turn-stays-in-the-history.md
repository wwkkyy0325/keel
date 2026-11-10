# ADR 0049 — An interrupted turn stays in the history

## Status

Accepted and implemented in `keel-runtime`. Released as 0.2.8.

## Context

A cancelled turn used to be left out of the session history: the driver
recorded a turn only when its state was not `Cancelled`. The host still
stored the row for display, so the transcript showed the user's question
while the model's context did not.

A live session showed the cost. The user asked a question, cancelled the
turn, and then sent `继续`. The model continued the previous completed turn,
because the cancelled input was never part of its history. Nothing in the UI
says "the model never saw this", so the reply read as an ignored message.

## Decision

1. Every turn is recorded, including a cancelled one. The record keeps the
   user input; when part of an answer was streamed, the body is that text
   followed by `[the user interrupted this turn; the answer is incomplete]`;
   tool summaries are included as for any turn.
2. A cancelled turn with no streamed text records the input alone.
3. The turn outcome's answer body carries the streamed partial text, so a
   host displays what was said instead of dropping it.
4. The turn state, events and FSM are unchanged; a host can still style the
   turn by its `Cancelled` state.

## Consequences

- `继续` after an interruption now refers to the interrupted question, which
  is what a user means by it.
- Interrupted turns count toward the segment's turns and can be summarized
  or absorbed like any other turn; they are part of the conversation.
- A partial answer is visible to the user and marked incomplete to the
  model, so it is not mistaken for a finished one.
