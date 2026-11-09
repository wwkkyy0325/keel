# ADR 0031 — Read paging and range coverage

## Status

Accepted and implemented in `ReadFileTool` and `ReadLedger`.

## Context

`read_file` returned up to 8 MiB in one call. A large source file could fill
the working set with a single read, and the whole text was persisted in the
session document. Paging existed (`offset`/`limit`) but did not say where the
range sat in the file, so a model could not plan the next call.

The read ledger also made a large file unwritable: `write_file` needs a full
read, a full read meant one uncapped call, and a capped file could therefore
never be rewritten even after reading all of it in ranges.

## Decision

1. A read without `offset` returns at most 1000 lines. The marker names the
   range and the next offset: `[showing lines 1-1000 of 4213; use offset=1001
   to continue]`.
2. A ranged read starts with a display-only header:
   `[file: 4213 lines; showing 1001-1200]`. When the file was cut at the byte
   limit the count reads `at least N`.
3. The header and the line numbers are not part of the file. The tool
   description says so, and edit text must not include them.
4. The ledger records 1-based inclusive line ranges per file. The ranges merge
   when they overlap or touch, and the file counts as fully read when the
   ranges cover every line.
5. The digest still has to match the bytes on disk. A read from changed bytes
   starts a new record with no ranges.
6. An edit drops the recorded ranges unless the file was already fully read,
   because an edit can move every later line.
7. A byte-truncated read never contributes to coverage.

## Consequences

- A large file can be read in several calls and then rewritten, and the
  `write_file` refusal names the remaining work.
- A capped default read is partial: it authorizes an edit but not a rewrite.
- The model has to page a long file, which costs a round trip but keeps the
  working set and the session document small.
- A file longer than 1000 lines can no longer be rewritten in one call unless
  the remaining ranges are read first.
