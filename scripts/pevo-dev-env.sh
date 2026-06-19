#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dev_home="${PSYCHEVO_DEV_HOME:-$repo_root/.local/.psychevo-dev}"
pevo_bin="${PEVO_BIN:-$repo_root/target/debug/pevo}"
cmd="${1:-}"

usage() {
  cat >&2 <<EOF
usage: $0 init|live

Environment:
  PSYCHEVO_DEV_HOME       Isolated live-validation home. Default: $repo_root/.local/.psychevo-dev
  PSYCHEVO_LIVE_PROVIDERS Whitespace-separated live providers. Default: xiaomi-token-plan
EOF
}

build_pevo() {
  if [[ -z "${PEVO_BIN:-}" ]]; then
    cargo build -p psychevo-cli
  elif [[ ! -x "$pevo_bin" ]]; then
    printf 'PEVO_BIN is not executable: %s\n' "$pevo_bin" >&2
    exit 2
  fi
}

init_dev_home() {
  build_pevo
  PSYCHEVO_HOME="$dev_home" "$pevo_bin" init
  cat <<EOF

Dev home is ready at:
  $dev_home

Prepare live credentials manually before running live validation:
  $dev_home/config.toml
  $dev_home/.env
EOF
}

require_dev_home() {
  if [[ ! -f "$dev_home/config.toml" || ! -f "$dev_home/.env" ]]; then
    printf 'dev home is not initialized: %s\nrun: %s init\n' "$dev_home" "$0" >&2
    exit 2
  fi
}

run_provider() {
  local provider="$1"
  local stamp="$2"
  local workdir="$dev_home/live-work/$provider-$stamp"
  local token="PEVO_LIVE_${provider}_${stamp}"
  local first_log="$dev_home/logs/live-$stamp-$provider-1.ndjson"
  local second_log="$dev_home/logs/live-$stamp-$provider-2.ndjson"
  local model_spec

  mkdir -p "$workdir" "$dev_home/logs"
  printf 'probe token: %s\n' "$token" > "$workdir/pevo_live_probe.txt"
  model_spec="$(provider_model "$provider")"

  PSYCHEVO_HOME="$dev_home" \
  PSYCHEVO_INFERENCE_PROVIDER="$provider" \
  "$pevo_bin" run \
    --dir "$workdir" \
    --format json \
    --include-reasoning \
    -m "$model_spec" \
    "There is a file named pevo_live_probe.txt in this workspace. Inspect the workspace and report the probe token it contains." \
    > "$first_log"

  PSYCHEVO_HOME="$dev_home" \
  PSYCHEVO_INFERENCE_PROVIDER="$provider" \
  "$pevo_bin" run \
    --dir "$workdir" \
    --format json \
    --include-reasoning \
    -m "$model_spec" \
    --continue \
    "Continue the same session and report the same probe token again." \
    > "$second_log"

  python3 "$repo_root/scripts/pevo-dev-env-verify.py" "$provider" "$token" "$first_log" "$second_log"
}

provider_model() {
  case "$1" in
    xiaomi-token-plan)
      printf '%s\n' 'xiaomi-token-plan/mimo-v2.5-pro'
      ;;
    deepseek)
      printf '%s\n' 'deepseek/deepseek-chat'
      ;;
    *)
      printf 'unsupported live provider: %s\n' "$1" >&2
      exit 2
      ;;
  esac
}

run_live() {
  build_pevo
  require_dev_home
  local stamp
  local providers
  stamp="$(date +%Y%m%d%H%M%S)"
  providers="${PSYCHEVO_LIVE_PROVIDERS:-xiaomi-token-plan}"
  for provider in $providers; do
    run_provider "$provider" "$stamp"
  done
}

case "$cmd" in
  init)
    init_dev_home
    ;;
  live)
    run_live
    ;;
  *)
    usage
    exit 2
    ;;
esac
