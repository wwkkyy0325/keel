# Keel — design summary

Keel is a thin agent kernel with interchangeable variants. The kernel owns
mechanisms and invariants; the middle layer resolves policy profiles into an
immutable `RuntimeConfig`; plugins add optional capabilities.

## Layers

- `kernel`: turn FSM, tool protocol, context assembly, permission enforcement,
  cancellation, event log and model wire. It never knows variant names.
- `middle`: profile resolution and the seven policy questions
  (`visible_tools`, `context_recipe`, `thinking_budget`, `verification_mode`,
  `memory_policy`, `model_route`, `before_commit`).
- `plugins`: tools, channels, memory backends and UI. They never bypass
  permissions, audit or cancellation.

## Workspace

| Member | Responsibility |
|---|---|
| `crates/keel-core` | IDs, messages, tool protocol, model wire, policy types, events, errors |
| `crates/keel-policy` | Profile schema, resolution, `RuntimeConfig`, `TurnLimits` |
| `crates/keel-model-openai` | OpenAI-compatible wire mapping and streaming provider |
| `crates/keel-runtime` | FSM, turn driver, context builder, dispatch, permission module, verifier, tools, mock model |
| `hosts/keel-cli` | Loads a profile, runs one turn, terminal interactions |

Status: M3 complete except for the summarization research. Sessions,
budget-driven history assembly, the context governor, tool summaries, segment
records, three-tier facts and the turn `Answer` artifact are implemented. A
real 20-turn DeepSeek companion run at an 8k window finished 20/20, and a
20-turn coding session with four tools at 16k finished 20/20. 199 tests
pass; the self-authored suite is 10/10 and the public baseline is HumanEval+
20/20 and MBPP+ 19/20.

## Permission and interaction model

- Two levels: `WorkspaceAuto` (logical sandbox) and `FullAuto` (no workspace
  boundary, hard deny list still applies).
- Tools declare permission requests; the dispatcher enforces before `call`.
- Plan mode and read-only are tool visibility, not permission levels.
- Deletion is soft: a `TrashStore` backs up overwritten files, and there is no
  hard-delete fallback.
- Questions, plan approval, permission confirmation and escalation are typed
  interactions. The kernel brokers deadlines and audit; the host composes the
  frontend.
- The acceptance checklist is an ADR/spec contract; verifiers land with M2.

## History and memory (M3)

- A session owns segments; a segment is a task topic with raw entries while
  open and a structured record once sealed.
- Segmentation is a kernel-owned deterministic heuristic; the model cannot
  open or seal a segment and the user never manages compression.
- A context governor measures headroom in calls at the observed growth rate
  and acts cheap-first; assembly keeps recent turns in detail, older turns as
  digests and sealed segments as records.
- `budget_tokens` is the working-set target and `context_window` is the hard
  capacity; a 1M window does not mean a 1M working set.
- Tool entries keep the call, status and head and tail by character count; the
  full output spills to `.keel/outputs/` and stays reachable.
- Facts are sedimented at a seal: durable ones become global, the rest stay
  session-scoped.
- `L2` and `L3` are structured extracts until the summarization research
  decides whether a model call is worth its cost.

See ADR 0006 and `docs/llmwiki/tasks/m3-history.md`.

## Key decisions

- Dependency direction is `hosts -> runtime -> core`; `core` depends on no
  other Keel crate.
- `RuntimeConfig` is built once per turn from a profile; the kernel only sees
  the `Policy` trait.
- The event log is append-only and records state transitions, tool audits and
  the turn outcome. Sampling belongs to telemetry, which arrives later.
- The kernel never reads a profile file. The host loads it and passes policy
  data to the driver.

See `docs/adr/` for decisions that are expensive to reverse and `docs/spec/`
for behavioral contracts.
