# ADR 0022 — `multi_edit` applies one file's changes atomically

## Status

Accepted and implemented in `keel-runtime::tools::MultiEditTool`.

## Context

A normal change often needs several related replacements in one file: a
signature, its call site, an import and a test. `edit_file` handles one
replacement per call. If the third call fails, the first two are already on
disk; the model must notice the partial state and repair it. Each call also
takes its own trash backup and write.

Claude Code exposes `MultiEdit` for this case; OpenCode and Codex go further
with a multi-file `apply_patch`. The first step is per-file atomicity, which
covers the common case without a patch grammar.

## Decision

1. **Tool shape.** `multi_edit { path, old: string_list, new: string_list,
   replace_all? }`. `old` and `new` are parallel lists and must have the same
   non-zero length.
2. **Validate everything first.** Each pair is applied in order to an
   in-memory copy. A missing or ambiguous snippet fails the whole call; no
   backup is taken and no byte is written.
3. **Edits see earlier edits.** Pair N applies to the text produced by pairs
   1..N-1. This is intentional: a later replacement can target the result of
   an earlier one. It also means two pairs that overlap can fail; the error
   names the edit index.
4. **Same matching contract as `edit_file`.** Literal match, exact first,
   line-ending-normalized fallback, `old`/`new` must be non-empty and
   different. The read ledger (ADR 0020) is checked once before any edit.
5. **One backup, one write.** The previous version goes to the trash once;
   the new bytes are written once and recorded in the ledger.
6. **`replace_all` is call-wide.** It applies to every pair. A per-pair flag
   would make the argument shape harder to produce reliably; a caller that
   needs a mix can split the call.
7. **No multi-file transaction.** The tool edits one path. A file list or a
   patch grammar is a separate decision; cross-file atomicity needs a
   different backup/rollback design.

## Consequences

- One logical change is one audited tool call, one backup and one write.
- A failed batch leaves the file exactly as it was, which is the property the
  trash cannot provide on its own.
- The output is one tool line; the caller does not see per-pair results. The
  error message names the failing pair, which is enough to retry.
- A large batch still holds the whole file in memory (8 MiB edit limit).
- Contract tests cover the success path, a missing edit with no write and no
  backup, unequal lists, the read guard, the line-ending fallback and
  `replace_all`.
