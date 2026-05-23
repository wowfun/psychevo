#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dev_home="${PSYCHEVO_DEV_HOME:-$repo_root/.local/.psychevo-dev}"
pevo_bin="${PEVO_BIN:-$repo_root/target/debug/pevo}"
cmd="${1:-}"

usage() {
  printf 'usage: %s init|live\n' "$0" >&2
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

  mkdir -p "$workdir" "$dev_home/logs"
  printf 'probe token: %s\n' "$token" > "$workdir/pevo_live_probe.txt"

  PSYCHEVO_HOME="$dev_home" \
  PSYCHEVO_INFERENCE_PROVIDER="$provider" \
  "$pevo_bin" run \
    --dir "$workdir" \
    --format json \
    --include-reasoning \
    "There is a file named pevo_live_probe.txt in this workspace. Inspect the workspace and report the probe token it contains." \
    > "$first_log"

  PSYCHEVO_HOME="$dev_home" \
  PSYCHEVO_INFERENCE_PROVIDER="$provider" \
  "$pevo_bin" run \
    --dir "$workdir" \
    --format json \
    --include-reasoning \
    --continue \
    "Continue the same session and report the same probe token again." \
    > "$second_log"

  python3 - "$provider" "$token" "$first_log" "$second_log" <<'PY'
import json
import sys
from pathlib import Path

provider, token, first_path, second_path = sys.argv[1:]

def load(path):
    rows = []
    for raw in Path(path).read_text(encoding="utf-8").splitlines():
        if raw.strip():
            rows.append(json.loads(raw))
    return rows

def final_text(events):
    text = ""
    for event in events:
        if event.get("type") != "message_end":
            continue
        message = event.get("message") or {}
        if message.get("role") != "assistant":
            continue
        parts = []
        for block in message.get("content") or []:
            if block.get("type") == "text":
                parts.append(block.get("text") or "")
        if parts:
            text = "\n".join(parts)
    return text

first = load(first_path)
second = load(second_path)
combined = first + second

if not any(event.get("type") == "reasoning_delta" and event.get("text") for event in combined):
    raise SystemExit(f"{provider}: missing reasoning_delta")
if not any(event.get("type") == "reasoning_end" and event.get("text") for event in combined):
    raise SystemExit(f"{provider}: missing reasoning_end")
if not any(
    event.get("type") == "tool_execution_start" and event.get("tool_name") == "read"
    for event in first
):
    raise SystemExit(f"{provider}: first run did not call read")
if not any(
    event.get("type") == "tool_execution_end"
    and event.get("tool_name") == "read"
    and event.get("outcome") == "normal"
    for event in first
):
    raise SystemExit(f"{provider}: first run did not complete read")
first_session = next((event.get("session_id") for event in first if event.get("type") == "run_start"), None)
second_session = next((event.get("session_id") for event in second if event.get("type") == "run_start"), None)
if not first_session or first_session != second_session:
    raise SystemExit(f"{provider}: --continue did not reuse the session")
if token not in final_text(first):
    raise SystemExit(f"{provider}: first final answer did not contain token {token}")
if token not in final_text(second):
    raise SystemExit(f"{provider}: continue final answer did not contain token {token}")

print(f"{provider}: ok ({first_path}, {second_path})")
PY
}

run_live() {
  build_pevo
  require_dev_home
  local stamp
  stamp="$(date +%Y%m%d%H%M%S)"
  run_provider deepseek "$stamp"
  run_provider xiaomi "$stamp"
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
