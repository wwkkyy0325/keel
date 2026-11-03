#!/usr/bin/env bash
set -euo pipefail
workspace="${1:?usage: verify.sh WORKSPACE}"
if command -v python >/dev/null 2>&1; then python_bin=python; else python_bin=python3; fi
"$python_bin" - "$workspace" <<'PY'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
text = (root / "done.txt").read_text(encoding="utf-8").strip()
if text != "ok":
    raise SystemExit(f"done.txt is {text!r}")
print("budget artifact ok")
PY
