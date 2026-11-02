# Outputs in the state directory

Long tool output was the last runtime file left in the workspace. It now
follows the state directory, and the spill marker names the absolute path so
the model can read it back from either location.

## Steps

- [x] `summarize_output` takes the outputs directory instead of deriving
      `.keel/outputs` from the workspace, and the marker carries the absolute
      path. Verify: `summary::tests`.
- [x] `PermissionEngine::with_outputs(path)` makes the configured directory
      readable, before the `.keel` deny check; the exception is read-only.
      Verify: `a_configured_outputs_directory_is_readable`,
      `reading_a_spilled_output_is_allowed`.
- [x] `TurnDriver::with_outputs_dir(path)` writes spills there; the default is
      still `<workspace>/.keel/outputs`. Verify:
      `a_large_tool_result_is_summarized_and_spilled`.
- [x] The host canonicalizes the state directory and sets outputs to
      `<state>/outputs`, or the workspace default when no state directory is
      configured.
- [x] Smoke: a 5000-character echo through the mock profile with
      `--state-dir`. The spill landed in `<state>/outputs/turn-1-call-0.txt`
      (5000 bytes), the store was in `<state>/keel.db`, and the workspace had
      no `.keel` directory.
- [x] The outputs directory keeps the newest 200 `turn-*-call-*.txt` files;
      pruning is best effort and leaves other files alone. Verify:
      `old_spills_are_pruned_and_other_files_are_kept`.
- [x] The state root now has a default (`KEEL_HOME` or `~/.keel`); see
      `docs/llmwiki/tasks/default-state-dir.md`.

## Boundaries

- The read exception covers only the configured outputs directory and only
  reads; everything else under `.keel` stays denied.
- Without a resolvable state root (no `--state-dir`, bundle path, `KEEL_HOME`
  or home directory), the spill stays in `<workspace>/.keel/outputs`.
- The marker is an absolute path. A model that is not allowed to read it gets
  the normal permission decision; the exception is what makes it reachable.
- Pruning keeps the newest 200 files, so a marker to an older spill can now
  return `NotFound`; the model can re-run the tool to recreate the output.
