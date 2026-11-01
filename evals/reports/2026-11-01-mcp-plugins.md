# MCP plugin adapter

Date: 2026-11-01. Two real MCP servers, both installed from npm, driven by the
coding profile through the CLI. The point is compatibility: can a mainstream
plugin be connected without a kernel change?

## Setup

- `plugins.toml`:

  ```toml
  [[servers]]
  name = "filesystem"
  command = "cmd"
  args = ["/c", "npx", "-y", "@modelcontextprotocol/server-filesystem", "<dir>"]

  [[servers]]
  name = "memory"
  command = "cmd"
  args = ["/c", "npx", "-y", "@modelcontextprotocol/server-memory"]
  ```

  Windows needs `cmd /c` because `npx` is a `.cmd` shim.

- The CLI spawns both, performs the MCP `initialize` handshake, calls
  `tools/list`, and registers every tool as `mcp_<server>_<tool>`.
- No kernel code changed: the adapter lives in the host and the kernel sees
  ordinary typed tools.

## Results

| Server | Tool | Result |
|---|---|---|
| filesystem | `mcp_filesystem_list_directory` | Listed `note.txt`, `plugins.toml`, `run.log`; 3 model calls, 2.3 s |
| memory | `mcp_memory_create_entities` + `mcp_memory_search_nodes` | Stored and found `Keel` (type `project`, one observation); 3 model calls, 3.4 s |

Both turns ended `Done`; the model chose the MCP tool from its name and
description without any special prompt engineering beyond the task.

## What broke and the fix

The memory server's `create_entities` takes an array of objects:

```json
{"entities": [{"name": "...", "entityType": "...", "observations": ["..."]}]}
```

The first mapping exposed every `array` as `ParamKind::StringList`. The model
sent an array of objects, the kernel's schema validation rejected it with
`string-list arguments must contain strings`, and the turn failed.

Fix: a schema maps to flat parameters only when every property is a string,
number, boolean or array of strings. Any nested object or object array switches
the whole tool to JSON mode: one optional string parameter `arguments` that
carries a JSON object, with the schema appended to the description. The retry
succeeded on the first call.

This is the right trade for a compatibility-first adapter: a partly mapped flat
schema silently drops fields, while the JSON mode can express anything.

## Remote MCP and other ecosystems

The two live servers above are stdio. Domestic MCP markets (ModelScope,
Alibaba Cloud Bailian, Tencent Cloud) mostly expose **hosted remote MCP**, so
the adapter now supports streamable HTTP as well:

```toml
[[servers]]
name = "hosted"
url = "https://example.invalid/mcp"
```

It POSTs JSON-RPC, accepts JSON or SSE response bodies, and echoes
`Mcp-Session-Id` when the server sets it. A pure SSE endpoint (`GET /sse` plus
a separate message POST) is not implemented; a hosted deployment must use
streamable HTTP. Verified against a local mock HTTP MCP server (initialize,
tools/list, tools/call); a live domestic endpoint still needs an account URL
and, for many services, an API key.

**DeepSeek Harness (DSH).** DSH is a separate agent runtime built on Cordis;
its plugins are TypeScript/ESM modules that call `ctx.tools.register(...)`
inside the harness process. They are not MCP servers and cannot be loaded by
Keel as-is. Options are a Cordis-hosted bridge that republishes DSH tools over
MCP, or invoking a headless `dsh` run through the shell tool as an external
agent. Neither is implemented.

**Dify / Coze.** Their plugin packages target their own runtimes and HTTP APIs,
not MCP. A host-side adapter would call the platform API; it is a different
integration path from this adapter.

## Limits

- Flat parameters only; nested schemas need the JSON string. The model must
  produce valid JSON or the call fails with `invalid_arguments`.
- One process per server, calls serialized by a mutex, 30 s request timeout.
  No server-to-client requests (sampling, roots) are handled.
- No hot reload: restart the host after editing `plugins.toml`.
- Permission coverage is coarse. The kernel gates the call (Read for
  `readOnlyHint`, Write otherwise) but cannot sandbox the server. A filesystem
  MCP write bypasses the kernel's trash backup; the server is trusted host
  software.
- Startup is network-dependent on first use because `npx` downloads the
  package.

## Artifacts

- Logs: `D:\workspace-new\wwkkyy0325\fusion-run\mcp-test\run.log` (filesystem),
  `run3.log` (memory).
- Config: `fusion-run/mcp-test/plugins.toml`.
- Adapter: `hosts/keel-cli/src/mcp.rs`; ADR 0013.
