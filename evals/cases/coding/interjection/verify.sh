#!/usr/bin/env bash
set -euo pipefail
workspace="${1:?usage: verify.sh WORKSPACE}"
if command -v python >/dev/null 2>&1; then python_bin=python; else python_bin=python3; fi
"$python_bin" - "$workspace" <<'PY'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
path = root / "injected.txt"
if not path.exists():
    raise SystemExit("injected.txt is missing; the interjection was not delivered")
text = path.read_text(encoding="utf-8").strip()
if text != "injected":
    raise SystemExit(f"injected.txt is {text!r}")
print("interjection artifact ok")
PY
