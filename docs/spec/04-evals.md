# 04 — Evaluation

Capability is measured at three layers. They are not one score.

## 1. Kernel correctness

Deterministic tests, no model:

- every FSM transition, terminal state, cancellation and timeout;
- tool schema validation, typed errors, concurrent reads, serialized writes;
- context budget, stage order, each compaction step;
- permission: traversal, denylist, admin, workspace boundary, audit;
- plugin register/unregister and denied capability escalation;
- dispatcher permission enforcement before the tool runs;
- interactions: request/resolve/defer/timeout/cancel, budget and no
  self-approval;
- permission grants: path scope, TTL expiry, revoke and deny-list precedence;
- soft delete: overwrite backup, trash failure abort, deny-list protection;
- plan mode hides write/execute tools without lowering permissions;
- context overflow after tool output ends the turn;
- assistant tool-call messages and tool-result call ids survive history;
- provider cancellation observes the shared token;
- fault injection: model 500, stream cut, tool timeout, disk full, overflow.

Gate: all pass in CI. Kernel overhead budgets:

| Metric | Budget |
|---|---|
| turn overhead, excluding model/tools | p50 < 5 ms, p99 < 20 ms |
| context assembly, 10k messages | < 20 ms |
| tool dispatch | < 1 ms |
| memory recall | < 50 ms |
| telemetry enqueue | < 1 ms |

Implemented as ignored release tests in
`crates/keel-runtime/tests/performance.rs`; the host measures event
persistence in its own ignored test. Results:
`evals/reports/2026-11-04-kernel-performance.md`.

## 2. Coding variant

Case format:

```
evals/cases/coding/task-001/
  task.md          # issue-style prompt, no solution
  setup/           # initial workspace
  hidden/          # tests applied after the run
  verify.sh        # accepts/rejects
  meta.toml        # timeout, profile, model, seeds
```

Sources:

- self-authored tasks (preferred);
- public repositories with MIT/Apache/BSD licenses, recorded in provenance.

Metrics:

- `pass@1` (all hidden tests pass);
- solved tasks per hour;
- tool-call format error rate;
- turns, tokens, wall clock;
- test files unchanged (hash check);
- acceptance checklist evidence: every required item has verifier evidence,
  and fabricated claims are flagged as unverified.

Start with 20 tasks. Do not scale until every failure has an explanation.

## 3. Companion variant

Case format:

```
evals/cases/companion/char-001/
  card.md          # original character, no copyrighted persona
  turns.jsonl      # 20-turn script
  rubric.md        # 1-5 scale
  meta.toml
```

Metrics:

- persona consistency (rubric);
- repetition rate (n-gram);
- out-of-character count;
- latency p50/p95;
- safety/moderation violations;
- memory recall correctness.

Human review of at least 20% of runs. LLM judges are a filter, not the final
score. Report the judge prompt and model version.

## 4. CI gates

- PR: kernel tests + 3 coding cases + 1 companion case.
- Nightly: 20 coding cases + 5 characters x 20 turns + cost report.
- Performance budgets fail the build.

## 5. Reporting

Every report includes:

- what was measured and why;
- profile/model/version/seed;
- pass rate and failures;
- cost and latency;
- known threats to validity;
- links to cases and raw trajectories.

Never report only successful cases.

## 6. Clean-room assets

- Self-authored cases: preferred.
- Public datasets/repos: license-checked and recorded.
- No employer tests, conversations, data or benchmarks.
- No copying another project's eval harness.
