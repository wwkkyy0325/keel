# Frontend views

Rule: a frontend renders; it does not join tables, sort rows, decode JSON
columns or reconcile timestamps. The host emits a composed view, and every
view is tested without starting a UI.

## Steps

- [x] `--view sessions`: the session list, ordered by `updated_at` then `id`,
      with branch metadata. Verify: `the_session_list_is_composed_and_stable`.
- [x] `--view session --session-id`: memo with covers, facts, tasks, branches
      and turns; `answer_model` and `usage` are decoded by the host. Verify:
      `a_session_view_carries_every_panel_the_ui_renders` and
      `a_branch_view_starts_at_the_fork_state`.
- [x] Determinism: two calls on the same store serialize identically. Verify:
      the list test compares both JSON strings.
- [x] Missing session exits non-zero instead of rendering an empty screen.
      Verify: `an_unknown_session_has_no_view` and the CLI error path.
- [x] Contract documented in `docs/llmwiki/frontend-contract.md`.
- [ ] Turn detail view: one turn with its three answer layers and the
      `context_snapshots` messages decoded, so a debugging screen needs no SQL.
- [ ] Memory timeline: `fact_history` and `todo_history` as typed events, with
      a stable order and no JSON string in the payload.
- [ ] Long sessions: explicit limits/cursors for turns and transcripts rather
      than serializing a whole store into one response.
- [ ] Golden files: seed a store, emit the views, compare to a checked-in
      fixture with timestamps normalized. This guards the field names against
      accidental renames.
- [ ] Event/audit timeline once events are stored.

## Rules

- A new field on a screen is added to a view struct and its test in the same
  change. A raw column is never handed to a frontend "for now".
- Ordering is part of the contract; adding a list means naming its `ORDER BY`.
- Views are read-only. They never write, migrate or repair.
- The CLI JSON is the frontend test fixture. A frontend test can run the CLI,
  parse the view and assert rendering without starting the app.
