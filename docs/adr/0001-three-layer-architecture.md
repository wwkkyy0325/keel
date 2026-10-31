# ADR 0001 — Three layers: kernel, middle, plugins

## Status

Accepted.

## Context

Keel must support coding, companionship and future variants without forking a
single growing agent loop. Variants differ in tool visibility, context recipe,
thinking budget, verification, memory policy and model route.

## Decision

Split the system into three layers:

1. **Kernel** — mechanisms and invariants. Owns the turn FSM, tool protocol,
   context assembly, permission enforcement, cancellation, event log, model wire
   and plugin lifecycle.
2. **Middle** — policy and composition. Resolves a profile into an immutable
   `RuntimeConfig`, answers the kernel's policy questions, selects plugins.
3. **Plugins** — optional capabilities. Tools, channels, memory backends, model
   adapters, UI.

The kernel never knows variant names. The middle layer never contains hidden
kernel logic. Plugins never bypass permissions.

## Consequences

- A second variant must run without kernel changes.
- Hot paths stay concrete; boundaries are coarse.
- Every extension point needs a contract test.
- A profile is data, not code.
- The middle layer can be tested independently of plugins.

## Alternatives rejected

- **One loop with variant branches**: grows into a monolith and makes variants
  fight.
- **Everything as a plugin**: core semantics fragment and every variant
  reimplements permissions, context and the loop.
- **Fork per variant**: unbounded maintenance and no shared fixes.
