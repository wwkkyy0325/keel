# Build a table of contents outside code fences

`docs.toc(markdown)` treats every line starting with `#` as a heading, including lines inside fenced code blocks and tags such as `#hashtag`. Return `(level, title)` for headings of level 1-3 that have a space after the markers; a heading whose title is empty after the markers is ignored. Skip fenced blocks.

Do not change the tests. Run the test suite to confirm the fix.
