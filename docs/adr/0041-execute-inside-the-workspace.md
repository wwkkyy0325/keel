# ADR 0041 — workspace_auto runs commands inside the workspace

## Status

Accepted and implemented in `keel-runtime`. Released as 0.2.2.

## Context

The kernel has two permission levels. `FullAuto` allows everything;
`WorkspaceAuto` allows reads and writes inside the workspace root and asks
about anything else. Execution was the exception: `permits` returned false
for `AccessKind::Execute` on every root, so the first `shell` call of every
session raised a confirmation even when its path was the workspace itself.

In practice that prompt is the only interruption a normal session sees, and
approving it escalates to `full_auto` for eight hours (the host's approval
scope), which is both more intrusive and more permissive than the level the
user chose. A host user who picks "workspace auto" expects the agent to run
its build and tests inside the workspace without a prompt.

## Decision

1. `Execute` follows the access of the root that contains the path: a write
   root (the workspace root and any write extra root) allows execution, a
   read-only root still confirms, and a path outside every root still
   confirms.
2. `FullAuto` is unchanged. The secret-path checks and the shell program
   allowlist are unchanged; they run before this decision.
3. The confirmation reason for a read-only root is the existing one
   (`` `{path}` is read-only in this root ``).
4. The base level is runtime-mutable: `PermissionEngine::set_base_level` and
   `TurnDriver::set_permission_level` switch it without reopening a session.
   The driver drops session grants with the switch, so a downgrade is real.

## Consequences

- `workspace_auto` no longer prompts for a command inside the workspace, so
  a coding session can run `cargo test`, `git status` and similar tools
  unattended.
- The boundary is the workspace root, not the program: a read-only extra
  root and anything outside the workspace still ask.
- A command can still modify anything the process can reach; the permission
  engine gates the command's own declared path, not its side effects. Hosts
  that need stronger isolation keep using a sandbox or `FullAuto` only
  deliberately.
