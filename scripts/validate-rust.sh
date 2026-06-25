#!/usr/bin/env bash
set -euo pipefail

validate_logs=()

cleanup_validate_logs() {
  if ((${#validate_logs[@]})); then
    rm -f "${validate_logs[@]}"
  fi
}

trap cleanup_validate_logs EXIT

print_command() {
  local arg
  local first=1
  for arg in "$@"; do
    if [[ "$first" -eq 1 ]]; then
      first=0
    else
      printf ' '
    fi
    printf '%q' "$arg"
  done
}

run_broad_step() {
  local name="$1"
  shift
  local log

  log="$(mktemp "${TMPDIR:-/tmp}/pevo-validate-${name}.XXXXXX.log")"
  validate_logs+=("$log")

  printf 'validate rust broad: %s ... ' "$name"
  if "$@" >"$log" 2>&1; then
    printf 'ok\n'
    return 0
  else
    local status=$?
    printf 'failed\n'
    {
      printf 'command: '
      print_command "$@"
      printf '\nexit: %s\n' "$status"
      printf 'output:\n'
      cat "$log"
    } >&2
    return "$status"
  fi
}

mode="${1:-broad}"
if [[ "$mode" == "broad" ]]; then
  run_broad_step fmt cargo fmt --all --check
  run_broad_step clippy cargo clippy --workspace --all-targets -- -D warnings
  run_broad_step test cargo test --workspace --all-targets
elif [[ "$mode" == "narrow" ]]; then
  shift
  cargo test "$@"
else
  cargo test "$@"
fi
