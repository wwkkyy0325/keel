# ADR 0024 — Acceptance selected by changed paths

## Status

Accepted and implemented in `TurnDriver::with_acceptance_selector` and
`AcceptanceSelector`.

## Context

A host could supply a fixed acceptance list, or a list that runs after any
workspace change. A polyglot workspace needs the checks of the project the
turn actually touched. Running every check lets an unrelated failure block the
turn; running only the first misses the others. The driver already knew that
the workspace changed, but not which paths changed.

## Decision

1. A successful mutating tool call records the paths it touched
   (`TurnStats::changed_paths`): `path` for the file tools, `from` and `to`
   for `move_file`. A shell call is not listed because its targets are not
   visible to the driver.
2. `with_acceptance_selector` takes an `AcceptanceSelector`, a sendable
   closure `Fn(&[PathBuf]) -> Vec<AcceptanceItem>`.
3. The selector runs once, after the model stops, with the paths in the order
   the calls succeeded. An empty result finishes the turn with a passing,
   evidence-free report.
4. The host owns selection. The kernel only reports paths; it does not know
   which ecosystem a check belongs to.
5. `multi_edit` is a workspace-mutating tool. It was missing from
   `WORKSPACE_MUTATING_TOOLS`; a turn that only used it skipped the
   after-changes check. It is now included.

## Consequences

- The host can pick the deepest project for each path and fall back to every
  check when a path belongs to none (a shell-only change, a root file).
- A selector that receives no paths must decide its own fallback; the host's
  current choice is to run every detected check.
- The selector is not called for a conversational turn, and a fixed
  acceptance list behaves exactly as before.
- A shell-only mutation still does not set `workspace_changed`, so it skips
  verification. That rule is unchanged; shell targets are not observable.
