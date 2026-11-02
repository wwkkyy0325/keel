#!/usr/bin/env bash
set -euo pipefail

workspace="${1:?usage: verify.sh WORKSPACE}"

if command -v python >/dev/null 2>&1; then
  python_bin=python
else
  python_bin=python3
fi

"$python_bin" - "$workspace" <<'PY'
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
chapters = sorted((root / "chapters").glob("chapter-*.md"))

if len(chapters) != 6:
    raise SystemExit(f"expected 6 chapters, found {len(chapters)}")

total = 0
for path in chapters:
    text = path.read_text(encoding="utf-8").strip()
    if len(text) < 500:
        raise SystemExit(f"{path.name} is too short: {len(text)} characters")
    total += len(text)

outline = (root / "outline.md").read_text(encoding="utf-8").strip()
if len(outline) < 100:
    raise SystemExit(f"outline.md is too short: {len(outline)} characters")

characters = (root / "characters.md").read_text(encoding="utf-8").strip()
if len(characters) < 100:
    raise SystemExit(f"characters.md is too short: {len(characters)} characters")

print(f"novel ok: {len(chapters)} chapters, {total} characters")
PY
