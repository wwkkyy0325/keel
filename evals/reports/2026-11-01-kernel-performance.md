# Kernel performance and event overhead

Date: 2026-11-01. Release build (`--release`), Windows 11, local machine, one
run per measurement. These are local gates, not a CI benchmark.

```bash
cargo test --release -p keel-runtime --test performance -- --ignored --nocapture
cargo test --release -p keel-cli --bin keel-cli saving_turns_with_events_stays_bounded -- --ignored --nocapture
```

The budgets come from `docs/spec/04-evals.md`.

## Kernel

| Metric | p50 | p99 | Budget | Result |
|---|---|---|---|---|
| Turn overhead, no tools, mock model | 0.015 ms | 0.060 ms | p50 < 5 ms, p99 < 20 ms | pass |
| Context assembly, 10k messages | 1.835 ms | 3.197 ms | < 20 ms | pass |
| Tool dispatch (echo) | 0.004 ms | 0.020 ms | < 1 ms | pass |
| Memory recall, 200 segments | 0.090 ms | 0.169 ms | < 50 ms | pass |
| Event enqueue | < 0.001 ms | < 0.001 ms | < 1 ms | pass |
| 500 consecutive turns | 13.8 ms total | | < 120 s | pass |

The 500-turn stress asserts every turn is `Done` and the last `ContextBuilt`
stays within its budget. The session grows across the run; the governor keeps
the prompt bounded.

## Event recording overhead

This was the user-visible question: does persisting events slow a turn down?

| Step | p50 | Note |
|---|---|---|
| Serialize 50 events | 0.003 ms | JSON encoding, once per turn |
| Save a turn with 50 events | 1.602 ms | SQLite transaction, WAL |
| Save a turn without events | 1.232 ms | same transaction; difference about 0.37 ms |
| p99 with / without | 2.000 / 2.616 ms | SQLite write jitter dominates |

- A typical turn emits 20–60 events. Serialization is microseconds; the extra
  inserts add well under a millisecond at p50, and the p99 difference is within
  SQLite's own variance.
- The kernel never emits per-token events. `TextDelta`/`ThinkingDelta` stay in
  the driver; the event stream is per boundary. `EventLog::append` takes a short
  mutex and was measured below a microsecond.
- The store writes events in the same transaction as the turn, so there is no
  second commit. A crash still leaves the previous turn intact.

## Limits

- Release-only; a debug build is not representative.
- One local machine, one run; p99 on a shared CI runner would be noisy, so
  these tests are `#[ignore]` and not a CI gate.
- The mock model and echo tool remove network and tool work; this measures
  kernel overhead, not end-to-end latency.
- The store numbers depend on the filesystem and SQLite WAL; they are host
  measurements, not a kernel budget.
