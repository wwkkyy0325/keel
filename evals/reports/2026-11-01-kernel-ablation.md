# Kernel ablation — 2026-11-01

This run measures the kernel's marginal contribution, not prompt quality. The
prompt, model, workload and state file are fixed across five arms; one
mechanism is removed at a time.

## Setup

- Project: `D:\workspace-new\wwkkyy0325\ascent-ablation`.
- Profile: `variants/coding/profile.deepseek.novel.toml`.
- Model: `deepseek-flash`, 128k window, 16k working set, 8k output reserve.
- Fifteen chapters, 1,200-1,800 Chinese characters each.
- Ten to fifteen host tasks; `full`, `no-recall`, `no-memo` and `unbounded`
  seed fifteen, `no-todo` seeds none.
- Fixed chapter-only protocol: read `state/continuity.md`, write the chapter,
  update the task status when available, update the state, answer a
  100-character summary. The prompt does not read the bible and does not plan
  other chapters.
- Arms: `full`, `no-todo`, `no-recall`, `no-memo`, `unbounded`.
- One run per arm. This is an observation set, not a repeated experiment.

## Results

| Arm | Done/Err | Ch | CJK | Missing | Overflow | Elided | Recall | Memo | Tasks | Input | Ctx max |
|---|---|---|---|---|---|---|---|---|---|---|---|
| full | 15/0 | 15 | 23,542 | - | 0 | 1 (-1,361) | 2 / 1,599 | 1,179 | closed 15/15 | 776,150 | 10,510 |
| no-todo | 15/0 | 14 | 20,937 | [7] | 0 | 0 | 2 / 1,265 | 2,623 | none | 451,572 | 8,894 |
| no-recall | 15/0 | 15 | 21,316 | - | 0 | 1 (-1,400) | 0 | 2,053 | closed 15/15 | 725,015 | 10,593 |
| no-memo | 15/0 | 14 | 22,246 | [4] | 0 | 1 (-1,367) | 0 | none | done=14 | 787,557 | 10,156 |
| unbounded | 14/1 | 15 | 24,644 | - | 1 | 3 (-3,239) | 2 / 1,463 | 1,011 | closed 15/15 | 871,313 | 11,083 |

Validator: full 1 error / 6 warnings, no-todo 2/6, no-recall 3/5, no-memo 3/1,
unbounded 0/4. Most errors are chapter length below the 1,200-character floor,
not structural corruption.

### Follow-up: derived current task

The todo protocol then changed: the current task is the first pending item, and
the model marks only completion. A chapter needs one `todo` call instead of
two. The prompt, model and workload are otherwise unchanged.

| Arm | Ch | CJK | Overflow | Elided | Recall | Memo chars | Tasks | Input |
|---|---|---|---|---|---|---|---|---|
| full, explicit current | 15 | 23,542 | 0 | 1 | 2 / 1,599 | 1,179 | closed 15/15 | 776,150 |
| full, derived current | 15 | 23,465 | 0 | 2 | 2 / 1,530 | 126 | closed 15/15 | 722,580 |

Input fell 6.9%. The per-turn delta swings from -30k to +31k because the model
varies how much it reads and writes per chapter, so one run cannot separate the
removed round-trip from that variance. To measure the structural change
directly, the usage line now reports `calls=` per turn; the next arms should
read round-trips from it instead of inferring them from token totals.

The derived arm also produced a 126-character memo against a 756-token budget,
where the explicit arm produced 1,179 characters. The kernel marked the covered
segment absorbed either way. A structured fallback covers a failed summarizer
call, not a degenerate but valid answer, so a very short memo can silently
replace history. This is a quality signal to probe, not a confirmed defect.

## Findings

### 1. Bounding live tool results is load-bearing

The four bounded arms had zero `ContextOverflow`. The unbounded arm needed
three elisions and still overflowed once (14,968 against 14,943 tokens). It
also used the most input tokens. The mid-turn elision itself worked in every
arm that needed it, saving 1.3k-3.2k tokens. The mechanism is doing real work;
the unbounded arm shows what happens without it.

### 2. The task list buys tracking and closure, at a real token cost

`full` and `no-recall` finished 15/15 and closed the list as `completed`.
`no-todo` wrote 14/15, had no closure, and used **451k input tokens versus
776k** in `full`. The difference is consistent with the two `todo` round-trips
per chapter (mark in progress, mark completed) plus the pinned list. The todo
state prevents drift and gives a terminal state, but it is the largest overhead
item observed here.

This suggests a next design step: derive the current task from the first
pending item, or advance it automatically when the previous one completes, so a
chapter needs at most one todo call. That keeps the tracking benefit and
removes one model round per chapter. The derived-current follow-up above
implements half of that and still closes the list.

### 3. Memo and recall are not load-bearing for this workload

`no-memo` still wrote 14/15 and used about the same input as `full`;
`no-recall` wrote 15/15 and used slightly less input. The chapter protocol reads
`state/continuity.md` every turn, and the state carries the story facts, so the
memo and recall have little to contribute to chapter completion. The memo still
has a role in consistency and the recall still recovers details, but this
workload cannot measure that: the state file is the dominant memory.

A future probe should remove early details from the state and ask questions
that require an absorbed segment, as the 24-turn companion run did. Chapter
count alone cannot separate recall from a well-maintained state file.

### 4. Missing chapters are model variance, not a kernel result

`no-memo` missed chapter 4, `no-todo` missed chapter 7, and the other three
wrote all fifteen. One run per arm is not enough to attribute missing chapters
to a mechanism. The validator caught them, which is the intended behavior.

## Limits

- One run per arm; no confidence intervals.
- The state file is a stronger memory than the memo in this protocol, which
  hides the memo/recall effect.
- The validator's length threshold (1,200) produced most of the errors; the
  qualitative chapter content was not reviewed.
- The fixed prompt is a chapter-only protocol. The result does not claim that
  the same numbers hold for a richer or more exploratory protocol.

## Next

1. Done: `2026-11-01-recall-probe.md` adds the recall counterfactual. With
   recall the model named `config/marlin.toml`; without it the model denied the
   project existed.
2. Probe the short-memo case: a 126-character memo absorbed a segment; remove
   an early detail from the state and ask for it after the seal.
3. Repeat `full`, `no-todo`, `full-derived` and `unbounded` once with `calls=`
   recorded, to separate model variance from mechanism effects.
