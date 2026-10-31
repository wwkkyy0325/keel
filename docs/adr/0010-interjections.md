# ADR 0010 — Interjections are queued user messages delivered at safe points

## Status

Proposed. The contract is fixed here; the implementation lands with an
interactive host. The current CLI is line-based and has no mid-turn reader.

## Context

A turn can run for many model calls and tool calls. The user may want to add a
constraint, correct a direction or stop the work while it is running.

The kernel already has three related mechanisms:

- `CancelToken` abandons the turn; the FSM reaches `Cancelled`.
- Interactions are kernel primitives for permission and questions; they are
  requested by the kernel, not by the user.
- The CLI reads one line per turn, so messages typed during a turn wait in
  standard input and run as the next turn. This is a between-turn queue, not
  steering.

Cancelling loses the work in flight. Waiting until the next turn is safe but
slow. A separate tool for user messages would confuse the tool protocol.

## Decision

An interjection is a user message that arrives while a turn is running. It is
not an interaction and it does not create a new FSM state.

### Queue

- The host owns a bounded FIFO per session (default four items).
- Each item carries the text, arrival time and an id. Duplicates are dropped.
- The queue is host-facing; the kernel exposes `push` and a delivery hook.

### Safe points

The driver may deliver queued items only:

1. at the top of the model-call loop, before building the next request;
2. after a tool round completes and its results are recorded;
3. before the reserved final call.

Never during a model stream, a tool call, an interaction wait or verification.
A delivery never splits an assistant tool-call message from its tool results.

### Delivery

- The item enters the message list as a user message marked
  `[user interjection] ...`.
- The current turn continues; the model may adapt its plan, update the task
  list or answer.
- The final answer's artifact notes any interjection that arrived too late to
  deliver.

### Modes

The host chooses the policy; the kernel only provides the queue and the hook:

- `queue`: deliver at the next safe point (soft steering);
- `interrupt`: cancel the current turn, then start a new turn with the message;
- `next_turn`: never inject mid-turn; run it after the current turn.

### Final call

If the final call is already reserved or has started, the interjection goes to
the next turn. The kernel does not re-enable tools or extend the call budget
for an interjection.

### Audit

`InterjectionQueued`, `InterjectionDelivered` and `InterjectionDropped` carry
the id and turn. Dropped items are reported; they are not silently forgotten.

## Consequences

- The turn loop reserves three safe points and one queue; a later host can add
  steering without changing the FSM.
- Cancellation and interjection stay distinct: one abandons the turn, the other
  informs it.
- The kernel does not read stdin or render UI; those remain host concerns.
- Tests will cover delivery before the next call, waiting during a stream,
  interrupt mode, final-call queueing and no split of tool pairs.

## Alternatives rejected

- Inject at any point, including during a stream: breaks the provider protocol.
- Treat interjections as interactions: they are user-initiated and need no
  model request.
- Only cancel and restart: loses in-flight work and the task state context.
- A `user_message` tool: the model would have to call a tool to receive a user
  message, which inverts the direction of the protocol.
