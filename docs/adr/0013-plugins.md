# ADR 0013 — Plugins are external processes, starting with MCP

## Status

Accepted. First implementation: `hosts/keel-cli/src/mcp.rs`.

## Context

The project is not a commercial product. Plugin work should maximize what can
be connected, with the simplest possible insertion path. Review gates, manifest
approval and capability escalation add friction that this project does not
need. A plugin may be a plain CLI the agent can call, or a server that speaks a
public protocol.

The kernel's safety rules still hold: no shell interpolation of natural
language, permission checks before a tool runs, and no secrets in logs. The
question is where a plugin lives and how much ceremony surrounds it.

## Decision

1. **A plugin is an external process.** The host starts it; the kernel never
   loads a dynamic library and never knows the protocol.
2. **MCP is the structured path.** `plugins.toml` lists stdio servers:

   ```toml
   [[servers]]
   name = "filesystem"
   command = "npx"
   args = ["-y", "@modelcontextprotocol/server-filesystem", "/some/dir"]
   ```

   The host performs the MCP handshake, calls `tools/list`, and registers each
   tool as an ordinary kernel `Tool`. The model sees
   `mcp_<server>_<tool>`.
3. **CLI is the unstructured path.** Any allowlisted command can already be
   reached through the shell tool. An MCP server is for tools that need typed
   arguments or persistent state.
4. **No review step.** A server that fails to start or handshake is skipped
   with a warning; the session continues. There is no signature or manifest
   approval.
5. **Flat schemas.** MCP input properties of type string, integer/number,
   boolean and array become kernel parameters. Nested objects are skipped
   rather than exposed as a half-working string parameter.
6. **Permissions still run.** A tool with `readOnlyHint: true` asks for Read;
   everything else asks for Write, with the workspace root as the path. The
   gate cannot constrain what the server does inside its own process; the trust
   boundary is the user who installed it.
7. **One process per server, serialized calls.** All tools from a server share
   a mutex and a 30 s request timeout. Requests are matched by JSON-RPC id;
   notifications are ignored. Restart the host to reload a plugin file.

## Consequences

- Any MCP server works without a kernel change: filesystem, memory, git,
  databases, browsers.
- The tool schema is the server's, minus nested objects. A server that relies
  on nested arguments is only partly usable until the mapping grows.
- A hung server stalls its tools, not the turn: the request times out.
- The agent can add a plugin by writing `plugins.toml`; the next process
  start picks it up. No hot reload.
- Permission coverage is coarse: the kernel gates the call, not the server's
  effects.

## Alternatives rejected

- A signed manifest with capability review: friction with no threat model for a
  personal project.
- Dynamic libraries: version and ABI risk, and a kernel API surface that must
  stay stable.
- A proprietary plugin protocol: every integration would need its own adapter.
