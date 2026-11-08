# ADR 0020 — Read before write

## Status

Accepted and implemented in `keel-runtime::read_ledger::ReadLedger` and the
`ReadFileTool`, `WriteFileTool` and `EditFileTool` constructors.

## Context

`write_file` replaced a whole file without proof that the model had seen it.
A model could overwrite a file it never read, or write over bytes that changed
after its read (an editor, a formatter, another tool). `edit_file` is safer
because it matches text, but a stale read can still make the intended snippet
ambiguous or apply the change to a different copy.

The established coding agents converged on the same guard: Claude Code
requires a prior Read before Edit/Write; Qwen Code rejects an edit when the
file changed since the session last read or wrote it; ironclaw requires a
full read and a content fingerprint. The failure is recoverable, so the
refusal must tell the model to read the file again.

## Decision

1. **One ledger per engine.** `ReadLedger` is a shared `Arc` of resolved path
   to SHA-256. The host creates one per engine and gives it to the read, write
   and edit tools. The kernel `new()` constructors keep working without one;
   the host is what turns the guard on.
2. **Reads record.** A successful `read_file` records the raw UTF-8 bytes it
   returned. A lossy decode of a truncated file is not recorded, because the
   model did not see the bytes on disk.
3. **Writes check.** `write_file` on an existing file requires a recorded
   path whose current bytes still match. A path that was never read, or whose
   content changed, is refused with `InvalidArguments` (retryable) and a
   message that says "read the file before writing it" or "the file changed
   since it was read; read it again".
4. **Edits check.** `edit_file` performs the same check against the bytes it
   just read, before matching or backing up.
5. **New files are allowed.** `write_file` to a path that does not exist does
   not need a read.
6. **The ledger advances.** A successful write or edit records the bytes it
   wrote, so a second edit in the same turn does not need another read.
7. **Best effort.** The check is content-based, not mtime-based, and is
   vulnerable to a change between the check and the write. That race is
   accepted; the trash still holds the previous version.

## Consequences

- A model that has not read a file gets one actionable error and must read
  first. The live coding eval shows this is a single extra round at most.
- The ledger is in memory per engine. Switching conversations creates a new
  engine and a new ledger; the model must read again, which is correct for a
  new session.
- A file changed by `shell` (a formatter, a code generator) is stale:
  the model must re-read it. That is deliberate.
- The hash covers at most the read limit (8 MiB). A change beyond that
  prefix is not detected; the write limit has the same bound.
- Contract tests cover an unread file, a stale file, a read-then-write, a
  new file and an unread edit.

## Amendment (2026-11-08)

Rule 2 recorded the whole file even when `read_file` returned only a line
range, so a model could read one line and then overwrite the file with
`write_file`, discarding content it never saw. The ledger entry now carries a
coverage flag:

- a plain `read_file`, and a successful `write_file`, record `full = true`;
- a ranged or truncated read records `full = false`;
- a later partial read of the same bytes does not clear an earlier full read;
- an edit (`edit_file`, `multi_edit`) keeps the flag of the read it checked.

`write_file` on an existing file uses `check_full` and refuses a partially
seen file with "read the whole file before overwriting it; a line range is
not enough". `edit_file` and `multi_edit` still accept any current read; their
exact-match requirement protects the bytes they do not replace.
