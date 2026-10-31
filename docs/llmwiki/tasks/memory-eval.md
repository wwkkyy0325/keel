# Memory evaluation

Evaluate Keel's own memory, not a vendor memory stack. The five abilities follow
LongMemEval; the implementation stays lexical and structured (ADR 0012). An
external memory adapter can be measured separately once one exists.

## Current coverage

| Ability | Status | Evidence |
|---|---|---|
| Information extraction | Covered | `an_explicit_remember_promotes_immediately`, `a_durable_constraint_needs_two_segments`; fact scopes and provenance |
| Multi-session reasoning | Pending M5 | Sessions do not persist across CLI runs; `Workspace`/`Global` facts are designed but not stored |
| Temporal reasoning | Pending | Facts carry `segment_id` but no event time; the event log has turn order only |
| Knowledge updates | Covered inside a session | `a_contradiction_demotes_the_promoted_fact`, `forget_removes_a_promoted_fact`; memo rewrite replaces contradicted statements |
| Abstention | Partly covered | `unrelated_input_recalls_nothing`; the kernel injects no unrelated memory, but answer-level abstention is the model's job and needs a probe |

## Plan

- [ ] Add a host-level scenario set with five groups, one per ability. Use the
      companion 24-turn script and the fusion workspace; no new kernel code.
- [ ] For extraction, state a fact in one turn and ask for it in a later one.
- [ ] For multi-session, wait for the M5 SQLite adapter; until then, record the
      gap instead of faking a pass.
- [ ] For temporal, ask "what was true before X" and record that Keel has no
      event-time model; do not claim temporal memory.
- [ ] For knowledge updates, state a fact, contradict it, ask which is current;
      repeat with `forget`.
- [ ] For abstention, ask about something never stated; require "I don't know"
      or a refusal to invent. Run with and without a recall block.
- [ ] Run full, `--no-recall` and `--no-memo` where the ability allows it.
- [ ] Write `evals/reports/<date>-memory-eval.md` with per-ability results,
      token cost and the model used.

## Rules

- Do not import vector, graph or MCP memory to pass an evaluation; those are
  separate adapters and their scores are not Keel's.
- Do not tune the benchmark to the implementation; use the public five-ability
  definitions and a held-out scenario set.
- Report abstention and knowledge-update failures explicitly; they are the
  common weak spots in public memory systems.
