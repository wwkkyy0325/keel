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
- [x] Turn detail view: `--view turn --turn N` returns one turn plus the
      decoded `context_snapshots` messages and stages. Verify:
      `a_turn_view_includes_the_captured_context`.
- [x] Records panel: the session view carries every sealed record ordered by
      segment sequence. Verify: the session test asserts the record fields.
- [x] Memory timeline: `--view memory` returns typed `fact_history` events
      (`added`/`superseded`/`forgotten`) and decoded `todo_history` revisions.
      Verify: `the_memory_view_lists_fact_versions_and_task_revisions`.
- [x] Long sessions: `--limit N` and `--before <seq>` page turns backward,
      always returned ascending; `session.turns` keeps the total. Verify:
      `a_turn_window_pages_backward_in_ascending_order` and a two-page CLI
      smoke on the memory store.
- [x] Golden file: seed a store, emit the session view, normalize timestamps
      and compare to `hosts/keel-cli/tests/fixtures/session-view.golden.json`.
      Verify: `the_session_view_matches_the_golden_file`.
- [x] CLI contract tests: the real binary against a temporary store covers
      `sessions`, `session` (including two paging calls), `turn`, `memory`,
      fork, delete and the error exits, plus `--plugins-check --json`. Verify:
      `hosts/keel-cli/tests/cli_views.rs`, four tests.
- [ ] Event/audit timeline once events are stored.

## Rules

- A new field on a screen is added to a view struct and its test in the same
  change. A raw column is never handed to a frontend "for now".
- Ordering is part of the contract; adding a list means naming its `ORDER BY`.
- Views are read-only. They never write, migrate or repair.
- The CLI JSON is the frontend test fixture. A frontend test can run the CLI,
  parse the view and assert rendering without starting the app.
