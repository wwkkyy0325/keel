# Fusion run — 57-turn novel with tools, tasks, memo, recall and acceptance

Date: 2026-11-03. This is the first run that puts the main mechanisms in one
session: file tools, a host-locked 50-task plan, the rolling memo, lexical
recall, mid-turn elision and an acceptance checklist. It used the remote
DeepSeek endpoint; the shared GPU was not used.

## Setup

- Workspace: `D:\workspace-new\wwkkyy0325\fusion-run\workspace`.
- Clean initial state (`Last chapter: 0`) from the 15-chapter ablation project,
  not the failed pilot state.
- 50 chapter prompts, four built-in probe turns (after chapters 10, 26, 42 and
  50) and three added probes about chapters 1, 6 and 8.
- 50 host tasks, locked.
- Profile: `variants/coding/profile.deepseek.novel.toml`, 128k window, 16k
  working set, 8k output, tools `read_file`, `write_file`, `list_dir`,
  `code_search`, `todo`.
- Acceptance: external `fusion-tools/check_latest.py`, run from outside the
  workspace, checks that the state points at the highest chapter file and that
  the chapter has at least 1,200 CJK characters. Max reprompts: 2.
- CLI binary built at 15:17; it includes the IDF recall change but predates the
  summary quality floor (committed 15:23). This matters below.

## Results

| Metric | Value |
|---|---|
| Turns | 57 |
| Done / Error | 49 / 8 |
| Error type | 8 `ContextOverflow`; no other failures |
| Chapters written | 28 / 50 |
| Missing chapter numbers | 17 |
| Done chapter turns without the requested file | 16 |
| State Last chapter | 45 |
| Tasks done / current | 28 / #10 stuck from turn 10, #42 after turn 45 |
| Task list closed | No |
| Memo revisions | 48 |
| Summary failures | 16 |
| Recall turns | 50 of 57 |
| Overflows / elisions | 8 / 17 |
| Input / output tokens | 1,809,696 / 164,653 |
| Acceptance checks | 49 passed; never failed |

All written chapters had a title and at least 1,437 CJK characters, so the
length check itself did not fail. The missing chapters and silent skips were
not caught.

## What worked

- No hang, panic or untyped failure; overflow is a terminal `Error`.
- Elision ran 17 times and kept several turns alive, but could not always
  remove enough of the current tool round.
- Every written chapter passed the length/title floor.
- The final probe (chapter 8) recalled segments `[46, 7]`, answered
  “铁盒”, and chapter 8 line 113 contains “铁盒”. Targeted recall works.
- Usage, events and task progress were recorded per turn; the log is replayable.

## Failure 1: the host advanced after errors and silent skips

The session loop fed the next prompt after an `Error`, and after a `Done` turn
that did not write the requested chapter. There was no per-turn expected
artifact:

- turn 4 `Done`, no `ch-004.md`, no `changes` line;
- turns 17–20 `Done`, no `ch-016..019.md`;
- turns 33–35, 39–42, 49–53 similar;
- turn 10 `Error`, but `changes` points at `ch-009.md` (the previous chapter).

16 chapter turns returned `Done` without a file for the requested chapter. The
acceptance checker passed because it looked at the latest chapter and state,
which had not moved. This is the main cause of the 17 missing chapter numbers
and the state that claims chapter 45 while only 28 files exist.

## Failure 2: the locked task plan did not stay sequential

The list was locked against add/remove, but status updates were not ordered.
`current` stayed at `#10` from turn 10 to turn 44 while `done` rose from 9 to
28: the model completed later tasks while `#10` remained pending. At turn 45 it
marked `#42` `in_progress`, which overrode the derived current. The plan never
closed.

The old 50-turn prompt still instructs the model to mark a chapter
`in_progress`; the 15-chapter prompt uses the derived-current protocol. The
kernel allowed an explicit `in_progress` on a non-current host item.

## Failure 3: 16k working set is too small for this tool pattern

The 50-turn prompt reads `state/continuity.md`, `bible/characters.md` and
`bible/timeline.md` every turn, then writes a 1,800–2,200-character chapter.
Turn-start context was 10.7k–15.1k against a 15.15k budget; the current tool
round pushed several turns over. Eight turns overflowed at 15.2k–18.9k, even
with elision. The 15-chapter prompt read only the state file and had zero
overflows.

## Failure 4: recall over-fires on long chapter briefs

Recall ran on 50 of 57 turns. Scores were 0.90–0.94 with 143/180 matched query
terms: a long chapter brief shares most of its bigrams with old segments, so
IDF weighting does not make it selective. Each block cost about 941 tokens and
added to the context pressure. The probe turns are different: they are short
questions and recall is useful.

## Failure 5: memo quality is poor under this load

48 revisions, 16 summary failures, and several very short summaries (42, 64,
144, 179 characters) were accepted because the binary predated the summary
floor. The 64-character case is exactly what the floor is for; with the floor,
the driver would keep the previous memo and use the structured fallback. The
fallback is safer but adds records to an already tight context.

## Root cause summary

1. **Host orchestration**: no per-turn expected artifact and no retry; the loop
   advances on `Error` and on `Done` without the requested file.
2. **Acceptance scope**: latest-chapter/state check, not the requested chapter,
   all chapters, or task alignment.
3. **Task ordering**: host-locked list permits out-of-order completion and
   non-current `in_progress`.
4. **Budget**: 16k working set does not fit three bible reads + a chapter write
   + 8k output.
5. **Recall precision**: no gate for long task briefs.
6. **Memo quality**: short summaries and the stale binary.

## Fixes before a rerun

1. Host runner: verify the requested chapter exists before advancing; on
   `Error` or a missing file, retry the same input (bounded), then stop. A
   long-run runner is a stateful loop, not a blind line feed.
2. Task state: for locked host plans, reject `completed`/`in_progress` on a
   non-current item; only the current item may advance. Update the 50-turn
   prompt to the derived-current protocol (mark completion only).
3. Acceptance: check the requested chapter, all chapters `1..Last`, and gaps;
   align the state, the task list and the files.
4. Profile/workload: use a 24k working set for the 50-turn protocol
   (24k + 8k < 128k), or read only the state file. Measure both.
5. Recall gate: skip recall for long task briefs; require a question or recall
   marker, or a small query-term count. Keep the short probe behavior.
6. Rebuild the CLI so the summary floor is active; rerun a 15–20 turn slice
   before the full 50.

## Fix validation

### 15-turn slice (before the recall-marker narrowing)

- 15/15 `Done`, 0 overflow, 14/14 chapters, 14/14 tasks closed, 15/15
  acceptance passes.
- Context peak 13,305 against 23,846; the 24k profile removed the overflow that
  the 16k run hit.
- Sequential task updates worked: done advanced 1→14 and the list closed; no
  out-of-order completion.
- The per-turn checker checked the requested chapter, all earlier chapters and
  the state; it passed every turn.
- Recall still fired on 12 of 15 turns at score 0.90 with 140–146/171–180
  hits. The cause was the marker `之前` matching the instruction `写之前` in
  every chapter prompt. That is a marker-specificity bug, not a budget issue.
- Three summary calls fell back (`too little text`); the floor and the
  structured fallback worked.

### 3-turn micro-slice (after narrowing the marker)

- Two chapter turns and one probe: 3/3 `Done`, 0 overflow, 2/2 chapters, 2/2
  tasks closed, 3/3 acceptance passes.
- No recall on the chapter prompts; the probe answered the chapter-1 detail
  correctly from the memo/current context.
- One summary fallback; no failure path broke the turn.

The fixes are validated on the slices. The full 50-turn rerun is still
pending; the slices are not a substitute for it.

## Full 50 attempt: stopped by a provider timeout

The first full run used the 24k profile, per-turn checker and
`--stop-on-error`.

- 10 turns: 9 `Done`, 1 `Error`.
- The error was `timeout during model stream after 180000 ms` on turn 10, not
  an overflow and not an acceptance failure. The model had already written
  `ch-010.md` and updated the state; the turn could not produce the final text
  before the turn deadline.
- Chapters 1–10 exist with no gaps; state Last chapter 10.
- Tasks: 9 done, current #10, list open. Chapter 10 was written but the task
  was not marked completed before the timeout.
- Acceptance passed on all 9 completed turns; 0 overflow; 0 elisions.
- Recall: 0 turns. The narrowed gate kept the chapter briefs from pulling old
  segments.
- Memo: 9 revisions, 4 summary failures (fallback).

The timeout is a provider/network event, not a kernel mechanism failure. It did
show one host gap: a single transient stream timeout ends a long run. The CLI
now has `--retry-errors N`, which re-runs the same input after a failed turn;
the state and files written before the timeout are already on disk. The full
50 is being rerun with two retries.

## Full 50 retry (attempt 2): the retry was not idempotent

A clean workspace, the same 24k profile, per-turn acceptance and
`--retry-errors 2`. The run stopped at turn 11.

| Metric | Value |
|---|---|
| Turns | 11 |
| Done / Error | 7 / 4 |
| Errors | 3 model-stream timeouts, 1 acceptance failure |
| Retries | 3 (`1/2`, `1/2`, `2/2`) |
| Chapters written | 1–9, no gaps |
| State Last chapter | 9 |
| Tasks | done 9, current #10, list open |
| Acceptance | passed on 7 turns; failed on turn 11 |
| Recall | 0 turns |
| Overflows / elisions | 0 / 0 |
| Memo revisions / failures | 7 / 2 (one timeout, one empty summary) |
| Input / output tokens | 748,864 / 60,219 |
| Exit code | 0, although the host stopped on a failed turn |

The sequence matters:

- Turn 9 (chapter 8) timed out before writing anything.
- Turn 10 re-ran the same input, wrote `ch-008.md` and marked task 8
  completed, then timed out again before the final answer.
- Turn 11 re-ran the same input a third time. The task list now said the
  current item was #9, so the model wrote `ch-009.md` for a request that asked
  for chapter 8. It said so in the answer. Acceptance then failed with
  `state says chapter 9, but the request is chapter 8`, and `--stop-on-error`
  stopped the run.

The retry replayed an input the world had already moved past. Two host defects
followed:

1. **No artifact check between attempts.** Turn 10 had already done the work;
   the host retried anyway. The checklist, not the turn state, decides whether
   the work exists.
2. **The stop was invisible to automation.** `run_session` returned `Ok(())`
   after stopping, so the process exited 0.

The timeout also recurred at the 180 s turn deadline during peak hours, with
turns taking 135–151 s before the deadline. The deadline is host-owned; the CLI
now exposes `--turn-timeout-ms`.

Fixes: after a failed turn, the host verifies the acceptance checklist; if it
passes, the turn counts as recovered and the session advances to the next
input. `--turn-timeout-ms` overrides the 180 s default. A session that stops
on an unrecovered failure exits non-zero. Three CLI tests cover the recovery
predicate.

The third run uses a clean workspace, `--turn-timeout-ms 300000` and the same
two retries.

## Full 50 third run: completed

A clean workspace, the 24k profile, per-turn acceptance, `--retry-errors 2`,
`--turn-timeout-ms 300000` and per-call cache telemetry. The session finished
all 57 turns with no error and no retry.

| Metric | Value |
|---|---|
| Turns | 57 (50 chapters, 7 probes) |
| Done / Error | 57 / 0 |
| Retries | 0 |
| Acceptance | 57 / 57 passed |
| Chapters | 50 / 50, no gaps |
| Chapter length | min 1,349, mean 2,077, max 2,873 CJK |
| State Last chapter | 50 |
| Tasks | 50 / 50, list closed `completed` |
| Context peak / budget | 15,853 / 23,739 |
| Overflows | 0 |
| Recall turns | 1 (probe 57), 2 hits, 907 tokens |
| Memo revisions | 56 |
| Memo fallbacks | 22 (20 empty, 2 short), 39% |
| Model calls | 314 (5.5 per turn) |
| Input / output tokens | 4,485,612 / 337,820 |
| Cache hit | 3,539,840 cached, 78.9% weighted; per-turn median 78.0% |
| Duration | 43 min, 44.7 s per turn average, 74 s max |
| Cost at off-peak prices | about ¥2.37 |

What this establishes:

- The mechanisms compose. Tools, a locked 50-item task plan, the rolling memo,
  recall, the budget governor and per-turn acceptance ran together for 57 turns
  without an overflow, an error or a silent skip.
- Acceptance is the right seam. Every turn's checker verified the requested
  chapter, all earlier chapters and the state; it passed 57 times. In the 16k
  run the same workload produced 16 silent skips.
- The task list closed exactly once, in order, at item 50.
- Recall is precise rather than noisy. It fired on one of 57 turns, the
  memory-only probe about chapter 8, and answered from segments with 2 hits.
  Probes 55 and 56 were answered from the memo without recall. The narrowed
  gate kept the 50 chapter briefs from pulling old segments.
- The memo fallback is load-bearing. The summariser returned no usable text on
  22 of 56 revisions; the structured fallback kept the memory pipeline alive,
  and the probes still passed.

Remaining quality issue: a 39% summary fallback rate is high. The fallback is
deterministic and safe, but it means the model-written memo is absent on those
turns. The summariser prompt or the empty-text handling deserves a probe.

### Follow-up: the memo fallback cause

The cause was reasoning at the output cap. A direct DeepSeek probe with the
summariser's settings:

| `reasoning_effort` | Completion tokens | Reasoning | Content | Finish |
|---|---|---|---|---|
| `low` | 200 (cap) | 548 chars | 40 chars | length |
| `none` | 69 | 0 | 123 chars | stop |

At `low` the model spends the whole output budget on reasoning and returns
almost no summary text, so the kernel falls back. `ReasoningLevel::None` now
maps to `reasoning_effort: "none"` and the summariser uses it.

A 3-turn live run with acceptance produced two memo revisions (2,470 and 3,225
characters), both `fallback=false`, with acceptance 3/3. A first 3-turn attempt
without acceptance produced no summaries at all: ADR 0006 seals a segment after
a turn passes acceptance, so the memo cadence depends on the host's checklist.

## Local 26B attempt

The local Gemma 4 26B was run at 32k server context and a 20k working set with
8k output, using the same per-turn checker and `--stop-on-error`.

- 15-turn slice: turn 1 ended `Error`. The model wrote `ch-001.md` (1,667 CJK)
  but not `state/continuity.md`; it pasted the state into the answer. The
  checker failed with `state says chapter 0, but the request is chapter 1`,
  reprompted twice (7 model calls, 43s), and the host stopped.
- Micro-slice with an explicit “write state with write_file, do not paste it”
  instruction: turn 1 passed (chapter and state written); turn 2 ended `Error`
  with `missing chapters: [2]; requested chapter 2 is missing`. The model
  marked task 2 completed and put the chapter summary in the answer, but did
  not write the file. The checker caught it, reprompted, and the host stopped.
- Resources while loaded: 13.6 GB / 16 GB VRAM and about 0.4 GB free physical
  memory. The server was stopped after each attempt; the GPU returned to
  ~837 MiB and about 12 GB free RAM.

What this shows:

- The acceptance and stop-on-error fixes do their job: a missing state or a
  missing chapter is now a bounded failure, not a silent advance.
- The 26B can write one chapter when the prompt is explicit about `write_file`,
  but it does not reliably use tools for the next chapter. The earlier 24-turn
  no-tool companion passed; the multi-chapter tool workflow is the weak axis.
- Task completion is not evidence-gated: the list closed as `completed` while
  `ch-002.md` was missing. Acceptance remains the authority; a host that trusts
  the task list alone would be misled. A future fix is to revert the task on a
  failed acceptance or require evidence for host task completion.
- The 26B is not a good fit for a shared machine: 13.6 GB VRAM and ~0.4 GB free
  RAM is too tight. DeepSeek remains the model for the 50-turn fusion; the 26B
  is useful for the no-tool memory probes when the machine is idle.

## Verdict

The 16k, blind-host-loop attempt was **not coherent**: its failures were at the
seams (host orchestration, acceptance scope, task ordering, budget). After those
seams were fixed, the 24k run completed the full 57 turns: 50/50 chapters, 50/50
tasks closed in order, 57/57 acceptance passes, zero overflow, zero errors and
zero retries. The fusion is coherent on this workload at 24k.

The result does not justify moving vector or graph memory into the kernel. The
memory that carried the run was the kernel's own: memo, records, the state file
and one targeted recall. It also sets the floor: 16k was too small for three
bible reads plus a chapter write, and the memo fallback rate (39%) is the next
quality target.

## Limits

- One successful run, one model, one quantisation; DeepSeek remote, no local
  GPU. It is not repeated, so model variance is not separated.
- The 57-turn 16k run used a binary that predated the summary floor; its memo
  numbers are not current behavior. The 24k run includes every fix.
- The 50-turn prompt still uses the older in-progress protocol; the
  15-chapter prompt uses the derived-current protocol.
- The acceptance checker is host code. It was weak for the 16k run and strict
  for the 24k run (requested chapter, all chapters, state).

## Artifacts

- Completed run: `D:\workspace-new\wwkkyy0325\fusion-run\final-50-retry\final-50-retry4.log`.
- Workspace: `D:\workspace-new\wwkkyy0325\fusion-run\final-50-retry\workspace`.
- Acceptance: `fusion-run/acceptance-final-50-retry.toml`,
  `fusion-tools/check_turn.py`.
- Earlier failed runs: `fusion-run/runs/fusion-50.log`,
  `fusion-run/final-50/final-50.log`, `final-50-retry/final-50-retry.log`,
  `final-50-retry/final-50-retry2.log`.
