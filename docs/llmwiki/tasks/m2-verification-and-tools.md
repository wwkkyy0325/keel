# M2 — Verification and coding tools

Goal: make one coding task solvable end-to-end and verifiable by the kernel,
not by the model's own claim.

## Steps

- [x] 1. Acceptance types and deterministic verifiers: `AcceptanceItem`,
      `VerifySpec`, `Evidence`, `VerificationReport`; compute and file-hash
      verifiers run real checks — verify: verifier unit/integration tests
- [x] 2. Read-only tools: `list_dir` and `code_search` with permission requests
      and bounded output — verify: dispatch and permission tests
- [ ] 3. Structured shell tool: program plus argument list, no interpolation,
      allowlist, permission request; command and test verifiers on top — verify:
      allowlist, timeout and permission tests
- [ ] 4. One coding eval case with `setup/`, `hidden/` and `verify.sh` — verify:
      the case rejects an unsolved workspace and accepts the reference fix
- [ ] 5. Real model end-to-end run — verify: user runs the OpenAI profile with a
      local `.env` and records the transcript

## Decisions

- Context is single-turn. There is no history stage and no reference to earlier
  turns. Under budget pressure, drop from the front and keep the latest
  fragments.
- The kernel produces `Evidence`; the model cannot set `verified`.
- Compute verification uses a small arithmetic parser, never `eval` or shell.
- File-hash paths are relative to the workspace and may not escape it.
- Command and test verification wait for the structured shell tool, so they
  cannot grow a second, interpolating execution path.
- Deferred items and their triggers are recorded in
  [backlog.md](../backlog.md).

## User points recorded

- Context assembly is single-turn; do not add history or "as mentioned
  earlier" behavior now.
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
- `cargo test --workspace` passes: 114 tests.
