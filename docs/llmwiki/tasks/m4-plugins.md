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
- [ ] Hot reload of `plugins.toml` without restarting the host.
- [ ] Handle server-initiated requests (sampling, roots) instead of ignoring
      them.
- [ ] Map nested schemas to typed parameters (or generate a JSON schema tool
      description) so the model does not have to invent structure.
- [ ] Test a third server family (git or sqlite) and a non-Node runtime.

## Reports

- `evals/reports/2026-11-04-mcp-plugins.md`.
