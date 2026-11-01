# Multi-session memory eval

Date: 2026-11-01. Two processes, one SQLite store, the companion profile (8k
working set, no tools). The goal is the M3/M5 seam: does memory survive a
process restart, and does it support extraction, update and abstention.

## Setup

- Profile: `variants/companion/profile.deepseek.toml`, 8k window, 7,168 working
  set, no tools.
- Store: `--store memory.db --session-id mem`; the second process restores the
  segment list, memo and facts from the first.
- Acceptance: one compute item (`1+1=2`). It passes every turn, so every turn
  after the first seals a segment and triggers fact extraction and a memo
  revision. This is a host trick to exercise the memory path; it is not a
  quality check.
- Session 1 (3 turns): `请记住` three facts — project code name, deploy window,
  test database name.
- Session 2 (6 turns, new process): three questions, one update, a question
  after the update, and one question about a fact that was never given.

## Results

| Ability | Turn | Expected | Answer |
|---|---|---|---|
| Extraction + persistence | project code name | 蓝鲸 | 你的项目代号是蓝鲸。 |
| Extraction + persistence | deploy window | 周二晚八点 | 你的部署窗口是每周二晚上八点。 |
| Extraction + persistence | test database | starlight | 你的测试环境数据库叫 starlight。 |
| Update | change code name | 已记住 | 已记住。 |
| Updated recall | code name now | 海鸥 | 你的项目代号现在是海鸥。 |
| Abstention | car plate | unknown | 我不知道你的车牌号——你之前没跟我提过这个。 |

- 9 turns, 9 Done, 9 acceptance passes across the two processes.
- 9 sealed segments, 8 memo revisions, one summary per seal.
- Facts: 8 rows, of which 4 are the real workspace constraints.

## Finding: verified Q&A turns became facts

The first run stored 16 facts. Eight of them were `verified: <question>`,
because every verified segment emitted a "verified" fact with the segment topic
as the statement. In a Q&A session where each turn is verified, that records
the user's question as a fact about the world:

```
verified: 不要查文件，直接回答：我的项目代号是什么？
```

The model still answered correctly because the real constraints came first in
the memory block and the memo carried them, but it is noise that grows with
conversation length. Fixed: a verified segment records a fact only when it
produced artifacts. A pure answer is not a completed task. The rerun produced
8 facts instead of 16, with all six answers still correct.

## Remaining precision gap

Four session facts are still noise:

```
constraint: 不要查文件 直接回答 我的项目代号是什么
constraint: 不要查文件 直接回答 部署窗口是什么时候
```

`不要` is a constraint cue, so an instruction about the current reply is parsed
as a durable preference. The kernel cannot tell "do not read files now" from
"never read files". Options are a narrower cue set, a separate imperative form,
or keeping them session-scoped and letting the budget drop them; none is
implemented.

## Update semantics

The update turn stored a new workspace fact (`项目代号改成海鸥`) with a different
key from the original. Both facts remain in the table; the answer is correct
because the memo was rewritten and the newer fact is more recent in the block.
Same-key replacement is covered by the `a_contradiction_demotes_the_promoted_fact`
unit test, but subject-level supersede is not implemented.

## Temporal probe

Two questions on the same store after the update:

| Question | Answer |
|---|---|
| What was the code name before? | 你的项目代号以前是蓝鲸。 |
| When was it changed? | 我不记得具体是什么时候改的——我只记得你先告诉我代号是蓝鲸，后来改成了海鸥，但没记下改动的日期或时间。 |

Relative order works because the memo carries both statements in order. The
kernel has no event time in the context, and the model abstained instead of
inventing a date. `fact_history.changed_at` exists in the store, but it is not
projected into the prompt; wiring it in is future work.

## Limits

- One model, one prompt set; no repeated runs.
- Acceptance forces a seal every turn, which is denser than a normal
  conversation. Without acceptance, seals depend on topic change or the turn
  cap, and facts appear later.
- Temporal reasoning has one probe pair in the first run and one in the
  rerun; it is not repeated across models.
- The small 679-token memo budget rejected a short model summary on one turn
  and fell back to the structured memo; the answers were unaffected.

## Artifacts

- Logs: `D:\workspace-new\wwkkyy0325\fusion-run\memory-eval\session1.log`,
  `session2.log`.
- Store: `D:\workspace-new\wwkkyy0325\fusion-run\memory-eval\memory.db`.

## Rerun after subject supersede (2026-11-01)

Same profile and store shape; the kernel now filters reply instructions and
replaces a subject's value on an explicit update cue. Three processes: the
three extraction turns, the six-question session, then two temporal probes in
a third process.

| Ability | Turn | Expected | Answer |
|---|---|---|---|
| Extraction + persistence | project code name | 蓝鲸 | 你的项目代号是蓝鲸。 |
| Extraction + persistence | deploy window | 周二晚八点 | 周二晚八点。 |
| Extraction + persistence | test database | starlight | 叫 starlight。 |
| Update | change code name | acknowledged | 记住了，项目代号现在改成海鸥。 |
| Updated recall | code name now | 海鸥 | 海鸥。蓝鲸那个名字已经不用了。 |
| Abstention | car plate | unknown | 这个我这儿没有——你没跟我说过车牌号… |
| Temporal, before | previous code name | 蓝鲸 | 蓝鲸。后来改成了海鸥，现在用的是海鸥。 |
| Temporal, when | no date stored | abstain | 这个我这边没有记录…没说过改名的具体时间。 |

- 11 turns across three processes: 11 Done, 11 acceptance passes, 0 overflow.
- Answers 8/8. 15,900 input and 3,674 output tokens; per-call cache hit
  26–47%, lower than the long run because every process starts a new prefix.
- Facts after the run: 3 current, all real (`项目代号改成海鸥`, deploy window,
  test database). The earlier run had 8, of which 4 were reply instructions.
- `fact_history`: 5 rows. The pre-update fact is version 1 `added` and version
  2 `forgotten`; the host diffs keys, so a subject-level replacement is
  recorded by disappearance rather than as `superseded`. Both statements stay
  in storage, and the memo keeps their order for temporal questions.
- The "before" answer shows why the memo is not redundant with facts: the fact
  table is current by design, the memo carries the order of change.

Artifacts: `D:\workspace-new\wwkkyy0325\fusion-run\memory-eval\rerun-supersede\`.
