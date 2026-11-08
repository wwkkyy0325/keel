# ADR 0021 — A glob tool for file discovery

## Status

Accepted and implemented in `keel-runtime::tools::GlobTool`.

## Context

`code_search` finds content but cannot answer "which files are there?".
`list_dir` is one directory deep, and its `max_entries` is an enumeration
budget, not a pattern. Every coding agent in the survey exposes a file-glob
primitive next to content search: Claude Code has `Glob`, OpenCode has
`glob`, Gemini CLI has `glob`, Cline has `list_files`.

Without it a model either guesses paths or spends a turn listing directories.
Both are unreliable in a repository with a nested layout.

## Decision

1. **Tool shape.** `glob { pattern, path?, limit? }`. `pattern` is required;
   `path` is the directory root (default `.`); `limit` defaults to 100 and is
   capped at 500.
2. **Pattern semantics.** `*` matches within one path segment, `**` matches
   zero or more segments, and `?` matches one character. A pattern with no
   separator matches the file name at any depth, so `*.rs` behaves like a
   recursive find. A pattern with a separator matches the path relative to
   the root.
3. **Files only.** Directories are traversed, not returned. Symlinks are
   skipped, as in content search.
4. **Bounded traversal.** The existing skip list and depth limit apply
   (`.git`, `target`, `node_modules`, build and cache directories). The walk
   collects at most 1000 candidates before sorting.
5. **Newest first.** Results are sorted by modification time descending, then
   by path, matching the convention of the surveyed tools. The output is one
   relative path per line; a truncated result says how many were shown.
6. **Read permission.** The permission request is a read on the search root,
   the same as `code_search`.

## Consequences

- `glob` and `code_search` form one discovery pair: the model locates files
  by path, then reads or searches their content.
- The output has no size or mtime column; the model can stat a chosen file
  through the shell if it needs one. Keeping the line a plain path makes it
  directly usable as the `path` of `read_file`.
- This is not a gitignore implementation. The skip list is conservative and
  explicit; adding `.gitignore` parsing is a separate decision.
- The matcher is byte-oriented, so `?` matches one byte rather than one
  Unicode scalar. File names are otherwise compared as they are on disk.
