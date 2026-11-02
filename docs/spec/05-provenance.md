# 05 — Provenance

This project is an independent implementation. Keep this file current; it is
the evidence trail for the clean-room claim.

## Rules

1. Every external design input gets a row: source, license, date, what was used.
2. Code, tests, docs, configuration and data from another repository are never
   copied.
3. Public sources are allowed only if their license permits the intended use.
4. AI tools may be used, but the output is reviewed, tested and owned by the
   author. Do not rely on "AI-generated" as a legal shield.
5. If a behavior is inspired by a public source, record the public source, not
   a private one.

## Source ledger

| Date | Source | License | Used for | Notes |
|---|---|---|---|---|
| 2026-10-31 | Apache License 2.0 text | Apache-2.0 | `LICENSE` | Standard license text |
| 2026-10-31 | HumanEval+ / MBPP+ (EvalPlus) | Apache-2.0 | Planned code-correctness smoke cases | Downloaded locally, not vendored |
| 2026-10-31 | BigCodeBench | Apache-2.0 | Planned library-usage cases | Downloaded locally, not vendored |
| 2026-10-31 | SWE-bench harness | MIT; each repository keeps its license | Planned later repo-level cases | Run in isolated workspaces; no code copied |
| 2026-10-31 | Anthropic context-editing documentation | Public documentation | Design reference for threshold-triggered tool-result clearing and summarization | No code copied; thresholds re-derived for Keel (ADR 0006) |
| 2026-11-01 | MemGPT / Letta papers and docs | Public papers/docs | Memory comparison note | Comparison only; no code or prompt copied |
| 2026-11-01 | Mem0 paper, blog and benchmark discussion | Public papers/docs | Memory comparison note | Comparison only; vendor claims treated as unverified |
| 2026-11-01 | Zep / Graphiti paper | Public paper | Memory comparison note | Comparison only; no code copied |
| 2026-11-01 | A-MEM and MemoryOS papers | Public papers | Memory comparison note | Comparison only; no code copied |
| 2026-11-01 | LongMemEval and LoCoMo papers, LoCoMo audit | Public papers/audit | Memory evaluation critique | Comparison only; no dataset vendored |
| 2026-11-01 | DeepSeek Harness memory plugin pages (npm/GitHub) | Public package pages | Landscape survey | Comparison only; no plugin code copied |
| 2026-11-04 | Claude Code settings documentation (public) | Public documentation | Design reference for a home-directory state root with an environment override | No source read or copied; Keel uses `KEEL_HOME` and `~/.keel`, implemented independently |
| 2026-11-05 | Anthropic Messages API documentation (public) | Public documentation | Streaming event and tool-use mapping for the Anthropic adapter | No code copied; the wire mapping is implemented independently |

## Excluded materials

| Material | Why excluded |
|---|---|
| Any previous employer/client repository | Ownership and confidentiality |
| Internal design docs, tickets, metrics, customer data | Confidentiality |
| Reference implementations on local disk | Copying risk |
| Employer tests, prompts, datasets | Ownership and confidentiality |

## Asset ledger

| Asset | Source | License | Date |
|---|---|---|---|
|  |  |  |  |

## AI tool usage

- Tools used:
- Models used:
- Human review:
- Tests run:

## Statement

No code, document, test, configuration or data from a previous employer or
client project is included in this repository. Any similarity is to general
engineering patterns, not to a specific implementation.
