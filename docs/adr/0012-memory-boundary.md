# ADR 0012 — Memory mechanisms live in the kernel; memory backends are ports

## Status

Accepted.

## Context

Public agent-memory systems add vector databases, temporal knowledge graphs,
LLM fact-extraction pipelines, embedding services, MCP servers and
cross-session stores. Some are real engineering; some are wrappers; their
benchmarks are vendor-run and disputed (see
`docs/llmwiki/memory-comparison.md`).

Keel is a kernel. Its value is the opposite of an unbounded memory product: a
bounded, deterministic projection with explicit budgets, provenance and
failure paths. Adopting a vendor memory stack into the kernel would make
context assembly depend on a lossy, model-driven, eventually consistent
service, add hidden model calls to the hot path, and couple the kernel to one
backend. That contradicts the architecture rules.

The kernel still needs to say what "memory" means, what it owns, and where an
external backend may attach.

## Decision

### 1. Kernel memory layers

- **Working memory**: the open segment's detail window, the rolling memo, and
  the pinned task state.
- **Episodic memory**: segments, structured `SegmentRecord`s, raw entries, and
  recall excerpts.
- **Semantic memory**: scoped facts with provenance, supersede and forget.
  Scopes are `Session`, `Workspace` and `Global`.
- **Procedural memory**: static persona and policy. The kernel does not learn
  skills.

### 2. Kernel-owned mechanisms

The kernel owns segmentation, the compaction ladder, the memo budget and
rewrite, IDF lexical recall, structural fact extraction and scoping, task
state, audit and fallback paths. These are deterministic for the same inputs
and profile, bounded by the context budget, and visible in the event log.

### 3. External memory is a port, not a kernel dependency

- `MemoryStore` in `docs/spec/01-kernel.md` is the only extension contract:
  `recall(query, limit)` and `write(record)`.
- Vector stores, graph databases, embedding services, LLM extraction
  pipelines, MCP memory servers and cross-session backends are **adapters or
  plugins** under `ports/`, `adapters/` or `plugins/`. They never live in
  `keel-core` or `keel-runtime`.
- An adapter returns typed, bounded results. The kernel treats memory content
  as data, never as instructions; it applies the same permission, cancellation,
  audit and budget rules as any other input.
- If an adapter fails or exceeds its allowance, the kernel degrades to the
  built-in lexical recall and structured records. It never blocks a turn on an
  optional memory service.
- No external memory type appears in the kernel API. A host that wants
  semantic or graph memory opts in through a profile/plugin and accepts that
  the backend's quality and dependencies are its own.

### 4. Persistence is a host adapter

M5 persistence stores sessions, summaries, facts and todos in SQLite (ADR
0008). The store is an adapter; the in-memory types and kernel invariants do
not change. `Workspace` and `Global` facts become cross-run only when the host
loads and stores them.

### 5. No agent self-edit of kernel memory

The model cannot freely rewrite kernel memory. A future memory tool, if any,
is host opt-in, goes through the normal tool policy and audit, and cannot alter
the kernel's segmentation or budget rules. This is the main auditability
tradeoff against agent-managed memory such as MemGPT/Letta.

### 6. Evaluation lives outside the kernel

Memory quality is evaluated at the host/eval layer with a LongMemEval-style
set: information extraction, multi-session reasoning, temporal reasoning,
knowledge updates and abstention. The kernel contributes deterministic tests
for extraction, fact supersede/forget and recall abstention; multi-session and
temporal cases wait for M5. LoCoMo leaderboard numbers are not a design target.

## Consequences

- The kernel stays thin, deterministic and backend-agnostic.
- Keel will not have vector or graph memory by default; a host can add it
  without changing kernel code.
- Memory claims from an external adapter are attributable to that adapter, not
  to Keel. The kernel only promises bounded context, provenance and fallbacks.
- Adding a backend is a plugin/port change, reviewed against the same clean-room
  and audit rules.
- The memory comparison note can cite public systems without importing them.

## Alternatives rejected

- Embed a vector database or graph store in the kernel: breaks determinism,
  adds a dependency and a vendor, and puts a lossy service on the hot path.
- Let the model write memory freely: loses scope, provenance and audit.
- Copy MemGPT, Mem0, Zep, A-MEM or MemoryOS: clean-room violation and the wrong
  layer; those are host/plugin capabilities.
- Make semantic recall mandatory: the current lexical IDF path is sufficient
  for named details, and the evaluation has not shown a mandatory gap.
