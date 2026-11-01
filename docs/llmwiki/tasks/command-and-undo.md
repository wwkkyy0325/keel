# Command ergonomics and undo

The kernel keeps a structured exec path with no shell. Two gaps remained:
Windows resolves `npm`/`npx` and friends as `.cmd` shims that a bare name does
not start, and the trash had no way back, so a rollback meant the model
rebuilding the file or running destructive git commands.

## Steps

- [x] Resolve Windows `.cmd`/`.bat` shims from PATH when a bare program name is
      not found. Verify: `a_windows_command_shim_is_resolved` runs `npm
      --version` through `command::run` on Windows.
- [x] Record the original path beside each backup in
      `.keel/trash/meta/<id>.toml`. Verify: `backups_are_listed_newest_first_and_found_by_id`.
- [x] Add `TrashStore::list`/`find` (with defaults so existing backends still
      compile) and the `list_trash`/`restore_file` tools. Verify:
      `restore_file_restores_the_newest_backup_and_keeps_the_current_version`,
      `restore_file_rejects_an_entry_for_another_path`,
      `restore_file_reports_a_missing_backup`,
      `list_trash_names_the_backups_and_their_paths`.
- [x] Enable the tools in the coding profiles. Verify: profile validation and
      a CLI smoke run.
- [x] Add `make_dir`, `move_file` and `delete_file`. Delete copies the file
      into the trash before removing it; move backs up an existing
      destination and stays on one volume. Verify: `file_tool_tests`.

## Decisions

- No shell is added. Pipes remain unavailable; `python`/`node` scripts stay the
  audited path. Git Bash is not a kernel dependency.
- `restore_file` takes the target path from the argument, so the permission
  gate approves the actual file, not a path read from the trash metadata. An
  entry is accepted only when its recorded original matches that target.
- The current version is backed up before a restore, so undo is itself
  reversible.
