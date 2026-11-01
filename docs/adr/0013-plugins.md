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
2. **MCP is the structured path.** `plugins.toml` lists a stdio server or a
   remote streamable-HTTP endpoint:

   ```toml
   [[servers]]
   name = "filesystem"
   command = "npx"
   args = ["-y", "@modelcontextprotocol/server-filesystem", "/some/dir"]

   [[servers]]
   name = "hosted"
   url = "https://example.invalid/mcp"
   ```

   The host performs the MCP handshake, calls `tools/list`, and registers each
   tool as an ordinary kernel `Tool`. The model sees
   `mcp_<server>_<tool>`. The HTTP transport POSTs JSON-RPC, accepts a JSON or
   SSE body, and echoes `Mcp-Session-Id` when the server sets it. A pure SSE
   endpoint (`GET /sse` plus a separate message POST) is not implemented; a
   hosted service must expose streamable HTTP.
3. **CLI is the unstructured path.** Any allowlisted command can already be
   reached through the shell tool. An MCP server is for tools that need typed
   arguments or persistent state.
4. **No review step.** A server that fails to start or handshake is skipped
   with a warning; the session continues. There is no signature or manifest
   approval.
5. **Flat schemas first, one JSON argument for nesting.** MCP input properties
   of type string, integer/number, boolean and array become kernel parameters.
   If any property is a nested object or object array, the whole tool becomes
   one `arguments` string whose description lists every field with its type and
   required mark, recurses into arrays, and includes a minimal example. The
   kernel's parameter model does not grow for MCP.
6. **Permissions still run.** A tool with `readOnlyHint: true` asks for Read;
   everything else asks for Write, with the workspace root as the path. The
   gate cannot constrain what the server does inside its own process; the trust
   boundary is the user who installed it.
7. **One process per server, serialized calls.** All tools from a server share
   a mutex and a 30 s request timeout. Requests are matched by JSON-RPC id.
   Server-initiated `ping` and `roots/list` are answered over both transports:
   a stdio reply is one line on stdin, an HTTP reply is a POST to the endpoint.
   Other methods are refused. Sampling is refused: it would spend tokens and
   send context outside the permission and audit path.
8. **Configuration is restart-based, with a probe gate.** The plugin file is
   the source of truth. `--plugins-check [--json]` starts every server,
   performs the handshake and lists tools before a configuration is saved; a
   failed probe exits non-zero and the caller keeps the previous file. A host
   restart applies the change. Restart is safe because session state lives in
   SQLite and every server is killed as a process tree on drop, so a stale
   child cannot hold a port or file into the next start.

## Consequences

- Any MCP server works without a kernel change: filesystem, memory, git,
  databases, browsers.
- The tool schema is the server's. Nested arguments reach the model as a
  described JSON string rather than typed parameters, so a complex tool is
  usable but the model must construct the object.
- A hung server stalls its tools, not the turn: the request times out.
- The agent can add a plugin by writing `plugins.toml`; the next process
  start picks it up. No hot reload; a configuration page probes before saving
  and asks the supervisor to restart.
- Permission coverage is coarse: the kernel gates the call, not the server's
  effects.

## Alternatives rejected

- A signed manifest with capability review: friction with no threat model for a
  personal project.
- Dynamic libraries: version and ABI risk, and a kernel API surface that must
  stay stable.
- A proprietary plugin protocol: every integration would need its own adapter.
