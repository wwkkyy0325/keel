# 03 — Modules and milestones

## Design principle

Directories exist only when something lives there. A crate boundary is an API
boundary, so it is created when a second consumer appears, not in anticipation.

- `crates/` holds reusable libraries.
- `hosts/` holds executables. A binary is not a library, even though Cargo can
  build both from any workspace member.
- `variants/` holds composition data, not kernel code.
- `plugins/` holds optional capability crates or packs.
- `evals/` holds the evidence.
- `docs/` holds the contracts.

## M1 layout (start here)

```
Keel/
  Cargo.toml
  crates/
    keel-core/          # types, traits, errors, events; no Keel dependencies
    keel-runtime/       # turn FSM, tool dispatch, context assembly,
                        # policy module, permission module, mock model
  hosts/
    keel-cli/           # binary: load a profile, run one turn
  variants/
    coding/profile.toml
    companion/profile.toml
  evals/
    cases/
    harness/
  docs/
    spec/
    adr/
```

Only three workspace members: two libraries and one binary. `policy`,
`permissions` and `model` are modules inside `keel-runtime` for M1.

## Target layout

```
Keel/
  Cargo.toml
  crates/
    kernel/
      keel-core/
      keel-runtime/
      keel-permissions/
    middle/
      keel-policy/
      keel-compose/
    ports/
      keel-model-api/
      keel-memory-api/
    adapters/
      keel-model-openai/
      keel-model-anthropic/
    plugins/
      keel-plugin-api/
    keel/                    # facade: the only stable surface for variants
  hosts/
    keel-cli/
    keel-server/
  plugins/
    coding-tools/
    companion-persona/
  variants/
    coding/
      profile.toml
      plugins.toml
      prompts/
    companion/
      profile.toml
      plugins.toml
      prompts/
  evals/
    cases/
      coding/
      companion/
    harness/
    scorers/
    reports/
  docs/
    spec/
    adr/
  xtask/                     # later: release, migrate, eval runner
```

The layer directories under `crates/` make the dependency direction visible.
They are not required at M1; promote modules to them only when the split
triggers below fire.

## Directory responsibilities

| Path | Purpose | Contains | Must not contain | M1 |
|---|---|---|---|---|
| `crates/` | Reusable libraries | core, runtime, policy, model, plugin API | binaries, variant data, prompts | yes |
| `hosts/` | Executables | CLI, server, device/game hosts | kernel mechanisms, business logic | yes (CLI) |
| `variants/` | Composition | profile.toml, plugins.toml, prompts, optional evals | kernel code, shared tools | yes (two profiles) |
| `plugins/` | Optional capabilities | tool/channel/memory packs | permission enforcement, audit | no |
| `ports/` | Stable contracts for external capabilities | memory, model, channel ports | vendor SDKs, storage engines, extraction pipelines | no |

Memory backends are ports/adapters/plugins, not kernel code. A vector store,
temporal graph, embedding service or MCP memory server lives under `ports/`,
`adapters/` or `plugins/`; the kernel keeps only lexical and structured memory
(ADR 0012).
| `evals/` | Evidence | cases, harness, scorers, reports | production code, model weights | skeleton |
| `docs/spec/` | Behavioral contracts | brief, kernel, profile, modules, evals, roadmap | code, internal data | yes |
| `docs/adr/` | Decisions | context, decision, consequences, alternatives | tutorials, logs | yes |
| `xtask/` | Dev automation | release, migrate, eval runner | runtime logic | no |
| `.github/workflows/` | CI | fmt, clippy, test, deny, bench | business code | no |
| `migrations/` | Schema migrations | inside the owning db crate | business logic | no |
| `assets/` | Static assets | icons, sample data | prompts (use `variants/*/prompts/`) | no |

## Root files

| File | Purpose |
|---|---|
| `Cargo.toml` | workspace, shared dependencies, resolver |
| `Cargo.lock` | committed for binaries/workspace reproducibility |
| `README.md` | problem, architecture, how to run |
| `CLAUDE.md` | rules for the implementation session |
| `CONTRIBUTING.md` | workflow and verification |
| `PROVENANCE.md` | clean-room source statement |
| `LICENSE` | MIT |
| `.gitignore` | target, secrets, data, logs |
| `.editorconfig` | line endings, indentation |
| `rustfmt.toml`, `clippy.toml`, `deny.toml` | format, lint, dependency/license gates |

## Dependency direction

```
keel-core
  ^        ^          ^
  |        |          |
runtime   policy    plugins
  ^        ^          ^
  +--------+----------+
           |
        hosts (cli, server)
```

- `keel-core` depends on no other Keel crate.
- `keel-runtime` depends on `keel-core`.
- Libraries never depend on a host.
- `hosts/` depends on libraries; the reverse is forbidden.
- No cycles. Enforced by review and `cargo deny`.

## Crate split triggers

Do not split a module into a crate until one of these is true:

| Trigger | Split |
|---|---|
| Second model provider | `keel-model` |
| Second memory backend | `keel-memory` |
| Second plugin or a stable plugin API | `keel-plugins` |
| Permissions exceed ~500 lines or need independent tests | `keel-permissions` |
| Profile schema exceeds ~300 lines or has multiple consumers | `keel-policy` |
| Second host (server, game, device) | `keel-server` / `hosts/` |
| Variants should not depend on internal crates | add `keel` facade |

If a split creates a cycle, the layer boundary is wrong — fix the boundary, not
the crate graph.

## First vertical slice (M1)

```
user text
  -> context assembly (system + latest)
  -> mock model
  -> one tool call (echo)
  -> tool result
  -> final text
  -> event log
```

Acceptance:

- `cargo test -p keel-runtime` passes;
- FSM, cancellation, timeout and tool-error tests pass;
- kernel overhead benchmark is recorded;
- no network, no real model.

## Milestones

| Milestone | Deliverable | Done when |
|---|---|---|
| M0 | repo, license, specs, provenance | docs reviewed; no code |
| M1 | core types + mock loop + CLI | 20 FSM/tool/cancel tests |
| M2 | one real provider + coding profile | one coding task passes |
| M3 | companion profile | 20-turn chat, no kernel changes |
| M4 | plugin manifest + registry | coding/companion packs load/unload |
| M5 | permissions + memory + compaction | fault-injection tests pass |
| M6 | evals + portfolio package | reports reproducible |

## API versioning

- Plugin API is semver.
- A breaking change requires a major version and an ADR.
- Deprecated fields remain readable for one minor cycle.

## Async and performance

- Kernel hot path is concrete and static; `dyn` only at plugin boundaries.
- No per-token allocation or JSON serialization.
- Context assembly is O(n) in messages, not O(n^2).
- Bounded channels and timeouts everywhere.

## What not to build in M1

- UI, channels, A2A, Android, voice.
- Vector memory, KG, multi-agent.
- More than three tools.
- More than two context stages.
- Empty directories for future features.
