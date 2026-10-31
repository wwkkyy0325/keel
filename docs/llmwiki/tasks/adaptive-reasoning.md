# Adaptive reasoning

Implements ADR 0005. Reasoning effort is kernel-owned, starts at `Low`, and
escalates on real failure signals.

## Steps

- [x] 1. ADR 0005 and reasoning-content passback: `Message` carries
      `reasoning_content`; the driver accumulates thinking deltas and the wire
      echoes them on assistant tool-call messages — verify: message and wire
      tests
- [ ] 2. `ReasoningLevel` controller: `Low -> High -> Max`, monotonic per turn,
      reset next turn, no user switch — verify: controller tests
- [ ] 3. Remove fixed `reasoning_effort` from profiles; map levels through the
      provider with clamping — verify: profile/provider tests
- [ ] 4. Wire real failure signals: required verifier failure, repeated tool
      failure and no-progress round — verify: driver tests
- [ ] 5. Real task-001 comparison: record levels, usage and latency — verify:
      eval report

## Decisions

- Context overflow and compaction are capacity events; they do not raise the
  reasoning level.
- At `Max`, a still-failing turn enters `Escalating` or ends in `Error`.
- DeepSeek thinking mode requires `reasoning_content` echo; this is a
  prerequisite, not an optional optimization.
