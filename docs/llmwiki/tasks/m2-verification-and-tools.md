# M2 — Verification and coding tools

Goal: make one coding task solvable end-to-end and verifiable by the kernel,
not by the model's own claim.

## Steps

- [x] 1. Acceptance types and deterministic verifiers: `AcceptanceItem`,
      `VerifySpec`, `Evidence`, `VerificationReport`; compute and file-hash
      verifiers run real checks — verify: verifier unit/integration tests
- [x] 2. Read-only tools: `list_dir` and `code_search` with permission requests
      and bounded output — verify: dispatch and permission tests
- [x] 3. Structured shell tool: program plus argument list, no interpolation,
      allowlist, permission request; command and test verifiers on top — verify:
      allowlist, timeout and permission tests
- [x] 4. One coding eval case with `setup/`, `hidden/` and `verify.sh` — verify:
      the case rejects an unsolved workspace and accepts the reference fix
- [ ] 5. Real model end-to-end run — verify: user runs the DeepSeek profile
      with a local `.env` and records the transcript

## Decisions

- The current context assembly is single-turn: `BuildCtx.history` is empty.
  This is an implementation limit, not a prompt rule.
- Prompt convention: prohibitions first, recommendations after; instructions in
  English; entity names and user-provided text keep their original language and
  spelling.
- The kernel produces `Evidence`; the model cannot set `verified`.
- Compute verification uses a small arithmetic parser, never `eval` or shell.
- File-hash paths are relative to the workspace and may not escape it.
- Command and test verification wait for the structured shell tool, so they
  cannot grow a second, interpolating execution path.
- Deferred items and their triggers are recorded in
  [backlog.md](../backlog.md).

## User points recorded

- Prompt convention: prohibitions first, recommendations after; write
  instructions in English; keep entity names and user-provided text in their
  original language and spelling.
- Deferred until their triggers: history, memory, compaction, base profile,
  prompt systemization and a unified pipeline registry.
- Verification must be real; the model's narrative is not evidence.

## Result

- Step 1 adds the typed checklist and deterministic compute/file-hash
  verification. Command, test and human items return `Unverified` until their
  execution path exists.
- File-hash paths are workspace-relative and may not escape the root.
- Calculator is a bounded parser; no shell, no `eval`.
- Step 2 adds `list_dir` and `code_search`. Both are read-only, request read
  permission, do not follow symlinks, and bound entries, file size, depth and
  output.
- The coding profiles now expose five tools: `echo`, `read_file`, `write_file`,
  `list_dir`, `code_search`.
- Step 3 adds `shell` and command/test verification:
  - program and arguments are separate; no shell and no interpolation;
  - programs must match an allowlist; paths and shell names are rejected;
  - the confirmation prompt shows the command line;
  - child processes do not inherit environment names that look like keys,
    tokens, secrets, passwords or credentials;
  - command/test verifiers use the same execution path and a 60 s bound.
- The CLI uses a terminal interaction port. A non-terminal stdin defers, so
  headless runs deny permission-gated actions.
- Coding profiles expose six tools, including `shell`.
- Step 4 adds `evals/cases/coding/task-001`: an empty-list mean fix with
  hidden unittest coverage and a reference fix. The case rejects the initial
  workspace and accepts the reference; this was checked with `verify.sh`.
- The CLI accepts `--workspace PATH`, so an eval runs in a temporary workspace
  while `.env` and the profile still load from the repository.
- `cargo test --workspace` passes: 122 tests.
- Step 5, the real model run, needs a local `.env` and is not yet executed.
