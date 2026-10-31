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

## Arms

- [ ] `full`: memo + recall + todo + bounded tools
- [ ] `no-todo`: no task list or tool
- [ ] `no-recall`: no lexical recall
- [ ] `no-memo`: no rolling summary; sealed records render
- [ ] `unbounded`: live tool results are not clipped

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
