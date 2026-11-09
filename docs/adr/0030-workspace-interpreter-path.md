# ADR 0030 — Run a workspace interpreter by path

## Status

Accepted and implemented in `AcceptanceVerifier`.

## Context

The verifier runs a check only when the program is on the coding allowlist,
and `is_allowed` rejects any program that contains a path separator. A
project venv's interpreter is always a path (`.venv/bin/python`,
`.venv/Scripts/python.exe`), so a host that preferred the project interpreter
produced a check that could never pass: every turn in a Python project with a
venv failed its required check with `program ... is not allowlisted`, no matter
what the code did.

The allowlist exists to stop a check from naming an arbitrary program. A path
inside the workspace does not cross that boundary, because the workspace
already defines the check and owns the file.

## Decision

1. A bare program name is still checked against the allowlist.
2. A check may also name an absolute program path when all of these hold:
   - the path has no `..` component,
   - the path starts with the verifier's workspace root,
   - the path is a file,
   - the file name alone is allowlisted (`python.exe` matches `python`).
3. Relative program paths are not accepted; the host passes absolute paths.
4. The shell tool's allowlist is unchanged: a model-initiated shell command
   still cannot name a path.

## Consequences

- A project venv now runs `python -m pytest` or `python -m unittest` with the
  interpreter its dependencies live in.
- The containment check is lexical. A symlinked interpreter inside the
  workspace is allowed, and a link that points outside still runs, because the
  file name has to be allowlisted. The workspace is already trusted to define
  its own check, the same way a `package.json` test script is trusted.
- A workspace that ships a file named like an allowlisted program can have it
  run by a check. That is the project's own program, not one the model chose.
- The error message for a rejected path still says `not allowlisted`.
