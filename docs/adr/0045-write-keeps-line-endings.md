# ADR 0045 — A rewrite keeps the file's line endings

## Status

Accepted and implemented in `keel-runtime`. Released as 0.2.3.

## Context

`edit_file` and `multi_edit` already write in the file's line-ending style:
`apply_edit` reads the existing content, picks CRLF when it contains a CRLF
pair, and converts the replacement through `with_line_ending`. `write_file`
did not. It wrote the model's content verbatim, and a model emits LF.

On Windows that turned a small rewrite into a whole-file diff. A live turn
changed six lines of a CRLF source file; the checkpoint reported +142/−139,
and the review card showed the file as rewritten. The content was correct;
the line endings were not.

## Decision

1. Before writing, `write_file` reads the existing file. When the file
   contains at least one CRLF pair, the content is written through
   `with_line_ending(content, "\r\n")`, the same helper the edit tools use.
2. An LF file is written as given; a new file is written as given. Content
   that already carries CRLF is normalized to a single CRLF per newline, not
   doubled.
3. The read ledger records the bytes actually written, so a later edit of
   the same file is checked against what is on disk.

## Consequences

- A rewrite of a CRLF file produces a diff of the lines that changed, which
  is what the checkpoint card and a review are meant to show.
- A file with intentionally mixed endings is normalized to CRLF on a rewrite;
  the alternative — preserving a mixed file byte-for-byte — would require
  matching every newline position and is not a case a model can express.
- The read is best effort: if the file cannot be read, the content is written
  as given and the write fails on its own if the path is unwritable.
