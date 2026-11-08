# ADR 0026 — Shell mutation hint

## Status

Accepted and implemented in the shell tool schema and the turn driver.

## Context

The driver marks a turn as workspace-changing only for the file tools. A shell
call is invisible because its targets are not in its arguments, so a formatter,
code generator, `git checkout` or `rm` run through shell skipped the
after-changes check. Classifying shell commands by program was rejected: any
read-only allowlist is incomplete (`python -c` and `npm run` can write).

## Decision

1. The shell tool accepts an optional boolean `mutates`, default false.
2. A successful shell call with `mutates: true` sets `workspace_changed` and
   reports no changed paths, so a selector falls back to every check.
3. The flag is a model hint, not a security control; permission is still the
   dispatcher's job. A wrong false skips a check, not an approval.
4. The tool's execution path ignores `mutates`.
5. The persona names the flag and the commands that need it.

## Consequences

- A cooperative model gets verification after a shell mutation without a
  second test run after every test-only command.
- A model that omits the hint keeps the old behavior: no verification after a
  shell-only turn.
- The flag cannot prove a mutation; it only closes the common case without
  parsing commands.
