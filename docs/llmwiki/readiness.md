# Readiness assessment

## Status

M2 is a working vertical slice, not a production-ready agent. "Done" means the
milestone criteria pass, not that the system survives daily use.

## Evidence

| Proven by real runs | Proven only by deterministic tests | Not proven |
|---|---|---|
| DeepSeek stream, tool loop and `reasoning_content` echo | FSM transitions, cancellation, timeout | Multi-turn conversation |
| One coding task (`task-001`) solved and verified | Permission boundaries, grants and deny list | Long context and compaction |
| Kernel acceptance runs a real `python -c` check | Interaction defer/timeout/approve | Real 429, retry and backoff |
| Adaptive reasoning `Low -> High` on invalid tool arguments | Acceptance failure reprompt loop | Real failing-task recovery |
| Usage and turn duration | Tool concurrency and serialization | Different languages and repositories |
| 10 self-authored cases pass with real kernels verification (2026-11-02) | Fault injection for provider and filesystem failures | Long-running or concurrent use |
| | Soft delete and overwrite backup | Interactive terminal approval end-to-end |
| | Mock and local-server provider paths | Cost, concurrency and long-running use |

## Risk register

| Risk | Detection | Mitigation | Trigger |
|---|---|---|---|
| Provider wire drift | Recorded SSE fixtures and contract tests | Adapter capability flags, clear errors | Any provider change |
| Context growth or silent truncation | Token accounting and overflow events | Compaction, larger window, subagents | First real overflow |
| Tool loops and repeated calls | Repeated-call detection and event log | Turn limits, reasoning escalation, early exit | Two identical failed calls |
| False completion | Kernel verifier and external `verify.sh` | Required evidence, reprompt, `verification_failed` | Any missing evidence |
| Permission denial or bypass | Tool error codes and audit events | Dispatcher enforcement, hard deny list, OS sandbox later | First external workspace |
| Cost or latency blowup | Usage and duration reports | Per-turn budgets, model route ceiling | Budget exceeded |
| Interaction deadlock | Deferred interaction events | Timeout policy, non-TTY defer | First headless run |
| Silent data loss | Trash and write events | Overwrite backup, no hard delete | Any write failure |

## Hardening plan

1. Run the harness on every case and write a JSON report.
2. Add self-authored agentic cases covering multi-file edits, permissions,
   verification failure, context pressure and tool errors.
3. Add public permissive cases (HumanEval+/MBPP+) only as code-correctness
   smoke tests, not as the main readiness signal.
4. Inject provider, tool and filesystem failures.
5. Dogfood three to five real tasks per week and record each friction point.

Do not start M3 memory work until the first three steps produce a stable
failure taxonomy.
