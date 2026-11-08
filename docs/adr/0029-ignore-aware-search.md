# ADR 0029 — Ignore-aware workspace search

## Status

Accepted and implemented in `code_search` and `glob` via
`ignore::WalkBuilder`.

## Context

The tools skipped a fixed list of build directories but walked every other
directory, including ones a project's `.gitignore` excludes (`data/`,
`generated/`, `.cache/`). On a real repository that means searching huge
generated trees and returning noise.

## Decision

1. `code_search` and `glob` walk with `ignore::WalkBuilder`.
2. The walker honours `.gitignore`, `.ignore`, `.git/info/exclude` and the
   global git excludes; `require_git(false)` makes `.gitignore` apply to a
   copy without `.git`.
3. Hidden files stay visible (`hidden(false)`) so `.github` and `.env.example`
   are still searchable.
4. The fixed build-directory list remains as a second filter; an explicitly
   chosen skipped directory (the walk root) is not pruned.
5. The walk is deterministic (file-name sort), depth-bounded, and runs on a
   blocking thread so a large tree does not stall the async turn.
6. Cancellation and the result/file limits still apply.

## Consequences

- A repository's own ignore rules now control what search and glob see.
- A file reachable only by an explicit path can still be read; the walk is
  what changes.
- The `ignore` dependency (MIT/Unlicense) is added; the license audit passes.
