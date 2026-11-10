# ADR 0047 — A host can tell the setting from the grant

## Status

Accepted and implemented in `keel-runtime`. Released as 0.2.6.

## Context

`PermissionEngine::level` answers with the level in force, including an
active full-auto session grant. That is the right answer for a permission
check, and the wrong one for a settings display.

A live session showed the cost. The user approved a permission card option
that grants `FullAutoSession` for eight hours; the composer's chip then read
"full auto" because it displayed the effective level. The persisted setting
was still `workspace_auto`. After a restart the grant was gone and the chip
read "workspace auto", which looked like a lost setting. Worse, choosing
"full auto" while the chip already showed it was a no-op, so the user could
not persist what the chip claimed.

## Decision

1. `PermissionEngine` gains `base_level()`, the configured level without a
   session grant. `level()` keeps its current meaning and now calls it.
2. `ToolDispatcher::base_permission_level` and
   `TurnDriver::base_permission_level` forward it, so a host does not need
   to keep its own copy of the setting.
3. No behavior changes: permission checks still use `level()`.

## Consequences

- A host can show the persisted setting in its menu and the effective level
  beside it; "full auto (temporary)" and "full auto" stop looking the same.
- A host can distinguish "the user chose this" from "a grant raised it", and
  can offer a click that persists the choice.
