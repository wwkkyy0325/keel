# 00 — Brief

## Problem

Agent runtimes tend to grow into a single monolith: every new use case adds
tools, prompt branches and special cases to one loop. Keel separates the
mechanisms that must be stable from the policies that make a variant feel
different.

## Users

1. **Variant authors** — build coding, companionship or automation products on
   the same kernel without forking it.
2. **Host authors** — embed the kernel in a CLI, web server, game or device.
3. **End users** — experience a narrow, coherent agent, not a jack-of-all-trades.

## Variants

- **Coding** — shell, files, git, tests, verification gates, higher tool budget.
- **Companion** — persona, episodic memory, low latency, no shell, no
  verification gates.

More variants are expected; the kernel must not know their names.

## Non-goals

- No UI, channel implementations or domain tools in the kernel.
- No employer-specific features, vocabulary, data or documents.
- No attempt to be a general workflow engine.
- No multi-tenant SaaS control plane in the first milestone.

## Success criteria

1. A second variant runs without kernel code changes.
2. Kernel overhead stays within the budgets in `04-evals.md`.
3. Every extension point has a contract test.
4. The coding variant solves a small task set; the companion variant survives a
   20-turn persona test.
5. The repository contains no code or data from another project.

## Constraints

- Rust, edition 2021, MSRV declared in the workspace.
- One process by default; plugins may be in-process first.
- Cloud and local OpenAI-compatible models are both first-class.
- A 6 GB GPU + 32 GB RAM laptop is a supported local target.

## Clean-room statement

This is an independent implementation. Design inputs are this spec set and the
public sources recorded in `05-provenance.md`. No previous employer or client
repository, document, test, configuration or data is used.

## Open questions

Record unresolved questions in `open-questions.md`, not in this file.
