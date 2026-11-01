# Resource safety

The kernel bounds its own state, but tool execution had three gaps found while
reviewing long-running use: command output was buffered in full before
truncation, `read_file` had no size limit, and a timed-out command killed only
its direct child. This task closes the first three items from that review.

## Steps

- [x] Command output is streamed into a bounded head and tail. Verify: unit
      test on the capture buffer with 16 MiB input, plus a real command that
      writes 200 kB and is reported as truncated.
- [x] A timed-out or cancelled command kills its process tree. Verify: Python
      fixture spawns a grandchild, the command times out, the grandchild is no
      longer alive.
- [x] `read_file` refuses files over 8 MiB before reading them. Verify: a
      sparse file over the limit returns `InvalidArguments` with the limit in
      the message; a normal file still reads.
- [x] Shell timeout is host-configurable and matches the dispatcher timeout.
      Verify: `--tool-timeout-ms` reaches `ShellTool`, and a slow command stops
      at the configured timeout.

## Follow-up

- [x] Event log retention: `EventLog::clear` drops persisted records between
      turns and keeps the sequence counter monotonic; the reference host calls
      it after each turn is printed and saved. Verify:
      `clear_drops_records_and_keeps_sequence_numbers`.
- [x] Checkpoint retention default: the store keeps the 20 most recent
      checkpoints per session unless `--keep-checkpoints` says otherwise;
      `0` still keeps all, and fork points are pinned. Verify:
      `the_default_checkpoint_cap_bounds_the_history`.
- [x] MCP message caps: a stdio message or HTTP body over 8 MiB is refused
      instead of buffered, and the HTTP body read is under the request
      timeout. Verify: `an_oversized_stdio_message_is_refused`,
      `an_oversized_http_body_is_refused`,
      `a_stdio_message_is_read_up_to_its_newline`.
