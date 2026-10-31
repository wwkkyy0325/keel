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

The kernel mechanisms work individually and in smaller runs, but the 57-turn
fusion is **not coherent** at 16k with a blind host loop. The failures are
mostly at the seams: host orchestration, acceptance scope, task ordering and
budget. This is a useful negative result; it does not justify calling the
fusion complete, and it does not justify moving vector or graph memory into the
kernel.

## Limits

- One run, one model, one quantisation; DeepSeek remote, no local GPU.
- The CLI binary predated the summary floor; the memo results are not the final
  kernel behavior.
- The 50-turn prompt is the older in-progress protocol; the 15-chapter prompt
  uses the derived-current protocol.
- The acceptance checker is host code; the kernel's acceptance contract is
  sound, but the checker was too weak for this run.

## Artifacts

- Log: `D:\workspace-new\wwkkyy0325\fusion-run\runs\fusion-50.log`.
- Workspace: `D:\workspace-new\wwkkyy0325\fusion-run\workspace`.
- Acceptance: `fusion-run/acceptance.toml`, `fusion-tools/check_latest.py`.
