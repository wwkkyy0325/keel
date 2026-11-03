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
notes = (root / "notes.txt").read_text(encoding="utf-8").strip()
alpha_path = root / "alpha.txt"
if not alpha_path.exists():
    raise SystemExit("alpha.txt is missing; the sub-agent did not write it")

alpha = alpha_path.read_text(encoding="utf-8").strip()
if alpha != notes.upper():
    raise SystemExit(f"alpha.txt is {alpha!r}, expected {notes.upper()!r}")
print(f"sub-agent artifact ok: {alpha!r}")
PY
