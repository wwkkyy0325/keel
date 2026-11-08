# ADR 0018 — `edit_file` is an exact-match search-replace tool

## Status

Accepted and implemented in `keel-runtime::tools::EditFileTool`.

## Context

`write_file` replaces a whole file. A coding turn that changes one line would
have to re-emit the entire file, which is expensive and makes an unrelated
mistake likely. The missing primitive is a targeted edit.

A fuzzy or regex edit hides the decision from the caller: a wrong pattern can
change more than intended, and a failure is hard to predict. The kernel should
guarantee that an edit either matches exactly what was asked or does not
write.

## Decision

1. **Tool shape.** `edit_file { path, old, new, replace_all? }`. The match is
   literal text, not a pattern.
2. **Exactly one by default.** With `replace_all` unset, `old` must occur
   exactly once. Zero matches return `NotFound`; multiple matches return
   `InvalidArguments` with the count. Neither writes.
3. **`replace_all` is explicit.** It is the only way to change more than one
   occurrence, and the result reports the number of replacements.
4. **No-op and empty input are refused.** `old` must be non-empty and must
   differ from `new`.
5. **Existing UTF-8 regular files only.** The path must resolve inside the
   workspace, exist, be a regular file, and be at most the read limit (8 MiB).
   The tool does not create files; `write_file` does.
6. **Write permission and backup.** It requests `AccessKind::Write` on the
   path and copies the previous version into the trash before writing. A
   failed backup aborts the edit. The success message names the trash entry.
7. **No diff in the result.** The model already supplied `old` and `new`; the
   audit has both. A structured diff belongs to the host/UI, not to a second
   tool result format.

## Consequences

- A model that supplies too little context gets an ambiguity error and must
  retry with a longer snippet. That is deliberate: silence would be worse.
- The trash grows by one version per edit; the existing trash tools prune and
  restore it.
- The tool cannot rename files, change permissions or create a file. Those
  remain separate operations.
- Contract tests pin the single-match, ambiguous, `replace_all`, missing-file
  and empty-snippet cases.

## Amendment — line-ending fallback (2026-11-08)

Exact matching is still the primary path. A Windows checkout is commonly CRLF
while a model's JSON snippet uses LF, so an exact-only tool fails on every
such edit even though the text is correct.

When the exact snippet is absent, `edit_file` converts `old` to the file's
line ending (CRLF when the file contains CRLF, otherwise LF) and tries again.
`new` is converted to the same style, so the edit cannot turn a CRLF file into
a mixed file. The fallback is still literal: zero matches fail, and more than
one match is still ambiguous. The success message says the line endings were
normalized. Read-before-write (ADR 0020) applies before either path.
