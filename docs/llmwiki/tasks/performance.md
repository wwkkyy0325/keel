# Performance gates

Budgets are defined in `docs/spec/04-evals.md`. They are release-only local
gates; CI timing on shared runners is too noisy for hard assertions.

## Steps

- [x] Turn overhead, no tools, 200 turns after warm-up: p50 < 5 ms, p99 < 20 ms.
      Verify: `turn_overhead_within_budget` (release).
- [x] Context assembly with 10k messages: p50 < 20 ms. Verify:
      `context_assembly_with_ten_thousand_messages`.
- [x] Tool dispatch: p50 < 1 ms. Verify: `tool_dispatch_within_budget`.
- [x] Memory recall over 200 segments: p50 < 50 ms. Verify:
      `memory_recall_within_budget`.
- [x] Event enqueue: p50 < 1 ms. Verify: `event_enqueue_within_budget`.
- [x] Stress: 500 consecutive turns stay `Done` and within the context budget.
      Verify: `five_hundred_turns_stay_bounded`.
- [x] Event persistence overhead: serialization, save-with-events and
      save-without-events measured. Verify:
      `saving_turns_with_events_stays_bounded` (host, ignored).
- [ ] CI smoke benchmark: optional; would need a dedicated runner and
      relaxed thresholds.

## Results

See `evals/reports/2026-11-04-kernel-performance.md`.

- Turn p50 0.015 ms / p99 0.060 ms; context 1.835/3.197 ms; dispatch
  0.004/0.020 ms; recall 0.090/0.169 ms; enqueue under a microsecond.
- 500 turns in 13.8 ms.
- Saving a turn with 50 events adds about 0.37 ms at p50 over a turn without
  events; serialization is 0.003 ms. Recording does not meaningfully change
  turn cost.

## Run

```bash
cargo test --release -p keel-runtime --test performance -- --ignored --nocapture
cargo test --release -p keel-cli --bin keel-cli saving_turns_with_events_stays_bounded -- --ignored --nocapture
```
