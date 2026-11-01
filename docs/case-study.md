# Case study

Question: can a small kernel with a bounded context sustain long tasks,
different variants and third-party tools, and what does it cost?

All numbers below come from local runs against `deepseek-flash`. Reports are in
`evals/reports/`; the raw per-case JSON sits beside them (generated runs are
gitignored except suite summaries).

## Evidence

| Run | Result | Report |
|---|---|---|
| 50-chapter novel, 57 turns, 24k window | 57/57 Done, 50/50 chapters, 50/50 tasks, 57/57 acceptance, 0 overflow, 78.9% cache hit, 43 min, about ¥2.37 | `2026-11-01-fusion-run.md` |
| 20 curated coding cases | 20/20 passed, 286,042 tokens, 85.6% cache hit, about ¥0.42, slowest 37 s | `2026-11-01-coding-suite.md` |
| Public cache: HumanEval+ / MBPP+ | 20/20 and 13/16; two boundary failures, one transient | `2026-11-01-coding-suite.md` |
| Five companion personas, 20 turns each | 100/100 Done, all surface rules 20/20, 15/15 memory probes | `2026-11-01-companion-personas.md` |
| Multi-session memory | 8/8 answers after a restart; abstention and temporal bounds | `2026-11-01-memory-multi-session.md` |
| MCP plugins | filesystem, memory, a Python SQLite server and a mock HTTP server; roots round-trip | `2026-11-01-mcp-plugins.md`, `2026-11-01-branching-and-plugin-check.md` |
| Terminal first run, failing-test workspace | 1/1 Done, 3/3 tests after the edit, 5 model calls, 5.4 s, 81.6% cache hit | `docs/llmwiki/tasks/tui.md` |

## What the numbers show

- A bounded prompt does not force amnesia. The novel kept state across 50
  chapters through records, a rolling memo, facts and recall; the memory run
  answered stored facts after a process restart; the companion runs recalled
  three facts 13 turns later.
- Acceptance is the load-bearing gate. The fusion run owes its 50/50 chapter
  count to per-turn acceptance and a checklist re-check between retries; before
  that, a stale retry wrote chapter 9 for a chapter-8 request and the run
  continued with gaps.
- Variants are data. The companion profiles differ only in prompt and profile;
  the MCP adapter registers tools without a kernel change.
- The reference host is not a skin. One real turn fixed the demo bug (mean
  divided by `n - 1`), ran the tests before and after, and finished in 5.4 s
  with 81.6% cache hit; the same workspace is the README's first-run
  walkthrough.
- Cache behaviour is measurable at the call level. The 24k run hit 78.9% and
  the curated coding set 85.6%. The cost model in
  `docs/llmwiki/cache-economics.md` shows the ordering penalty is small and
  that one uncached 1M-token prefix costs more than an entire bounded run.

## Failure analysis

- **16k was too small** for three bible reads plus a chapter write. The first
  fusion attempt failed at 16k with 8 overflows and silent skips. The fixes
  were a 24k working set, sequential tasks, acceptance between retries and a
  host deadline.
- **Empty summaries** came from `reasoning_effort: low` spending the output
  budget on reasoning. The summarizer now requests `none`; an 11-turn rerun
  produced 10/10 model-written summaries.
- **Verified Q&A became facts.** Every verified turn stored its question as a
  "verified" fact. A verified segment now records a fact only with artifacts;
  the fact count halved with no answer loss.
- **Tool headroom**: history reserved room for input but not for tool results,
  causing a 154-token overflow. A tool-headroom reservation removed it.
- **Benchmark boundaries**: MBPP 16 expects a single lowercase word to be
  valid, and MBPP 20 treats `True` as `1`. Both are spec readings, not loop
  failures, and are recorded as such.
- **Turn closure**: one `task-008` run wrote the correct files but never
  produced a final text round, so the turn ended Error while `verify.sh`
  passed. The same case passed in other runs; this is model variance.

## Reproduction

```bash
cargo build -p keel-cli
python evals/harness/run_suite.py --filter coding
python evals/harness/run_companions.py
python evals/harness/run_case.py evals/cases/coding/task-001
```

The 50-turn fusion used the profile in `variants/coding/profile.deepseek.novel-24k.toml`
and the protocol in `docs/llmwiki/tasks/fusion-run.md`; model runs need
`DEEPSEEK_API_KEY` in `.env`.

## Limits

- One model, few repeats; model variance is not separated.
- Memory recall is lexical by design; no vector or graph memory is used or
  claimed.
- The event log is in memory; there is no stored audit timeline yet.
- The companion rules are surface checks, not a human or model judge.
- Costs use peak `deepseek-flash` prices and are estimates, not invoices.
