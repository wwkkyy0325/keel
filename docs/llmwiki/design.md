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

## M1 workspace

| Member | Responsibility |
|---|---|
| `crates/keel-core` | IDs, messages, tool protocol, model wire, policy types, events, errors |
| `crates/keel-policy` | Profile schema, resolution, `RuntimeConfig`, `TurnLimits` |
| `crates/keel-runtime` | FSM, turn driver, context builder, dispatch, permission module, verifier, mock model |
| `hosts/keel-cli` | Loads a profile and runs one mock turn |

M1 has no network, no real model, no plugin lifecycle, no memory backend and no
UI. The FSM is exercised with a scripted mock model and three tools (`echo`,
`read_file`, `write_file`).

Status: M1 implemented, including the permission refactor and interaction
primitives. 86 tests pass; the mock turn overhead is p50 2 us and p99 9 us over
1000 turns. The next milestone is M2 (one real provider and the coding profile).

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
