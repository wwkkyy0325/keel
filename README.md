# Keel

A thin agent kernel with interchangeable variants.

Keel is a clean-room project. It keeps the load-bearing mechanisms of an agent
runtime — turn state machine, tool protocol, context assembly, permissions,
memory, model wire — small and stable, while variants (coding, companionship,
automation) arrive as policy profiles and plugins.

**Status:** M1 implemented; M2 in progress — OpenAI-compatible streaming
provider, acceptance verifier (compute, file hash, command and test), six
coding tools and profile-driven provider selection. The first real coding eval
is next.

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

## Run

```bash
cargo run -p keel-cli -- --profile variants/coding/profile.toml --input "hello"
cargo run -p keel-cli -- --profile variants/companion/profile.toml --input "hello"
```

The M1 mock model makes no network calls. The coding profile exercises one
`echo` tool call; the companion profile exercises the no-tools policy.

For DeepSeek (`deepseek-flash`, OpenAI-compatible):

```bash
cp .env.example .env
# edit .env and set DEEPSEEK_API_KEY only
cargo run -p keel-cli -- --profile variants/coding/profile.deepseek.toml --input "hello"
```

The profile carries `base_url = "https://api.deepseek.com"` and
`model = "deepseek-flash"`; only the key belongs in `.env`.

For an OpenAI endpoint:

```bash
cp .env.example .env
# edit .env and set OPENAI_API_KEY
cargo run -p keel-cli -- --profile variants/coding/profile.openai.toml --input "hello"
```

The CLI loads `.env` automatically. Credentials stay in `.env` or the OS
keyring, never in the chat or a profile.

The workspace defaults to the current directory. Use `--workspace PATH` to run
against a separate eval workspace while still loading `.env` and the profile
from this repository:

```bash
cargo run -p keel-cli -- \
  --workspace /tmp/keel-task-001 \
  --profile variants/coding/profile.openai.toml \
  --input "fix the mean function"
```

M1 uses two permission levels: `workspace_auto` is the logical sandbox and
`full_auto` removes the workspace boundary while keeping the hard deny list.
Deletion is recoverable through the trash; interactions (questions, plan
approval, permission confirmation and escalation) are kernel primitives, not
domain tools. See ADR 0002 and ADR 0003.

## Secrets

Keys never go into the chat or the repository. Put credentials in `.env`
(gitignored; copy `.env.example`) or the OS keyring. Input is never blocked;
when it matches a secret pattern, the turn records a `SecretWarning` event and
the CLI prints the notice with that turn's result. Test keys and hashes trigger
the same patterns, so blocking would make the CLI unusable. The event log
redacts detected secrets. If a real key was ever pasted into a chat, rotate it.

## Verification

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo deny check
```

## License

Apache-2.0. See `LICENSE`.
