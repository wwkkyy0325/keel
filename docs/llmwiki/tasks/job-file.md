# Job file

A job binds composition, scope, state and the first input into one reusable
task, so a repeated operation is one file instead of a long flag list.

## Steps

- [x] `job::Job::load` reads `bundle` or `profile`/`plugins`, `workspace`,
      `roots`, `state_dir`, `store`, `session_id` and `input`. Every path
      resolves relative to the job file; unknown fields and unknown access
      values are rejected. Verify: `job::tests`.
- [x] `--job PATH` applies the job as a default layer: explicit CLI
      composition wins as a whole, `--workspace`/`--input`/`--session-id`/
      `--state-dir`/`--store` override their fields, and CLI `--root` entries
      are added to the job's roots.
- [x] A job that sets `bundle` and a bundle's own `state_dir` compose: the job
      picks the bundle, and the bundle supplies composition and state unless
      the job or CLI overrides them.
- [x] Real smoke: a temporary job referenced a temporary bundle (mock
      profile), set `workspace = "ws"`, `session_id = "job-smoke"` and one
      read-only root. The run loaded `profile: coding`, created
      `state/keel.db`, and `--view sessions` showed `job-smoke`; the workspace
      stayed empty.

## Boundaries

- `input` runs in line mode. `--tui --job` applies the scope and state but
  starts with an empty prompt; prefilling or auto-running the TUI is future
  work.
- Precedence is deliberately simple: CLI composition replaces the job's
  composition; CLI roots add to the job's roots.
- A job is data, not a scheduler. Running it repeatedly, watching a folder or
  retrying on failure belongs to the host or an external scheduler.
- The job file never grants full auto and never bypasses the permission gate.
