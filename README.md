# Keel

A thin agent kernel with interchangeable variants.

Keel is a clean-room project. It keeps the load-bearing mechanisms of an agent
runtime — turn state machine, tool protocol, context assembly, permissions,
memory, model wire — small and stable, while variants (coding, companionship,
automation) arrive as policy profiles and plugins.

**Status:** specification phase. No implementation yet.

## Why "Keel"

The keel is the structural spine of a ship: it carries the load, and the hull,
rigging and cabins are built around it. Keel is that spine for agent variants.

## Three layers

| Layer | Responsibility |
|---|---|
| `kernel` | Mechanisms and invariants: turn FSM, tool protocol, context assembly, permission enforcement, cancellation, event log, model wire |
| `middle` | Policy and composition: profile resolution, tool visibility, context recipe, thinking budget, verification mode, memory policy, model route |
| `plugins` | Capabilities: tools, channels, memory backends, UI, domain packs |

The kernel never knows variant names. The middle layer answers the kernel's
policy questions. Plugins add capability and never bypass permissions.

## Layout

- `crates/` — reusable libraries.
- `hosts/` — executables (CLI, server).
- `variants/` — composition data: profiles, prompts, plugin selection.
- `evals/` — capability evidence.
- `docs/` — specs and ADRs.

M1 has three workspace members: `keel-core`, `keel-runtime`, `hosts/keel-cli`.
Crates are split only when a second consumer appears; see
`docs/spec/03-modules.md`.

## Clean-room notice

This repository is an independent implementation. It must not contain code,
documents, tests, configuration, data or internal vocabulary from any previous
employer or client project. All design input comes from `docs/spec/` and public
sources recorded in `docs/spec/05-provenance.md`.

## Start here

1. `CLAUDE.md` — working rules for this repository.
2. `docs/spec/00-brief.md` — problem, users, non-goals.
3. `docs/spec/01-kernel.md` — kernel contract.
4. `docs/spec/02-profile.md` — policy/profile schema.
5. `docs/spec/06-roadmap.md` — milestone order.

## Verification

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo deny check
```

## License

Apache-2.0. See `LICENSE`.
