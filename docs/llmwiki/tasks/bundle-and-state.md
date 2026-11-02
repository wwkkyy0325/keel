# Bundle and state directory

A bundle ties the profile, optional plugins and optional state directory into
one file, so a variant is one flag instead of four. The state directory keeps
sessions and undo data out of the task directory.

## Steps

- [x] `Bundle::load` resolves `profile`, `plugins` and `state_dir` relative to
      the bundle file; absolute paths are kept. `--bundle` is mutually
      exclusive with `--profile` and `--plugins`. Verify: `bundle::tests` and
      the conflict smoke run.
- [x] `--state-dir PATH` (or a bundle's `state_dir`) supplies
      `<state>/keel.db` when `--store` is absent and moves the trash to
      `<state>/trash`. `LocalTrashStore::at(dir)` accepts the directory
      directly. Verify: `a_state_directory_supplies_the_default_store`,
      `at_uses_the_given_directory_directly`.
- [x] Reference bundles `variants/coding/bundle.toml` and
      `variants/companion/bundle.toml`. Verify: a mock bundle run from a
      scratch directory loaded `profile: coding` and created
      `state/keel.db`; the workspace stayed empty.
- [ ] An OS-level default state directory and moving the output spill out of
      the workspace remain future work.

## Boundaries

- The bundle is data. The kernel never sees it; the host resolves it into the
  existing profile, plugin and permission paths.
- `--store` wins over the state-derived database; an explicit `--state-dir`
  overrides a bundle's value.
- Truncated tool output stays in `<workspace>/.keel/outputs` because the model
  reads it back; the state directory covers the store and the trash.
- A bundle may name a plugin file but does not discover plugins.
