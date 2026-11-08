# ADR 0019 — Workspace reads are bounded and filterable

## Status

Accepted and implemented in `keel-runtime::tools::{ReadFileTool, CodeSearchTool}`.

## Context

The first live coding turn exposed two limits that `write_file` did not fix:

- `read_file` returned a whole file or its first 8 MiB. A model working in a
  large file could not ask for the region around a symbol, and a truncated
  file had no way to reach its tail.
- `code_search` was an unconditional case-insensitive substring scan. It could
  not be case-sensitive and could not be narrowed to one file type, so a
  query in a large repository spent its result budget on unrelated files.

Both are host-independent read paths. The fix belongs in the kernel tool
contract, not in a host workaround.

## Decision

1. **`read_file` gains `offset` and `limit` in lines.** Both are optional and
   1-based; `limit` counts lines. Without either, the tool keeps its existing
   whole-file behavior.
2. **A ranged read returns the exact bytes of the range.** Line endings are
   preserved (CRLF included) and no line numbers or headers are added, so the
   returned text can be passed to `edit_file` unchanged.
3. **An offset past the end is a successful, explicit answer.** The result
   says how many lines the file has. An offset or limit below 1 is
   `InvalidArguments`.
4. **The 8 MiB read cap still applies first.** A range is taken from the
   readable prefix; the truncation marker still names the full file size.
5. **`code_search` gains `case_sensitive` and `glob`.** `case_sensitive` is a
   boolean, default false. `glob` is a literal pattern with `*` and `?`.
6. **A glob without a separator matches the file name; one with a separator
   matches the workspace-relative path.** `*` may cross separators. This is a
   filter, not a full gitignore or ripgrep implementation; the tool stays a
   literal substring search with no new dependency.
7. **The skip list stays conservative.** Build and cache directories are
   skipped by name: `.git`, `target`, `node_modules`, `dist`, `build`, `out`,
   `coverage`, `vendor`, the common virtualenv and tool caches, and the editor
   folders.

## Consequences

- A model can page through a large file and copy an exact snippet into an
  edit. It must track its own position, because the result carries no header;
  that price buys byte-exact output for `edit_file`.
- Search remains literal. Regex, whole-word and gitignore semantics are still
  out of scope; if a real repository needs them, that is a separate decision
  with a dependency and a performance budget.
- The glob matcher is byte-oriented, so `?` matches one byte rather than one
  Unicode scalar. File names are otherwise compared as they are on disk.
- Contract tests cover the line range, CRLF preservation, offset past the
  end, a zero offset, case-sensitive search and a glob filter.
