# Adaptive reasoning

Implements ADR 0005. Reasoning effort is kernel-owned, starts at `Low`, and
escalates on real failure signals.

## Steps

- [x] 1. ADR 0005 and reasoning-content passback: `Message` carries
      `reasoning_content`; the driver accumulates thinking deltas and the wire
      echoes them on assistant tool-call messages — verify: message and wire
      tests
- [x] 2. `ReasoningLevel` controller: `Low -> High -> Max`, monotonic per turn,
      reset next turn, no user switch — verify: controller tests
- [x] 3. Remove fixed `reasoning_effort` from profiles; the provider sends the
      kernel level only when `supports_reasoning` is true — verify: profile and
      provider tests
- [x] 4. Wire the required verifier failure into `Verify`; repeated tool failure
      and no-progress are also wired — verify: driver tests
- [x] 5. Real task-001 comparison: after refining failure signals, the turn
      stayed at Low, used 15361 tokens in 10702 ms, and passed 4/4 — verify:
      eval report

## Decisions

- Context overflow and compaction are capacity events; they do not raise the
  reasoning level.
- Only model-action failures raise the level: invalid tool arguments count;
  permission denied, not found, timeout, cancellation and I/O do not. More
  reasoning cannot change an environment decision.
- At `Max`, a still-failing turn enters `Escalating` or ends in `Error`.
- DeepSeek thinking mode requires `reasoning_content` echo; this is a
  prerequisite, not an optional optimization.
