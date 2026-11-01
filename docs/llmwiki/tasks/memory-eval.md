# Memory evaluation

Evaluate Keel's own memory, not a vendor memory stack. The five abilities follow
LongMemEval; the implementation stays lexical and structured (ADR 0012). An
external memory adapter can be measured separately once one exists.

## Current coverage

| Ability | Status | Evidence |
|---|---|---|
| Information extraction | Covered | Unit tests for fact scopes and provenance; two-process eval stored three constraints and answered all three. |
| Multi-session reasoning | Covered | Two processes on one store: facts, memo and segments restored; 3/3 questions answered. Report: `2026-11-01-memory-multi-session.md`. |
| Temporal reasoning | Pending | Facts carry `segment_id` but no event time; the event log has turn order only. |
| Knowledge updates | Covered | `an_update_replaces_the_fact_for_the_same_subject` plus the contradiction and forget tests; the rerun kept one current `项目代号` fact; the state before an update stays in `fact_history` and the memo. |
| Abstention | Covered | The two-process eval refused a car plate it was never given. Kernel unit test: `unrelated_input_recalls_nothing`. |

## Plan

- [x] Add a host-level scenario set: two processes, one store, companion 8k,
      no tools, acceptance so every turn seals. No new kernel code.
- [x] Extraction: state three facts in session 1, ask in session 2. Verify:
      all three answered.
- [x] Multi-session: the second process restored facts, memo and segments
      through the SQLite store. Verify: `history` and facts present; 3/3.
- [x] Temporal: ask "what was true before X". Verify: answered `蓝鲸` for
      "before"; asked when it changed, the model abstained ("没记下日期")
      instead of inventing a time. `fact_history.changed_at` is stored but not
      in the prompt.
- [x] Knowledge updates: state a fact, update it, ask which is current.
      Verify: answered 海鸥 after the update; both fact rows remain.
- [x] Abstention: ask about something never stated; require a refusal.
      Verify: "我不知道你的车牌号——你之前没跟我提过这个。"
- [ ] Run full, `--no-recall` and `--no-memo` where the ability allows it.
- [x] Write `evals/reports/2026-11-01-memory-multi-session.md` with results.
- [x] Fix the verified-Q&A fact noise found by the eval: a verified segment
      records a fact only when it produced artifacts. Verify: 16 facts became
      8 with the same 6/6 answers.
- [x] Subject-level supersede: an explicit update cue ("改成", "changed to",
      ...) replaces every fact whose key starts with the subject before the
      cue. Verify: `an_update_replaces_the_fact_for_the_same_subject` keeps one
      workspace fact and its statement carries the new value.
- [x] Keep reply instructions out of the fact table: sentences that ask for
      an answer now ("直接回答", "just answer") are not constraints even when
      they open with `不要`. Verify: `a_reply_instruction_is_not_a_constraint`
      for the Chinese eval sentence and its English form.

## Rerun 2026-11-01 (after supersede and reply filtering)

- Three processes on one store: 3 extraction turns, 6 question/update turns,
  2 temporal probes. 11 turns, 11 Done, 11 acceptance passes, no overflow.
- Answers 8/8: three stored facts, update acknowledged, 海鸥 after the update,
  car plate refused, previous code name 蓝鲸, change date refused.
- Facts: 3 current, all real; 0 reply instructions (the first run had four).
  History: the pre-update fact is `forgotten` because the host diffs keys, and
  both versions stay in storage.
- The "before" answer still works from the memo, so current facts and ordered
  history do not compete.
- Report section: `evals/reports/2026-11-01-memory-multi-session.md`.

## Run 2026-11-01

- Two processes, one store, companion 8k, no tools: 9 turns, 9 Done, 9
  acceptance passes.
- Answers 6/6: 蓝鲸, 周二晚八点, starlight, 已记住, 海鸥, and an explicit
  refusal for the car plate.
- 9 sealed segments, 8 memo revisions, 8 facts (4 real workspace constraints).
- The first run stored 16 facts; eight were `verified: <question>`. Fixed by
  requiring artifacts for a verified fact; the rerun kept all answers correct.

## Rules

- Do not import vector, graph or MCP memory to pass an evaluation; those are
  separate adapters and their scores are not Keel's.
- Do not tune the benchmark to the implementation; use the public five-ability
  definitions and a held-out scenario set.
- Report abstention and knowledge-update failures explicitly; they are the
  common weak spots in public memory systems.
