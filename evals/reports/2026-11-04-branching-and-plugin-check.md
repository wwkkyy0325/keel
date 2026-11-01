# Branching and plugin checks

Date: 2026-11-04. Two host-layer slices: fork/delete on the SQLite store, and a
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

- The CLI prints the kernel's per-process turn id (`turn: 1` after a resume),
  not the store sequence. The transcript rows and checkpoints use the correct
  persisted sequence; only the live label restarts.
- Checkpoints are not pruned; storage grows with the transcript. That is host
  policy and is left open.
- Server-initiated requests (sampling, roots) are still ignored.
- The nested-schema JSON fallback still clips the raw schema at 600 characters;
  the field/type/example description is not implemented.
