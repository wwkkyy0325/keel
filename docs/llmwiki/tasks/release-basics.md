# Release basics

Before packaging, the host needs a usage surface and the CI gates must run on
the platforms the project claims.

## Steps

- [x] `--help`/`-h` and `--version`/`-V` are handled before argument parsing,
      so they work even when other flags are present or the profile is
      missing. Verify: `help_and_version_are_detected_before_parsing` and a
      direct `cargo run -- --help`/`--version`.
- [x] CI runs `fmt`, `clippy` and `cargo test --workspace` on Ubuntu, Windows
      and macOS (`fail-fast: false`). Verify: the workflow matrix in
      `.github/workflows/ci.yml`.
- [x] A separate `deny` job keeps the Linux-only cargo-deny action out of the
      OS matrix.
- [x] The kernel job runs the kernel-only tests and the `minimal_turn` example
      on all three platforms, proving the host-free boundary.
- [ ] Release binaries, installers and tags remain future work.

## Boundaries

- `--help` is a concise reference, not an exhaustive flag list; README.md
  carries the examples.
- `--version` prints the host package version, which follows the workspace
  version.
- The eval harness stays on Ubuntu: it needs bash and the dataset fixtures,
  not the host binary.
- CI cannot open a real TTY, so the terminal UI's raw-mode smoke stays manual.
