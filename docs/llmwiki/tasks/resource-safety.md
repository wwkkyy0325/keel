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

## Out of scope

- Event log retention across turns.
- Checkpoint default retention.
- MCP response size caps.

Those are recorded in the design notes and are separate changes.
