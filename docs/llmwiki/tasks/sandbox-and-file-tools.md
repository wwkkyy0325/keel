# Sandbox boundary and file tools

The workspace was a permission boundary, not an OS sandbox. Three gaps made it
leak or contradict itself: symlinks were not resolved, `..` could climb above
the root, and the truncated-output marker pointed at a `.keel` path the gate
denied. The reference tool set also lacked move, delete and mkdir.

## Steps

- [x] The dispatcher canonicalizes every declared path before the permission
      gate. For a write target that does not exist, it canonicalizes the
      deepest existing ancestor and appends the rest; a dangling symlink is
      refused. Verify: `a_symlink_cannot_read_outside_the_workspace`,
      `a_symlink_cannot_write_outside_the_workspace`.
- [x] `normalize` stops at the drive or filesystem root instead of popping it,
      so `..` cannot turn an absolute path into a relative one. Verify:
      `parent_components_do_not_pop_the_filesystem_root`,
      `an_absolute_path_keeps_its_drive_and_root`.
- [x] The outputs directory is readable, so the spill marker is no longer a
      dead path; the rest of `.keel` stays denied and the exception is
      read-only. The directory is configurable (see
      `docs/llmwiki/tasks/state-outputs.md`). Verify:
      `reading_a_spilled_output_is_allowed`, `the_rest_of_keel_stays_denied`.
- [x] MCP servers start in the workspace directory, matching `roots/list`.
      Verify: the MCP transport tests plus a manual plugin run.
- [x] `make_dir`, `move_file` and `delete_file` are registered and exposed by
      the coding profiles. Delete moves into the trash on the same volume
      (copying across volumes); move backs up an existing destination and
      stays on one volume. Verify: `file_tool_tests`,
      `discard_moves_the_file_and_keeps_it_restorable` and the profile
      validation tests.
- [x] Real tidy run on a scratch folder with a shell-free profile: 7 tool
      calls, 8.1 s, 81.0% cache hit. It created `Archive`, moved three `.txt`
      files (including one from a nested folder), deleted `junk.log` into the
      trash with its original path recorded, and left `keep.md` untouched.

## Boundaries

- The gate is still a logical boundary. An approved `shell` command or MCP
  server is not confined; OS-level isolation is a host option.
- Canonicalization happens at check time, so a concurrent swap between the
  check and the tool call (TOCTOU) is not eliminated. Tools should prefer the
  typed operations over shell for anything that must be recoverable.
- `move_file` does not cross volumes; the model is told to use copy plus
  delete, or to choose a destination on the same volume.
- Multi-root job scopes, a state directory and per-volume trash remain future
  work.
