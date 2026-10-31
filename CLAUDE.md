# CLAUDE.md — Keel

## Project identity

Keel is a clean-room, thin agent kernel with interchangeable variants. The
kernel owns mechanisms and invariants; the middle layer owns policy and
composition; plugins own optional capabilities.

## Clean-room rules (hard)

1. Do not read, copy or reference any other repository on this machine.
2. Do not use code, documents, tests, configuration, data, customer details or
   internal vocabulary from any previous employer or client project.
3. The only design inputs are `docs/spec/` and public sources recorded in
   `docs/spec/05-provenance.md`.
4. If a behavior is unclear, write the question in
   `docs/spec/open-questions.md`; do not guess from memory of another project.
5. New code, comments, docs and commit messages are original work.

## Language

- Code, comments, commit messages and all written artifacts: English.
- Conversation with the user: Chinese.
- Structural labels and status markers: English.

## Architecture rules

- Dependency direction: `plugins -> kernel`; `middle -> kernel`; the host
  composes all three. The kernel depends on neither middle nor plugins.
- `crates/` holds reusable libraries; `hosts/` holds executables. Libraries
  never depend on a host.
- The kernel must not contain `if variant == ...`. Variant behavior is resolved
  from a profile into an immutable `RuntimeConfig` once per turn.
- Plugins never bypass permission, audit, cancellation or secret handling.
- Hot paths stay concrete: no per-token trait objects, no per-token JSON, no
  synchronous audit writes. Boundaries are coarse: turn, tool call, context
  build, model stream chunk.
- Every extension point has a contract test.
- Do not create a crate or directory before it has content. Split a module
  into a crate only when a second consumer appears (see
  `docs/spec/03-modules.md`).

## Coding rules

Read `docs/coding-rules.md` before writing code. The short version:

- `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`, `cargo deny check`.
- No `unwrap`/`expect` on user input, network, filesystem or model output.
- No blocking I/O inside async tasks; use timeouts and cancellation.
- Typed data at the core; `serde_json::Value` only at wire boundaries.
- Share messages with `Arc`; do not clone the full history per turn.
- No secrets in code, logs, tests or git. Use `.env` (gitignored) or the OS
  keyring.
- Comments explain why, not what.

## Safety rules

1. No `dd`, `mkfs`, flash, format, `fdisk`, `parted`, reboot or shutdown
   without explicit user confirmation.
2. The agent never gets a raw root shell. High-privilege actions go through a
   permission broker and are audited.
3. Natural language is never concatenated into a shell. Use structured
   parameters, allowlists and no shell interpolation.
4. Send, delete, payment and secret upload require confirmation.
5. Keys never enter git.

## Git rules (hard)

- Commit messages are English, conventional commits (`feat:`, `fix:`, `chore:`,
  `refactor:`, `docs:`, `test:`).
- Zero AI traces: no `Co-Authored-By`, no "Generated with Claude Code", no AI
  mentions, no tool signatures, no emoji.
- Commit only when the user asks.
- Small, focused commits; each commit compiles and passes tests.
- Never commit `.env`, keys, tokens, generated artifacts or `target/`.

## Verification before claiming done

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo deny check
```

For performance-sensitive changes, run the relevant benchmark and compare with
the budgets in `docs/spec/04-evals.md`.

## Documentation rules

- Design decisions go in `docs/adr/`.
- Behavioral contracts go in `docs/spec/`.
- A change that shifts a contract updates the spec in the same commit.

## First task

Read `docs/spec/00-brief.md` through `docs/spec/06-roadmap.md`, restate the
kernel invariants and the M1 file plan, then implement M1 only.
