#!/usr/bin/env bash
set -euo pipefail

prod_limit="${PROD_LIMIT:-900}"
test_limit="${TEST_LIMIT:-1200}"
generated_limit="${GENERATED_LIMIT:-900}"
roots=("$@")
if [[ ${#roots[@]} -eq 0 ]]; then
  roots=(apps crates packages specs)
fi

is_generated() {
  local path="$1"
  [[ "$path" == *"/generated/"* ]] && return 0
  [[ "$path" == packages/protocol/schema/*.json ]] && return 0
  return 1
}

is_test() {
  local path="$1"
  [[ "$path" == *"/tests/"* ]] && return 0
  [[ "$path" == *"/e2e/"* ]] && return 0
  [[ "$path" == *.test.* ]] && return 0
  [[ "$path" == *.spec.* && "$path" != specs/* ]] && return 0
  return 1
}

category_for() {
  local path="$1"
  if is_generated "$path"; then
    printf 'generated'
  elif is_test "$path"; then
    printf 'test'
  else
    printf 'production'
  fi
}

limit_for() {
  local category="$1"
  case "$category" in
    generated) printf '%s' "$generated_limit" ;;
    test) printf '%s' "$test_limit" ;;
    *) printf '%s' "$prod_limit" ;;
  esac
}

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

rg --files "${roots[@]}" \
  -g '!**/target/**' \
  -g '!**/dist/**' \
  -g '!**/node_modules/**' \
  -g '!**/coverage/**' \
  -g '!**/test-results/**' \
  -g '!**/.local/**' |
while IFS= read -r path; do
  lines="$(wc -l < "$path")"
  category="$(category_for "$path")"
  limit="$(limit_for "$category")"
  if (( lines > limit )); then
    printf '%s\t%s\t%s\t%s\n' "$category" "$lines" "$limit" "$path"
  fi
done > "$tmp"

if [[ ! -s "$tmp" ]]; then
  printf 'large-file inventory: ok (production<=%s test<=%s generated<=%s)\n' \
    "$prod_limit" "$test_limit" "$generated_limit"
  exit 0
fi

printf 'large-file inventory: oversized files (category lines limit path)\n'
sort -k1,1 -k2,2nr "$tmp" |
while IFS=$'\t' read -r category lines limit path; do
  printf '%-10s %6s > %-6s %s\n' "$category" "$lines" "$limit" "$path"
done
exit 1
