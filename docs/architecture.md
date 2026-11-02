# Architecture

Keel is a thin kernel plus composition data. The kernel owns mechanisms and
invariants; the middle layer turns a profile into policy; plugins add
capability. Detailed contracts live in `docs/spec/` and `docs/adr/`.

## Layers and dependencies

```
plugins (MCP servers, CLIs) ─┐
middle (profiles, policy)  ──┼──> kernel (keel-core, keel-runtime)
hosts (keel-cli)           ──┘
```

- `keel-core`: messages, tools, session/segment/fact types, budget math.
- `keel-runtime`: turn FSM, driver, dispatcher, context stages, governor,
  permissions, acceptance, event log.
- `keel-policy`: profile schema and `RuntimeConfig`.
- `keel-model-openai`: OpenAI-compatible streaming provider.
- `hosts/keel-cli`: composition, SQLite store, MCP adapter, frontend views.

The kernel never tests for a variant name. A profile is resolved once per turn
into an immutable config.

## One turn

1. `Ready`: the driver builds context from ordered stages:
   `system` → `memory` → `task_state` → `history` → `latest`.
2. The model streams text and tool calls. Tool calls go through the
   dispatcher: permission request, execution, audit event, bounded result.
3. Tool results enter history as clipped summaries; long transcripts are
   governed by watermarks and tool headroom.
4. `Converge`: if an acceptance checklist exists, the verifier runs and a
   failed required item reprompts within a limit.
5. The host saves one transaction: snapshot, turn row, context snapshot
   (opt-in), projection, history and checkpoint.

States are explicit; every step appends an event. Cancellation and the turn
deadline terminate the loop without a partial save.

## Bounded context

The model never receives the whole transcript. The context is assembled each
turn from:

- a rolling model-written memo covering sealed segments;
- structured segment records, folded once facts cover their changes;
- scoped facts (session, workspace, global) with provenance;
- lexical recall for absorbed segments that match the query;
- the recent segment in detail, bounded by the governor.

The governor reserves room for the model's output and for tool results, and
elides old tool outputs before dropping conversation turns. The design is
append-only in storage and bounded in the prompt; see ADR 0006, 0007, 0012.

## Memory

A segment seals on acceptance, topic change or the turn cap. At the seal the
kernel builds a record, extracts constraints and artifact facts, and the host
asks the model for a new memo revision. Facts are versioned rather than
deleted; an explicit update cue replaces the value for the same subject.
Recall is lexical (IDF-weighted), deliberately not vector-based in the kernel.

## Tools and permissions

The registry holds typed tools; each declares risk and permission requests. Two
permission levels (`workspace_auto`, `full_auto`) share a structural deny list
for the agent's own state and operating-system paths; a credential name is
refused outside every root and asks for confirmation inside one. Deletes move
to a trash store. Interactions
(question, plan, confirmation, escalation) are broker primitives, so a host can
present them. The dispatcher enforces the gate before any tool runs and appends
an audit event.

## Storage and read models

The host store is SQLite in WAL mode. One transaction per turn writes:

- `sessions.snapshot_json` — the kernel resume format;
- `turns` — the transcript row with the structured answer;
- normalized projection tables (`segments`, `records`, `facts`, `todos`, ...);
- append-only history (`fact_history`, `todo_history`, memo revisions);
- `events` — the kernel event stream for the turn, and `turns.error` when the
  turn did not finish;
- `session_checkpoints` — the state after the turn, used for branches.

`session_current` and the other tables are derived; there is no stored status
column. A frontend consumes composed views (`--view sessions|session|turn|
memory`), never the snapshot. Branching copies a checkpoint and rebuilds the
projection; deleting a session removes its own rows and leaves branches. ADR
0008 and `docs/llmwiki/frontend-contract.md` define the details.

## Plugins

An MCP server is an external process or a remote streamable-HTTP endpoint. The
host starts it, runs the handshake, lists tools and registers each as an
ordinary kernel tool with a permission gate. Nested schemas become one JSON
argument with a generated field/type/example description. Server-initiated
`ping` and `roots/list` are answered; sampling is refused. Configuration is
restart-based and gated by `--plugins-check`; child processes are killed as a
tree. ADR 0013.

## Deliberately outside the kernel

Vector stores, graph memory, embeddings, MCP itself, frontend rendering and
retention policy are host or plugin concerns. The kernel exposes snapshots and
typed tools; it does not grow adapters for a specific vendor.
