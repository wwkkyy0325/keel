# Extra roots

A task can span directories outside the workspace without widening the
workspace to a whole drive. The host adds roots, each with an access level.

## Steps

- [x] `PermissionEngine::with_root(path, access)` and `RootScope`: read-only
      or read-write, the most specific root wins, execution stays
      confirm-only, and a drive/filesystem root or system directory is
      refused. Verify: `an_extra_read_root_allows_reads_and_confirms_writes`,
      `an_extra_write_root_allows_reads_and_writes`,
      `the_most_specific_root_wins`, `a_filesystem_root_is_rejected`.
- [x] `--root PATH[:ro|:rw]`, repeatable; the suffix is parsed from the end so
      a Windows drive letter is not mistaken for it. Verify:
      `root_specs_parse_read_and_write_suffixes`.
- [x] MCP `roots/list` advertises every scope and the spawn working directory
      stays the primary. Verify:
      `server_requests_are_answered_by_method`.
- [x] The TUI lists scopes with `/roots`. Verify:
      `slash_roots_lists_every_scope`.
- [x] Real two-root smoke: a read-only extra root, four model calls, 4.2 s,
      83.7% cache hit. The agent read `note.txt` by its absolute path and wrote
      `summary.txt` in the workspace; the extra directory was unchanged.

## Boundaries

- Relative tool paths stay in the primary workspace; extra roots use absolute
  paths.
- One writable root per phase; keep the rest read-only, and use separate
  sessions when the phases should not share undo history.
- The boundary is still logical. An approved `shell` command or MCP server is
  not confined by the roots.
- The root list is a host/session setting, not a profile field. A job file
  that bundles roots, profile and state is future work.
