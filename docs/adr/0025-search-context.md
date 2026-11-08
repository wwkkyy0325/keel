# ADR 0025 — Search context lines

## Status

Accepted and implemented in `CodeSearchTool`.

## Context

`code_search` returned one line per match. A hit often needs the lines around
it to be useful (a function signature, a closing brace, a branch), so the
model issued a separate `read_file` for each hit. The tool already caps result
count, line length and output bytes.

## Decision

1. `context` is an optional integer, clamped to `0..=5`, default 0.
2. `context = 0` keeps the existing `path:line: text` output.
3. With context, each match prints up to N lines before and after:
   `path:line: text` for the match, `path:line- text` for a neighbor, and
   `--` between match blocks. Lines are trimmed and capped at the existing
   line limit.
4. Matches are ordered by path and numeric line.
5. The existing result, line-length and output-byte caps still apply; context
   does not bypass them.

## Consequences

- A line can appear as context for one match and as a match for another. That
  is accepted for simplicity; the `--` separator keeps blocks readable.
- The output cap can truncate a trailing block; the stopped marker still says
  how many matches were collected.
- The model can replace several `read_file` calls with one search, which
  lowers turn count and context use.
