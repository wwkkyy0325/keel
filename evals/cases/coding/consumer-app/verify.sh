#!/usr/bin/env bash
set -euo pipefail

workspace="${1:?usage: verify.sh WORKSPACE}"
crate="$workspace/keel-notes"

if [[ ! -f "$crate/Cargo.toml" ]]; then
  echo "missing keel-notes/Cargo.toml"
  exit 1
fi

cd "$crate"
cargo test --quiet

output="$(cargo run --quiet -- ask "hello kernel")"
echo "$output"

grep -q '^state: Done$' <<<"$output"
grep -q '^answer: mock turn complete: echoed ' <<<"$output"
