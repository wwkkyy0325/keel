# ADR 0028 — Cross-file atomic edits

## Status

Accepted and implemented in `ApplyEditsTool`.

## Context

`multi_edit` validates several replacements to one file and writes once. A
rename or a small refactor often spans files; applying edits one file at a
time can leave a partially refactored workspace when a later file does not
match its snippet.

## Decision

1. `apply_edits` takes parallel `paths`, `old` and `new` lists. Edit i
   replaces `old[i]` with `new[i]` in `paths[i]`; the same path may repeat.
2. Every file is read once, the read ledger is checked once per file, and
   every replacement is applied to in-memory copies before any backup or
   write.
3. If any replacement is empty, identical, missing or ambiguous, the call
   fails and no file is written.
4. Changed files are backed up and written one by one. A write failure
   restores the previously written files from their backups, best effort.
5. The ledger advances for every written file; line-ending normalization uses
   the same fallback as `edit_file`.
6. The driver records every path in `paths` as a changed path, so the check
   selector sees the whole refactor.

## Consequences

- A cross-file refactor fails before touching the workspace when one snippet
  is wrong, which is the common failure.
- Filesystem writes are not transactional; a crash mid-write can leave a
  partial set, but the trash holds every previous version.
- The lists must stay aligned; a mismatch is an argument error.
- The tool is registered by the host and exposed by the coding profiles; the
  kernel does not require it.
