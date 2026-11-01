# Tool contract hardening

Two gaps found while auditing the tool layer as a kernel: MCP `number`
parameters were advertised as integers, and nothing enforced that a read-only
tool only declares read access.

## Steps

- [x] A `number` MCP property switches the whole tool to one `arguments` JSON
      string instead of being flattened to an integer. Verify:
      `a_fractional_number_switches_to_json_mode`; the existing
      `flat_schema_properties_become_params` still passes.
- [x] The dispatcher rejects a tool declared `ReadOnly` that requests `Write`
      or `Execute`, before the permission gate and before the call. Verify:
      `a_read_only_tool_may_not_request_write_access` (permission-denied
      error, the tool is not called, and a `PermissionDecided` deny event is
      recorded).

## Boundaries

- The kernel argument model stays at string, integer, boolean and string list.
  Fractional numbers travel only through the JSON argument path.
- The check is one-directional: mutating or dangerous tools may still request
  read access.
- Trust in an external MCP server is still the user's: the gate constrains the
  call, not what the server process does.
