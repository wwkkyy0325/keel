# ADR 0002 — Two permission levels, internalized sandbox and soft delete

## Status

Accepted. Amended 2026-11-04: the deny list is split into a structural list
(the agent's state and operating-system areas, never lifted) and credential
names, which ask for confirmation inside a root and are refused outside every
root. See `docs/spec/01-kernel.md`.

## Context

The first permission draft had four levels (`read_only`, `manual`, `semi_auto`,
`full_auto`), a workspace-trust flag and a separate external-path flag. The
review showed that `read_only` and `manual` duplicate tool visibility and
host-level review, while the flags duplicate the level itself. The same review
made `full_auto` mean "complete control except the hard deny list" and moved
deletion to a recoverable operation.

## Decision

1. Permission levels are `WorkspaceAuto` and `FullAuto`.
2. `WorkspaceAuto` is the logical sandbox: reads and writes inside the
   workspace run automatically; outside actions require confirmation.
   `FullAuto` removes the workspace boundary; the hard deny list remains.
3. `read_only` is tool visibility, not a permission level. `manual` is a
   host-level confirmation wrapper, not a kernel level.
4. Plan mode is a tool visibility filter: it hides write/execute tools and
   never lowers permissions.
5. The agent never hard-deletes. A delete moves the target into a
   `TrashStore`; overwriting an existing file backs the previous version up
   first. A failed trash operation aborts the call; there is no hard-delete
   fallback. The agent never empties or restores the trash.
6. The hard deny list covers system/privileged paths, credential stores,
   secret files, other users' homes, privilege escalation, destructive storage
   commands, security bypass, Keel audit/`.git` internals, and
   send/payment/secret upload. It is hardcoded plus host additions; profiles
   and plugins cannot remove entries.
7. Confirmation and escalation are typed interactions. The user may allow one
   call, allow a scope, or escalate the session to `FullAuto`; only the user or
   host approves. Escalation never bypasses the deny list.
8. `[permission]` contains only `level`.

## Consequences

- Profiles use `workspace_auto`; the companion profile has no tools.
- Tools declare `permission_requests`; the dispatcher enforces them before
  calling the tool. A tool cannot grant itself access.
- M1 implements dispatcher enforcement, a workspace-local trash backend, and
  the baseline deny list. OS recycle-bin integration, symlink hardening,
  scoped grants and TTL/revocation arrive with M5.
- The `delete_file` tool and the OS trash backend arrive with the coding
  profile; `write_file` already uses the backup path.
