# Terminal UI

An interactive reference host: streaming transcript, tool lines, permission
modal, slash commands, session resume and cancellation. Simplified on purpose:
no markdown rendering, syntax highlighting, mouse or model picker.

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
