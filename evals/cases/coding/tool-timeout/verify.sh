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
text = (root / "timeout.txt").read_text(encoding="utf-8").strip()
if "timed out" not in text.lower():
    raise SystemExit(f"timeout.txt does not record the timeout: {text!r}")
print("timeout artifact ok")
PY
