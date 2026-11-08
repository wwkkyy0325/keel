# ADR 0027 — Read file line numbers

## Status

Accepted and implemented in `ReadFileTool`.

## Context

`read_file` returned raw bytes. The model could ask for a line range but could
not see absolute line numbers, and `code_search` returned numbers in a
different shape. A numbered read helps it navigate a large file and describe a
location without guessing.

## Decision

1. `read_file` accepts `line_numbers` (boolean, default false).
2. When true, each returned line is prefixed with its absolute number in a
   right-aligned five-column field and ` | `.
3. The ledger still records the raw bytes; the prefix is display only.
4. The truncation marker is appended after numbering and is not numbered.
5. The default output is unchanged; `edit_file` and `multi_edit` still match
   raw text.

## Consequences

- A model that copies a numbered line into `old`/`new` will fail to match; the
  tool description and persona say the numbers are not part of the file.
- A ranged read reports absolute numbers, not offsets from one.
- Output is slightly larger when the option is on.
