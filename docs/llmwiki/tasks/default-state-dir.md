# Default state directory

With no `--state-dir` and no bundle `state_dir`, the host now falls back to
`KEEL_HOME` or a `.keel` directory under the user profile
(`%USERPROFILE%\.keel` on Windows, `~/.keel` elsewhere). This matches the
observable Claude Code layout — a home directory with an environment override.
No source was read or copied; the provenance ledger records the public docs.

## Steps

- [x] `state_home` checks `KEEL_HOME`, then `USERPROFILE`, then `HOME`, and
      appends `.keel`; no home means the old workspace-local fallback. Verify:
      `keel_home_overrides_the_default_state_root`,
      `the_default_state_root_is_a_home_dot_directory`,
      `home_is_the_fallback_and_no_home_means_no_state_dir`.
- [x] The resolved state directory is created and canonicalized; the store
      defaults to `<state>/keel.db`, the trash to `<state>/trash` and output
      spills to `<state>/outputs`.
- [x] An explicit `--state-dir` or a bundle's `state_dir` overrides the
      default; `--store` overrides only the database.
- [x] The eval harness sets `KEEL_HOME` to a per-case temporary directory and
      removes it, so eval runs do not touch the developer's home.
- [x] Smoke: a mock run with `KEEL_HOME` created `<home>/keel.db` and left the
      workspace empty; with both `KEEL_HOME` and `--state-dir`, the explicit
      directory won and the home stayed empty.
- [ ] A platform-native split (`%LOCALAPPDATA%`, XDG state, Application
      Support) is deliberately not used; a home dot directory keeps parity
      across platforms.

## Boundaries

- The default is a home dot directory, not a per-OS data directory. On a
  machine without `USERPROFILE`/`HOME`, the old workspace-local layout
  remains.
- Sessions now persist by default. `--store` or `--state-dir` relocates them;
  the eval harness must isolate itself with `KEEL_HOME` (it does).
- The state directory is created eagerly on a normal run. A read-only home
  fails with a clear error instead of silently falling back.
- The reference bundles no longer set a repo-local `state_dir`, so they use
  the same default as a plain run.
