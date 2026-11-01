# Memory systems: Keel and the public landscape

Last updated: 2026-11-01. This is a comparison note, not a design input copied
from another implementation. No code was read or copied; the sources are public
papers, documentation and package pages. Public sources used here are recorded
in `docs/spec/05-provenance.md`.

## What Keel already has

Keel's memory is session-scoped and kernel-owned. It is not a vector database
bolted onto a prompt.

| Layer | Keel implementation | Where |
|---|---|---|
| Working memory | The open segment's detail window, the rolling memo, and the pinned task state | `context.rs`, `driver.rs` |
| Episodic memory | Segments, structured `SegmentRecord`s, raw entries, recall excerpts | `session.rs` |
| Semantic memory | Scoped facts with provenance, supersede and forget | `session.rs`, ADR 0006 §8 |
| Procedural memory | Static persona and policy; no learned skills | profiles |
| Retrieval | IDF-weighted lexical recall over absorbed segments; artifact paths; recall-intent threshold | ADR 0011 |
| Write path | Kernel seals a segment, extracts structural facts, asks the model to rewrite one memo | `driver.rs`, `summarizer.rs` |
| Consolidation | Memo rewrite, record absorption, fact promotion, fold; no graph or decay | ADR 0006 |
| Budget | Governor, memo 10% of the working set, recall a quarter of remaining history, wrap-up, final call, mid-turn elision | ADR 0006 |
| Audit | Append-only events, memo revisions and coverage, `absorbed_in`, fact provenance | ADR 0008 |
| Scope | Session now; `Workspace` and `Global` facts are designed; persistence is M5 | ADR 0008 |

What it does not have yet: cross-session persistence, embeddings or a vector
index, an entity or temporal graph, user-level personalization, learned
procedural memory, a standard memory benchmark, and conflict resolution across
sessions. `ADR 0008` is still proposed; the in-memory types match it, but SQLite
persistence is M5.

## Comparison

| System | Store and write | Retrieval | Update / forget | Scope and budget | Audit / provenance | External dependency |
|---|---|---|---|---|---|---|
| MemGPT / Letta | Core memory blocks, recall store, archival store; the agent edits memory through tools | Vector + keyword search; recall and archival tools | Agent rewrites blocks; eviction and recursive summary | Agent-managed; context-pressure warnings; no fixed memory budget | Agent decisions are visible as tool calls; no formal fact provenance | Vector DB, agent runtime |
| Mem0 | Fact extraction then consolidation (2026 version is single-pass ADD-only); vector, entity store, SQL history | Semantic + BM25 + entity matching, rank fusion | Older ADD/UPDATE/DELETE; current add-only keeps old and new | Top-k and token budget per query | SQL history; vendor benchmarks; no independent audit | Vector DB, embedding model |
| Zep / Graphiti | Episodes, entities, facts, communities in a bi-temporal graph; LLM extraction | Cosine + BM25 + graph BFS + rerank | Invalidate, not delete; `valid_at` / `invalid_at` | 1.6K-token context in the reported setup | Bi-temporal validity and transaction time | Neo4j/FalkorDB/Kuzu, ingestion LLM calls |
| A-MEM | Zettelkasten notes with keywords, tags, context descriptions, embeddings and links; LLM evolution | Embedding top-k then one-hop link expansion | LLM rewrites neighbours; destructive, no versioning | No explicit context budget | None for evolution; raw content can drift | Embeddings, LLM calls |
| MemoryOS | Short-term, mid-term, long-term persona; FIFO and segmented pages | Tiered retrieval plus persona | Heat-based eviction; persona update | Tiered storage, not a token-budget governor | Research prototype; no formal audit | Storage and LLM calls |
| DeepSeek Harness / MCP memory plugins | Varies: JSON, SQLite, vector DB, graph; hooks extract every N turns or on demand | TF-IDF, embeddings, graph, or hybrid | Forgetting curves, review queues, contradiction tracking in some | Host-level token accounting; varies | Some have audit/review gates, most do not | MCP server, embeddings, DB |
| Keel | Segments, records, memo, scoped facts; kernel writes at seal | Deterministic IDF lexical recall, no embedding | Memo rewrite; fact supersede/forget; no cross-session conflict handling | Kernel governor with explicit shares and elision | Event log, memo revisions, coverage, fact provenance | None for retrieval; model only for the memo |

## Are the public claims real?

Mostly real systems, frequently inflated claims. Both are true at once.

- **MemGPT / Letta** is real engineering: virtual context, agent-managed
  paging, recall and archival storage. Its own DMR result was strong, but DMR is
  a narrow retrieval task and the architecture's quality depends on the model's
  judgment; every memory operation costs inference.
- **Mem0** is a real extraction-and-consolidation pipeline. Its LoCoMo and
  LongMemEval numbers are vendor-reported and disputed; Mem0 and Zep have
  publicly run each other's harnesses with different results. The 2026 rewrite
  dropped consolidation and is ADD-only, which is simpler but keeps
  contradictions.
- **Zep / Graphiti** is a real bi-temporal knowledge graph. Its LongMemEval
  result (71.2% vs 60.2% full-context) is vendor-reported; the paper itself
  reports regressions on knowledge updates, and ingestion is slow and
  operationally heavy.
- **A-MEM** and **MemoryOS** are real research prototypes. A-MEM's link
  evolution is destructive and unversioned; MemoryOS's heat-based tiers are
  promising but not a production memory service.
- **DeepSeek memory plugins** are a mixed field: some are serious (hierarchical
  memory, hybrid retrieval, review gates, audit), many are thin wrappers around
  SQLite/JSON plus an embedding model. The name "memory" does not imply a
  context governor, conflict handling or provenance.

## Are the benchmarks trustworthy?

No single leaderboard is. Treat every headline as directional.

- **LoCoMo** is widely used and has real problems: an audit found 6.4% of the
  golden answers are wrong, the judge accepts a large share of intentionally
  wrong but topical answers, the adversarial category is malformed, and
  conversations are only 16K-26K tokens, within modern context windows. Scores
  above roughly 93% are mathematically suspect, and differences under about 5
  points are noise. A full-context CoT baseline has beaten some "memory"
  systems.
- **LongMemEval** is a better fit: 500 questions across information extraction,
  multi-session reasoning, temporal reasoning, knowledge updates and
  abstention. Vendor numbers disagree sharply (Zep 71.2 vs Mem0 49; Mem0's own
  2026 claim is 94.4), no neutral reproduction is settled, and a brute-force
  chunked RAG reportedly reached about 86. The weakest areas are abstention and
  knowledge updates.
- The honest conclusion: memory systems can beat a naive full-context baseline
  on long interactions, especially for extraction and multi-session recall;
  they do not "remember like a human", and update, temporal, contradiction and
  abstention failures are common.

## Verdict for Keel

Keel is **real but narrow**. It is a deterministic, budgeted, auditable
session-memory and context governor, not a cross-session memory OS:

- strong at named-detail recall, bounded context, task-state separation,
  fallback paths and audit;
- weak at paraphrase and synonymy (lexical only), cross-session memory,
  entity/temporal reasoning, personalization, and standard memory benchmarks;
- deliberately does not let the model freely edit kernel memory; the kernel
  extracts facts and rewrites one memo. This trades flexibility for
  determinism and audit.

So the answer to "is this a memory system?" is yes: working, episodic, semantic
and (static) procedural memory are implemented for a session. The answer to
"is it as capable as the strongest public systems?" is no, not yet: M5
persistence, cross-session scopes, semantic recall (optional plugin) and a
memory evaluation suite are the missing pieces.

## What to do next

1. Done: ADR 0012 fixes the memory layers and the kernel/port boundary. The
   pieces were spread across ADR 0006, 0008 and 0011; that is now explicit.
2. M5 persistence: sessions, summaries, facts and todos in SQLite; workspace
   and global facts survive runs; export and deletion.
3. A small LongMemEval-style evaluation: extraction, multi-session, temporal,
   knowledge update and abstention, run on the existing companion/fusion
   workloads. Do not chase LoCoMo leaderboard numbers.
4. Add knowledge-update and contradiction tests across sessions; facts already
   supersede and forget, but there is no cross-session conflict policy.
5. Keep lexical recall as the default. Add semantic recall as a plugin behind a
   flag only if the evaluation shows misses that IDF cannot cover.
6. Keep the no-self-edit rule unless a host explicitly opts into a memory tool;
   it is the main auditability advantage over agent-managed memory.

## Sources

- MemGPT / Letta: [arXiv 2310.08560](https://arxiv.org/abs/2310.08560),
  [Letta comparison](https://vectorize.io/articles/hindsight-vs-letta)
- Mem0: [arXiv 2504.19413](https://arxiv.org/abs/2504.19413),
  [Mem0 blog](https://mem0.ai/blog/mem0-the-token-efficient-memory-algorithm),
  [benchmark critique](https://llms3.com/blog/when-the-benchmarks-stopped-agreeing-july-2026)
- Zep / Graphiti: [arXiv 2501.13956](https://arxiv.org/abs/2501.13956),
  [comparison](https://vectorize.io/articles/hindsight-vs-zep)
- A-MEM: [arXiv 2502.12110](https://arxiv.org/abs/2502.12110),
  [NeurIPS 2025](https://neurips.cc/virtual/2025/loc/san-diego/poster/119020)
- MemoryOS: [arXiv 2506.06326](https://arxiv.org/abs/2506.06326),
  [EMNLP 2025](https://aclanthology.org/2025.emnlp-main.1318/)
- LongMemEval: [arXiv 2410.10813](https://arxiv.org/abs/2410.10813),
  [ICLR 2025](https://proceedings.iclr.cc/paper_files/paper/2025/hash/d813d324dbf0598bbdc9c8e79740ed01-Abstract-Conference.html)
- LoCoMo: [arXiv 2402.17753](https://arxiv.org/abs/2402.17753),
  [audit](https://github.com/dial481/locomo-audit),
  [benchmark critique](https://huggingface.co/blog/mjfk/evaluating-agent-memory-honestly)
- DeepSeek Harness memory plugins:
  [@zheexinn/dsh-memory](https://www.npmjs.com/package/@zheexinn/dsh-memory),
  [dsh-memory-amem](https://github.com/Zhang-Zhengyuan/dsh-memory-amem),
  [ALTM](https://github.com/cuiyuestar/Autonomous-Long-Term-Memory-System),
  [Mem0 MCP](https://github.com/MorseWayne/mcp-ai-memory)
- Surveys: [arXiv 2602.06052](https://arxiv.org/abs/2602.06052),
  [arXiv 2512.13564](https://arxiv.org/abs/2512.13564)
