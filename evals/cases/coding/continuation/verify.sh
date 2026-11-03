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
a = (root / "a.txt").read_text(encoding="utf-8").strip()
b = (root / "b.txt").read_text(encoding="utf-8").strip()
if a != "A" or b != "B":
    raise SystemExit(f"a.txt={a!r} b.txt={b!r}")
print("continuation artifacts ok")
PY
