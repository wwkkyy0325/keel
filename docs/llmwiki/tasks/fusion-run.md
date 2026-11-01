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
- [ ] Run the full 50 again from a clean workspace with
      `--turn-timeout-ms 300000`, per-turn acceptance and two retries.
- [ ] Consider evidence-gating host task completion: a task marked `completed`
      without the expected artifact, or a turn that fails acceptance, should
      not leave the list closed as `completed`. Host-side revert or kernel
      evidence rule; not implemented. The 26B micro-slice showed the list can
      close while a chapter file is missing.

## Verify criteria

- 0 `ContextOverflow`.
- Memo revisions at least 2 and recall hits at least 2 with scores.
- Task list closes as `completed`.
- Acceptance passes or reprompts, never silently accepts a short chapter.
- The probe answers match the chapter content or honestly say they do not know.

## Logs

`D:\workspace-new\wwkkyy0325\fusion-run\runs\fusion-50.log`.
