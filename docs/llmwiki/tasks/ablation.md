# Kernel ablation

The goal is to measure the kernel's marginal contribution, not prompt quality.
The prompt, model and workload stay fixed across arms; only one mechanism is
removed at a time.

## Switches

- [x] `KernelOptions { recall, memo, bounded_tools }` in the driver, defaulting
      to all on. These are host evaluation controls, not variant policy.
- [x] CLI flags `--no-recall`, `--no-memo`, `--unbounded-tools`, `--no-todo`.
      `--no-todo` also removes the tool from the visible set and skips seeding.
- [x] Unit/integration tests still pass with the default options — verify:
      230 tests, fmt and clippy clean.

## Workload

- Fixed project `D:\workspace-new\wwkkyy0325\ascent-ablation`.
- Ten chapters, 1,200-1,800 Chinese characters each.
- The prompt is chapter-only: read `state/continuity.md`, write the chapter,
  update the task status when the tool is available, update the state, answer
  a 100-character summary. It does not read the bible and does not plan other
  chapters.
- Ten host tasks, so a completed arm also exercises the terminal list state.

## Findings so far

- The ten-chapter full arm wrote 10/10 with no overflow and the task list
  closed. It did not trigger memo or recall: ten turns never reached the
  twelve-turn segment cap, so the whole arm was one open segment.
- The fifteen-chapter full arm triggered one memo revision and one recall
  candidate, but the fixed 400-token cap dropped the hit (`recall=0(0/1)`).
  It also hit four `ContextOverflow`s from tool results added after the
  context was built. Both are kernel allocation problems, now fixed:
  recall takes a quarter of the remaining allowance, and old tool results are
  elided before overflow.
- The 15-chapter arm wrote 14/15 chapters; chapter 5 was skipped and the task
  list stopped at `done=14`. That part is protocol/model behavior, not the
  kernel.

## Arms

- [x] `full`: memo + recall + todo + bounded tools — 15/15, closed, 0 overflow
- [x] `no-todo`: no task list or tool — 14/15, no closure, 451k input
- [x] `no-recall`: no lexical recall — 15/15, closed, 725k input
- [x] `no-memo`: no rolling summary; sealed records render — 14/15, 788k input
- [x] `unbounded`: live tool results are not clipped — 15 chapters, 1 overflow,
      three elisions, 871k input

## Results

See `evals/reports/2026-11-01-kernel-ablation.md`. The short version:

- bounding live tool results is load-bearing: bounded arms had zero overflow,
  unbounded still overflowed after three elisions;
- the task list buys tracking and a terminal state at a large token cost
  (two todo round-trips per chapter);
- memo and recall are not load-bearing in this workload because the state file
  carries the story; they need probes or a state-less workload to measure;
- missing chapters in `no-memo` and `no-todo` are single-run model variance.

## Derived current task

- [x] `TodoList::current` returns the explicit in-progress item or the first
      pending item; `[tasks]` renders it; the model marks only completion.
      Verify: unit tests and 233 workspace tests.
- [x] `full-15-derived`: 15/15, 0 overflow, 2 elisions, tasks closed,
      722,580 input tokens versus 776,150 for the explicit arm (-6.9%).
      Per-turn variance is larger than the delta, so this run cannot attribute
      the saving to the removed round-trip alone.
- [x] `TurnOutcome::model_calls` and `usage: calls=N` report model round-trips
      directly. Verify: `text_only_turn_reaches_done` asserts one call.
- [ ] Probe the short memo: the derived arm wrote a 126-character memo and the
      kernel still absorbed the covered segment.
- [x] Recall probe with a disabled arm: 24 scripted companion turns with and
      without recall. Both 24/24 `Done`, 21 memo revisions, 0 overflow; the
      full arm answered `config/marlin.toml` from segment 0, the no-recall arm
      denied the project. A "pasta recipe" probe missed because the segment
      lacks `recipe`. Report: `evals/reports/2026-11-01-recall-probe.md`.
- [ ] Memo probe with a short absorbed segment and early details removed from
      the state, to test the 126-character memo case.

## Metrics

- chapters written and CJK characters
- validator errors and warnings
- `ContextOverflow` count
- input/output tokens and model calls
- task progress / drift / closed state
- recall turns and tokens
- memo revisions and sizes

The conclusion is the difference between arms under the same protocol; a
successful `full` arm alone is not evidence.
