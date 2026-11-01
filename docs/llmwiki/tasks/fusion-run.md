# Fusion run

A 57-turn novel run that exercises tools, host-locked tasks, the rolling memo,
recall, mid-turn elision and an acceptance checklist in one session. The GPU
host is shared, so the run uses the remote DeepSeek endpoint.

## Setup

- Workload: `D:\workspace-new\wwkkyy0325\fusion-run\workspace`.
- Clean initial state (`Last chapter: 0`) taken from the 15-chapter ablation
  project, not the failed pilot state.
- 50 chapter prompts, four built-in probe turns (after chapters 10, 26, 42 and
  50) and three added probes about chapters 1, 6 and 8.
- 50 host tasks in `prompts/todos.txt`; the list is locked.
- Profile: `variants/coding/profile.deepseek.novel.toml`, 128k window, 16k
  working set, 8k output, tools `read_file`, `write_file`, `list_dir`,
  `code_search`, `todo`.
- Acceptance: external `fusion-tools/check_latest.py` checks that the state
  points at the highest chapter file and that it has at least 1,200 CJK
  characters. The checker lives outside the workspace so the model cannot edit
  it; `python` is on the command allowlist. Max reprompts: 2.

## Steps

- [x] Prepare a clean state, the prompts, the host task list and the external
      checker. Verify: the checker fails on `Last chapter: 0` and passes on a
      1,303-character dummy chapter.
- [x] Start the 57-turn run with `--acceptance`, `--todo-file` and `--session`.
- [x] Analyze completion, overflow, elision, memo revisions, recall
      scores/hits, task closure and acceptance reprompts. Verify: 49 Done /
      8 overflow, 28/50 chapters, 17 missing, 28/50 tasks, 50 recall events,
      16 summary failures.
- [x] Write the report and update the validation matrix and changelog.
      Report: `evals/reports/2026-11-03-fusion-run.md`.
- [x] Fix the host loop: `--stop-on-error` stops instead of advancing after an
      error; `--turn-input` writes the current input for the external checker.
      A same-input retry loop is still a follow-up.
- [x] Enforce sequential task updates on locked host lists; reject
      `completed`/`in_progress` on a non-current item. Verify: `OutOfOrder`
      tests, 10 todo tests.
- [x] Make acceptance check the requested chapter, all chapters `1..Last`, and
      gaps via `fusion-tools/check_turn.py`, which reads the turn input.
- [x] Add a recall gate for long task briefs; keep short-question recall.
      Verify: `a_long_task_brief_does_not_recall`.
- [x] Rebuild the CLI with the summary floor and the fixes. Verify: 242 tests,
      fmt and clippy clean.
- [x] Run a 15-turn slice with the 24k profile and per-turn acceptance.
      Verify: 15/15 Done, 0 overflow, 14/14 chapters, 14/14 tasks closed,
      15/15 acceptance passes. Recall still fired on 12 turns because `之前`
      matched `写之前`; fixed by narrowing the markers.
- [x] Run a 3-turn micro-slice after the marker fix. Verify: 3/3 Done, 0
      overflow, 2/2 chapters, 2/2 tasks closed, no recall on chapter turns,
      probe answered correctly.
- [x] Try the local 26B on the slice. Verify: 15-turn slice stopped at turn 1
      because the state was not written; a micro-slice passed chapter 1 but
      missed `ch-002.md` while marking task 2 completed. Acceptance caught both;
      `--stop-on-error` stopped. Resource: 13.6 GB VRAM, ~0.4 GB free RAM.
- [x] First full 50 attempt on DeepSeek: 10 turns, 9 Done, 1 model-stream
      timeout at turn 10; 10 chapters, no gaps, tasks 9/50, acceptance 9/9,
      0 overflow, 0 recall. Stopped by `--stop-on-error`.
- [x] Add `--retry-errors N` to the CLI: re-run the same input after a failed
      turn, bounded. Verify: 242 tests; no test covers the session loop yet.
- [x] Rerun the full 50 on DeepSeek with `--retry-errors 2`, the 24k profile,
      per-turn acceptance and narrowed markers. Verify: stopped at turn 11.
      Chapter 8 timed out twice; the third replay saw task #9 as current and
      wrote `ch-009.md` for a chapter-8 request; acceptance caught the mismatch.
      11 turns, 7 Done / 4 Error, chapters 1–9 present, tasks 9/50, 0 overflow,
      0 recall. The retry was not idempotent and the stopped process exited 0.
- [x] Check the acceptance checklist between retry attempts: a failed turn
      whose expected artifact already exists is recovered, not replayed.
      Verify: three CLI tests for the recovery predicate.
- [x] Add `--turn-timeout-ms` (host-owned; the policy default is 180 s) and make
      a session that stops on an unrecovered failure exit non-zero.
- [x] Run the full 50 again from a clean workspace with
      `--turn-timeout-ms 300000`, per-turn acceptance and two retries.
      Verify: **57/57 Done, 0 Error, 0 retries; 50/50 chapters, no gaps;
      50/50 tasks closed `completed`; 57/57 acceptance; 0 overflow; context peak
      15,853 / 23,739; 1 recall turn with 2 hits; 4.49M input / 338k output;
      78.9% cache hit; 43 min; about ¥2.37 off-peak.** The memo fallback fired
      on 22 of 56 revisions and held.
- [x] Add per-call cache telemetry and a cost model. Verify: `call:` lines,
      `cache-economics.md`, 246 tests.
- [x] Probe the 39% memo fallback rate (20 empty summaries, 2 short). The
      cause was reasoning at the output cap: a DeepSeek probe with
      `reasoning_effort: low` spent 548 tokens on reasoning, 40 on content and
      hit the 200-token limit; `reasoning_effort: none` returned 90 content
      tokens and no reasoning. `ReasoningLevel::None` now maps to `"none"` and
      the summarizer uses it. Verify: wire test, summarizer test, and a 3-turn
      real run with acceptance. The first 3-turn attempt had no summary calls
      at all because a segment only seals after a turn passes acceptance; the
      rerun produced two model-written revisions (2,470 and 3,225 chars), both
      `fallback=false`.
- [x] Evidence-gate host task completion: the CLI snapshots the task list
      before each turn and restores it when the turn fails and acceptance does
      not pass. Verify: `a_failed_turn_restores_the_task_list`.
- [x] Reserve tool headroom in history assembly. Verify: the 12-turn slice
      overflowed by 154 tokens; the 11-turn rerun had 0 overflow, peak
      9,784 / 23,772, 11/11 Done and 10/10 model-written summaries.
- [ ] Persist the session across processes (M5, ADR 0008). A resumed slice
      wrote chapters 8–17 but the task list started at #1 (`done=0`); the memo
      and recall also start empty. This blocks multi-session memory evaluation.

## Verify criteria

- 0 `ContextOverflow`.
- Memo revisions at least 2 and recall hits at least 2 with scores.
- Task list closes as `completed`.
- Acceptance passes or reprompts, never silently accepts a short chapter.
- The probe answers match the chapter content or honestly say they do not know.

## Logs

`D:\workspace-new\wwkkyy0325\fusion-run\runs\fusion-50.log`.
