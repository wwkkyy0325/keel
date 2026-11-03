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
main = (root / "main.txt").read_text(encoding="utf-8").strip()
if main != "main":
    raise SystemExit(f"main.txt is {main!r}")
bg = (root / "bg.txt").read_text(encoding="utf-8").strip()
if bg != "bg done":
    raise SystemExit(f"bg.txt is {bg!r}; the background command did not run")
print("background artifact ok")
PY
