#!/usr/bin/env bash
set -euo pipefail

workspace="${1:?usage: verify.sh WORKSPACE}"
case_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if command -v python >/dev/null 2>&1; then
  python_bin=python
else
  python_bin=python3
fi

cd "$workspace"
PYTHONPATH="$workspace:$case_dir/hidden" "$python_bin" -m unittest discover \
  -s "$case_dir/hidden" -p 'test_*.py' -v
