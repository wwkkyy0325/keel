# ADR 0023 — `code_search` accepts a regex

## Status

Accepted and implemented in `keel-runtime::tools::CodeSearchTool`; `regex`
1.13 is a `keel-runtime` dependency.

## Context

`code_search` was a literal substring scan. A coding turn constantly needs a
pattern: a function signature, an import shape, a TODO marker with a prefix.
Every surveyed agent exposes regex content search: Claude Code's `Grep` is
ripgrep-based, OpenCode's `grep` uses ripgrep, Gemini CLI has `grep_search`,
Cline's `search_files` takes a regex. A literal-only tool pushes the model
back to `shell` and a platform-specific grep, which needs an execution
confirmation and may not be installed.

## Decision

1. **One optional flag.** `code_search { ..., regex?: boolean }`, default
   false. The literal path is unchanged.
2. **One flag for case.** `case_sensitive` applies in both modes. In regex
   mode it sets `case_insensitive(!case_sensitive)`, so the default stays
   case-insensitive, matching the literal default.
3. **Compile once.** The pattern is compiled once per call and reused for
   every line. An invalid pattern is `InvalidArguments` with the compiler
   message; the walk does not start.
4. **Line-oriented.** The result format is one line per match, so the pattern
   is matched against a single line. There is no multiline mode. Rust's
   `regex` crate does not support lookaround or backreferences; that is a
   documented limit, not a fallback to another engine.
5. **Same traversal and limits.** The file-size cap, depth cap, skip list,
   result cap, line truncation, `glob` filter and read permission are
   unchanged. The regex cannot reach a path the literal search could not.
6. **One dependency.** `regex` is MIT OR Apache-2.0, already covered by the
   `deny.toml` allowlist; `cargo deny check licenses` passes. A hand-written
   regex engine would be larger and less correct.

## Consequences

- A model can find a pattern without a shell confirmation and without
  knowing the platform's grep.
- Regex compilation costs a small fixed amount per call, not per line.
- A pathological pattern can still be expensive; the per-file size, per-line
  length and result caps bound the work, and the regex crate has its own
  size limit.
- Search remains line-oriented and `.gitignore`-unaware. A full ripgrep
  integration is still a separate decision.
- Tests pin a pattern match, the case flag in regex mode and an invalid
  pattern; the literal and glob tests are unchanged.
