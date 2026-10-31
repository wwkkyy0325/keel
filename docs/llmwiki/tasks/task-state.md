# Task state — ADR 0009

The novel pilot showed that a model-authored state file becomes a task plan and
then overrides the prompt. Task state gets a kernel-owned home and a narrow
editing surface.

## Steps

- [x] Typed `TodoList` / `TodoItem` in `keel-core`: host and model source,
      locked lists, one `in_progress`, 100 items, 200 characters — verify:
      host items get ids, a second in-progress is rejected, a locked list
      rejects add/remove but accepts status updates
- [x] `todo` tool with `list`, `add`, `update` and `remove`; returns the
      rendered list — verify: update moves a host task, add is rejected on a
      locked list, unknown ops are typed errors
- [x] Pinned `task_state` context stage between `memory` and `history` — verify:
      the block contains ids and status markers and is not summarized
- [x] Driver wiring: shared handle, `TodoUpdated` event, snapshot in
      `BuildCtx` — verify: a tool call that changes the list emits the event
- [x] CLI `--todo-file` seeds and locks host tasks; each turn prints
      `tasks: done=… current=…#id total=…` — verify: the tool smoke marked a
      host task completed and wrote the file
- [x] List lifecycle: `active`, `completed`, `failed`, `abandoned`; auto-close
      when every item is terminal; explicit `close` with an outcome; closed
      lists reject edits and collapse to one summary line — verify: auto-close
      on the last completion, failed lists keep failed item lines, and
      `completed` rejects open items
- [x] Novel variant uses `task_state` and `todo`; the project seeds 50 chapter
      tasks and shortens chapters to 1,800-2,200 characters — verify: prompts
      and todos regenerate from the chapter table
- [ ] Ten-chapter smoke: no chapter drift, no gap list in the state file, all
      ten chapters present — verify: run and inspect the log, validator and
      task summary

## Verification commands

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Notes

- Task state is not history. It is not summarized, folded or recalled.
- Host items are immutable; locked lists keep the model to status updates.
- The kernel does not order or schedule tasks; that is policy.
