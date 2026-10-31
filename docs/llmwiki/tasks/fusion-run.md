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
- [ ] Analyze completion, overflow, elision, memo revisions, recall
      scores/hits, task closure and acceptance reprompts.
- [ ] Write the report and update the validation matrix and changelog.

## Verify criteria

- 0 `ContextOverflow`.
- Memo revisions at least 2 and recall hits at least 2 with scores.
- Task list closes as `completed`.
- Acceptance passes or reprompts, never silently accepts a short chapter.
- The probe answers match the chapter content or honestly say they do not know.

## Logs

`D:\workspace-new\wwkkyy0325\fusion-run\runs\fusion-50.log`.
