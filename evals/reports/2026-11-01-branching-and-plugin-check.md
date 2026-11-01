# Branching and plugin checks

Date: 2026-11-01. Two host-layer slices: fork/delete on the SQLite store, and a
plugin probe that gates a restart-based configuration flow. No kernel change.

## Branching

Setup: the mock profile (`variants/coding/profile.toml`), no tools, one SQLite
store. The parent ran two turns; the branch was forked at turn 1 into a new
session id and ran one more turn.

| Check | Result |
|---|---|
| Parent rows after two turns | 2 turns, 2 checkpoints, 2 segments |
| Fork | `forked parent:1 into branch` |
| Branch rows | 2 turns (1 copied + 1 new), 2 checkpoints, 2 segments |
| Parent lineage | `parent_session_id = parent`, `fork_turn_seq = 1` |
| Parent after the branch turn | still 2 turns; no rows changed |
| Delete branch | all branch rows gone; parent rows still 2 |
| Delete again | `session \`branch\` not found`, exit 1 |

The branch snapshot is the state after turn 1, not the parent's head. It was
written before the parent ran its second turn, so the test separates a
checkpoint restore from a transcript replay.

## Plugin check

`--plugins-check --plugins <file>` starts each server, runs `initialize` and
`tools/list`, prints one line per server and exits 1 if any failed.

```
server wrapped: ready, 1 tools, 155 ms
server direct: ready, 1 tools, 131 ms
server broken: failed after 0 ms: cannot start `definitely-not-a-real-command`: program not found
2/3 servers ready
exit=1
```

`wrapped` is `cmd /c python <fixture>`: the direct child is `cmd`, and the
Python process is a grandchild. Both the wrapped and direct servers wrote their
PID at startup. After the probe returned, both PIDs were gone (`Get-Process`
reported nothing), which verifies the tree kill rather than a direct-child
kill.

## Third server family

`evals/harness/fixtures/mcp_sqlite_server.py` is a minimal MCP stdio server on
the Python standard library, backed by SQLite. It supports the handshake,
`tools/list`, and a `sqlite_query` tool. The unit test probes it through
`check`, then calls `CREATE TABLE`, `INSERT` and `SELECT` through the protocol;
the last result is `[[7]]`. The test returns early when `python` is not on
PATH, so it does not add a runtime dependency to the workspace.

## Verification

- 278 tests pass; `cargo fmt --check`, `cargo clippy --workspace --all-targets
  -- -D warnings` and `cargo deny check` are clean.
- Store tests: checkpoint per turn, fork from a checkpoint, fork errors
  (missing turn, taken id, same id), delete removes every row, branch outlives
  its parent.

## Known gaps

- The CLI printed the kernel's per-process turn id (`turn: 1` after a resume)
  when this run was made. It now prints the persisted session sequence; see
  the follow-up.
- Checkpoints are not pruned; storage grows with the transcript. That is host
  policy and is left open.
- Server-initiated requests were ignored when this run was made; the stdio
  path now answers them (see the follow-up). HTTP still drops them.
- The nested-schema JSON fallback still clipped the raw schema when this run
  was made; the field/type/example description landed afterwards.

## Follow-up: nested schema description and server requests

- Nested schemas still use one `arguments` string, but the description now
  carries the shape. For the memory server's `create_entities` it reads:

  `entities: array of object (required); entities[].entityType: string
  (required); entities[].name: string (required); entities[].observations:
  array of string Example: {"entities":[{"entityType":"text","name":"text",
  "observations":["text"]}]}`

  bounded at 900 characters. The raw-schema clip that could cut fields off
  the end is gone.
- Server-initiated requests over stdio now get answers: `ping` returns `{}`,
  `roots/list` returns `{"roots":[{"uri":"file:///...","name":"workspace"}]}`,
  and other methods (including sampling) get `-32601`. The client advertises
  `roots` with `listChanged: false`.
- The Python fixture now sends `roots/list` before its initialize response and
  exposes `roots_seen`; the test asserts the URI the server received equals the
  workspace URI the client computed. The fixture therefore reports two tools,
  where the earlier probe in this report saw one.
- Streamable HTTP now answers server requests too: a JSON or SSE body is split
  into the client's response and any server requests, and each request is
  answered with a POST. The mock server emits `roots/list` beside the
  `tools/list` result and the test asserts it receives the workspace reply.
- Resumed turn numbering: the loop now prints `next_turn_seq` from the store.
  Smoke: two turns as one process, one more as a second process → `turn: 3`;
  a fork from turn 1 printed `turn: 2` on its first new turn.

## Follow-up: checkpoint retention

- `--keep-checkpoints N` keeps the newest N checkpoints per session and pins
  every turn an existing branch was forked from. Default `0` keeps all.
- Store test: five turns, a fork at turn 1, limit 2 → checkpoints `[1, 4, 5]`;
  forking from the pruned turn 2 reports `no checkpoint at turn 2`.
- CLI smoke: three turns with `--keep-checkpoints 2` left `[2, 3]`; a fork
  from turn 1 failed with the typed error above. The transcript still has all
  three turns.

## Follow-up: JSON plugin status

`--plugins-check --json` returns the same rows in a stable shape for a
configuration page:

```json
[
  {"name": "sqlite", "tools": 2, "error": null, "duration_ms": 145},
  {"name": "broken", "tools": 0, "error": "cannot start `nope`: program not found", "duration_ms": 0}
]
```

The exit code is still 1 when any entry has a non-null `error`.
