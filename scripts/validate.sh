#!/usr/bin/env bash
set -euo pipefail

mode="${1:-broad}"
if [[ "$mode" == "broad" ]]; then
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace --all-targets
elif [[ "$mode" == "narrow" ]]; then
  shift
  cargo test "$@"
else
  cargo test "$@"
fi
