# M4 plugins

A plugin is an external process. The host connects it; the kernel stays
unchanged. MCP is the structured path; the shell tool is the unstructured one.

## Steps

- [x] Write ADR 0013: MCP stdio servers, no manifest review, flat schema
      mapping with a JSON fallback, host-side trust.
- [x] Implement the MCP client in `hosts/keel-cli/src/mcp.rs`: spawn,
      `initialize`, `tools/list`, `tools/call`, 30 s timeout, id matching.
- [x] Register each discovered tool as `mcp_<server>_<tool>` and add it to the
      profile's visible tools. Verify: the model sees the tool without a
      kernel change.
- [x] Add `--plugins <file>` with `[[servers]] name/command/args`.
- [x] Test the filesystem server: `list_directory` returned the expected
      files. Verify: `run.log`, 3 calls, 2.3 s.
- [x] Test the memory server: `create_entities` + `search_nodes` stored and
      found `Keel`. Verify: `run3.log`, 3 calls, 3.4 s.
- [x] Fix the array-of-objects crash: nested schemas switch to one JSON
      `arguments` string. Verify: six mapping unit tests; the memory retry
      succeeded.
- [x] Add remote transport: `url = "https://.../mcp"` uses streamable HTTP
      (JSON or SSE response body, `Mcp-Session-Id` echo). Verify: a local mock
      HTTP MCP server completes initialize, tools/list and tools/call.
- [ ] Live test against a domestic hosted MCP (ModelScope/Bailian/Tencent).
      Deferred: a streamable-HTTP deployment URL and a service key are not
      available; the transport already passes against a mock server.
- [ ] A DSH bridge: DSH plugins are Cordis TS modules, not MCP. Deferred: the
      agent can drive a headless `dsh` through the shell tool when needed
      instead of adding a runtime adapter now.
- [x] `--plugins-check`: probe every configured server (spawn, initialize,
      tools/list), print one status line per server and exit non-zero when any
      fails. `--json` emits the same rows as `{name, tools, error,
      duration_ms}` for a configuration page. Verify: a wrapped and a direct
      server reported ready, a missing command reported failed, exit code 1;
      the JSON form parsed with the same two rows.
- [x] Kill the server process tree on drop: `taskkill /T /F` on Windows, the
      process group on Unix. Verify: the Python servers, including the
      grandchild started through `cmd /c`, were gone after the probe.
- [x] Restart-based configuration instead of hot reload: the file is the
      source of truth, a changed file needs a host restart, and
      `--plugins-check` gates the save. Rejected in-process reload: the registry is immutable for
      a turn and a mid-turn tool swap has no correctness story.
- [x] Handle stdio server requests: `ping` is acknowledged, `roots/list`
      returns the workspace as a `file://` URI, and everything else is refused
      with JSON-RPC `-32601` instead of being ignored. Sampling is deliberately
      refused: it would spend tokens and send context outside the kernel's
      permission and audit path. Verify: the Python fixture asks for roots
      before the initialize response and `roots_seen` returns the same URI.
- [x] Server requests over streamable HTTP: a JSON or SSE body is split into
      our response and any server requests; each request is answered with a
      POST. Verify: the mock HTTP server emits a `roots/list` request beside
      the `tools/list` result and records the client's reply.
- [x] Improve the JSON fallback description for nested schemas: fields, types,
      required names and a minimal example, bounded at 900 characters, instead
      of a clipped raw schema. Verify:
      `a_nested_schema_is_described_with_fields_and_an_example`.
- [x] Test a third server family on a non-Node runtime: a Python stdlib SQLite
      server (`evals/harness/fixtures/mcp_sqlite_server.py`) passed probe,
      initialize, tools/list and tools/call (create, insert, select).

## Reports

- `evals/reports/2026-11-01-mcp-plugins.md`.
