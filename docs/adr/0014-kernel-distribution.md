# ADR 0014 — Kernel distribution and the reference host

## Status

Accepted. Applies from 0.1.0.

## Context

The repository only builds the kernel. Products derive from it: they depend on
the kernel crates, add their own host, UI, storage and composition, and update
the kernel deliberately. If the repository grows into a complete application,
the kernel contract is buried under product decisions, and every derived
project inherits them.

At the same time the kernel needs an executable that exercises it end to end.
Unit tests do not prove that a profile, a tool loop and a store compose.

## Decision

1. `crates/` is the distributable kernel: `keel-core`, `keel-policy`,
   `keel-runtime` and `keel-model-openai`. These crates never depend on a host.
2. `hosts/keel-cli` is a **reference host**, not the product. It composes the
   kernel, provides a SQLite store, an MCP adapter, composed read models and a
   CLI, and serves as the executable contract test. A derived project may
   replace it entirely.
3. The kernel is versioned as one unit. Before 1.0 a minor may break with an ADR
   and a changelog entry; from 1.0 a breaking change requires a major.
4. The session snapshot carries `format_version`. `Session::restore` refuses a
   newer format instead of guessing; a host that upgrades migrates stored
   sessions.
5. `docs/kernel-contract.md` lists the stable surface, the extension points and
   the contract test for each. CI runs a kernel-only build and test job so a
   host change cannot hide a kernel dependency.
6. A derived project supplies its profile, tools or plugins, store adapter, UI
   and model configuration. It does not add product code here.

## Consequences

- The kernel can be updated on its own schedule; products pin a tag.
- The reference host shows one composition without becoming a second kernel
  API. Its flags, SQLite schema and view JSON are host contracts.
- Kernel changes have a written contract and a test; product features do not
  leak into the kernel.
- Two release artifacts exist: the kernel crates and the reference binary.

## Alternatives rejected

- One repository per product: the kernel would drift and the contract tests
  would disappear.
- Vendoring the kernel into each product: updates become merges of copies.
- Treating the reference CLI as the product: UI and packaging decisions would
  enter the kernel and every derived host would inherit them.
