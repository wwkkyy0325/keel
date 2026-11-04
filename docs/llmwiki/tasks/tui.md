# Terminal UI

An interactive reference host: streaming transcript, tool lines, permission
modal, slash commands, session resume and cancellation. Simplified on purpose:
no markdown rendering, syntax highlighting, mouse or model picker.

The daily-use iteration is `docs/llmwiki/tasks/tui-daily.md`: one screen, only
the information the user acts on, two settings, Chinese and English.

## Steps

- [x] `--tui` entry point with a terminal guard, raw mode and alternate screen.
      Verify: `the_tui_requires_an_interactive_terminal` (a piped stdin exits
      with a clear error instead of entering raw mode).
- [x] Pure `App` reducer: input editing, history, Enter/submit, slash parsing,
      Ctrl-C cancel/quit, modal answers. Verify: the reducer unit tests,
      including CJK backspace.
- [x] Renderer split from the loop and tested with `TestBackend`. Verify:
      `the_empty_screen_has_a_header_input_and_status`,
      `a_streamed_answer_is_rendered_in_the_transcript`,
      `the_permission_modal_lists_every_scope`, plus goldens under
      `hosts/keel-cli/tests/fixtures/tui-*.txt`.
- [x] Streaming: a `HostModel::Tui` wrapper forwards `TextDelta` to the UI and
      passes the same chunk to the driver. Verify:
      `deltas_stream_into_the_assistant_block`.
- [x] Tool progress: the UI drains `EventLog` by sequence and renders one line
      per call. Verify: `tool_events_become_one_line_blocks`.
- [x] Interactions: a channel-backed `InteractionPort` is answered by the modal
      using the same `parse_response` as the line UI. Verify:
      `permission_keys_map_to_the_same_scopes_as_the_line_ui`.
- [x] Turn completion saves through the existing store, clears the event stream
      and updates usage. Verify: the existing session persistence tests plus
      `a_session_persists_every_turn_after_the_event_log_is_cleared`.
- [x] A finished turn hides its process: streamed text, tool lines and notices
      are dropped, and the transcript keeps the user message, the answer and
      one aggregate line (`tools · files · tokens · duration`). Verify:
      `a_finished_turn_hides_its_process`,
      `a_finished_turn_renders_only_the_result_and_its_fields`, golden
      `tui-result.txt`.
- [x] The permission prompt is inline above the input rather than a centered
      modal. Verify: `the_permission_prompt_is_inline`, golden
      `tui-prompt.txt`.
- [x] Prompt options are a vertical list: `↑`/`↓` selects, `Enter` confirms,
      the default is Allow once, and letters remain shortcuts. Verify:
      `arrows_select_a_scope_and_enter_confirms_it`.
- [x] `Shift+Tab` switches the session mode; the bottom status line is the
      single place that shows it. The driver exposes `escalate_full_auto` and
      `revoke_permission_grants`. Verify: `shift_tab_requests_a_mode_toggle`;
      the grant primitives are covered by the permission tests. The UI mode
      uses a session-long TTL and is never shown as a notice.
- [x] The transcript keeps every turn. Finished turns hide their process as
      blocks; `Ctrl-O` reveals or hides them for the whole history. Verify:
      `a_finished_turn_hides_its_process`, `ctrl_o_toggles_process_details`.
- [x] Scrolling starts from the current bottom instead of jumping to the top,
      and a scrollbar appears when the transcript overflows. Verify:
      `page_up_starts_at_the_bottom_and_page_down_returns_to_it`.
- [x] `--demo` (mock only) gives the UI something to show: a preamble, a
      `shell` call that waits two seconds, and an answer streamed in six-char
      pieces. A `Paced` model wrapper delays chunks; `DemoModel` alternates a
      tool call and an answer so every turn has both phases. Verify:
      `text_chunks_split_on_char_boundaries`,
      `the_demo_script_runs_a_sleeping_shell_tool`, and a manual run with
      `--tui --demo`.
- [x] Tool output is visible: `ToolFinished` carries a bounded, redacted
      `output_preview` that serde skips, so it never reaches the store. A tool
      line shows the head and tail while its turn runs and the whole preview
      under `Ctrl-O`. Verify: kernel tests
      `a_preview_is_redacted_and_bounded`,
      `a_tool_output_preview_is_not_persisted`; host test
      `a_running_tool_shows_a_short_output_excerpt`.
- [x] `examples/demo-workspace` holds a failing-test task for the first-run
      walkthrough. Verify: `python -m unittest -v` fails before the fix and
      passes after it.
- [x] One real turn against `examples/demo-workspace` with
      `profile.deepseek.toml`: 1/1 Done, `stats.py` changed, 3/3 tests after
      the edit, 5 model calls, 5.4 s, 81.6% cache hit. The same workspace is
      the README walkthrough, so the UI is not a scripted shell.
- [ ] Real-terminal smoke (raw mode restore, resize, Ctrl-C) is manual; CI
      cannot open a TTY.

## Boundaries

- The TUI is a host under `hosts/keel-cli`; the kernel crates do not depend on
  `ratatui`, `crossterm` or the UI loop.
- Slash commands are local: `/help`, `/clear`, `/cost`, `/sessions`, `/cancel`,
  `/quit`. Session switching happens by starting with `--store` and
  `--session-id`; `/resume` is not implemented in the simplified UI.
- Non-permission interactions with options are selected by number; freeform
  questions without options are deferred.
