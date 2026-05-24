#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
capture_assets="$repo_root/scripts/tui-capture"
mock_provider_script="$capture_assets/mock_provider.py"
tape_template="$capture_assets/pevo-tui-demo.tape.tpl"
fixture_workdir="$capture_assets/fixtures/workdir"
fixture_home="$capture_assets/fixtures/home"
pevo_bin="${PEVO_BIN:-$repo_root/target/debug/pevo}"
server_pid=""

usage() {
  cat <<'USAGE'
Usage:
  scripts/pevo-tui-capture.sh install-deps
  scripts/pevo-tui-capture.sh demo

Commands:
  install-deps  Install VHS screenshot dependencies on Debian/Ubuntu.
  demo          Generate deterministic pevo TUI PNG diagnostics with VHS.

Environment:
  PEVO_BIN      Optional path to a pevo binary. Defaults to target/debug/pevo.
USAGE
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

require_file() {
  [[ -f "$1" ]] || die "missing required file: $1"
}

require_dir() {
  [[ -d "$1" ]] || die "missing required directory: $1"
}

cleanup_mock_server() {
  if [[ -n "${server_pid:-}" ]]; then
    kill "$server_pid" >/dev/null 2>&1 || true
    wait "$server_pid" >/dev/null 2>&1 || true
    server_pid=""
  fi
}

cleanup_and_exit() {
  local status="$1"
  cleanup_mock_server
  exit "$status"
}

install_deps() {
  require_command sudo
  require_command apt-get

  if ! command -v curl >/dev/null 2>&1 || ! command -v gpg >/dev/null 2>&1; then
    sudo apt-get update
    sudo apt-get install -y ca-certificates curl gpg
  fi

  sudo mkdir -p /etc/apt/keyrings
  curl -fsSL https://repo.charm.sh/apt/gpg.key \
    | sudo gpg --dearmor --yes -o /etc/apt/keyrings/charm.gpg
  echo 'deb [signed-by=/etc/apt/keyrings/charm.gpg] https://repo.charm.sh/apt/ * *' \
    | sudo tee /etc/apt/sources.list.d/charm.list >/dev/null
  sudo apt-get update
  sudo apt-get install -y vhs ttyd ffmpeg
}

check_demo_deps() {
  local missing=()
  for cmd in vhs ttyd ffmpeg python3 git; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      missing+=("$cmd")
    fi
  done
  if (( ${#missing[@]} > 0 )); then
    printf 'error: missing VHS capture dependencies: %s\n' "${missing[*]}" >&2
    printf 'run: scripts/pevo-tui-capture.sh install-deps\n' >&2
    exit 1
  fi
}

check_capture_assets() {
  require_file "$mock_provider_script"
  require_file "$tape_template"
  require_file "$capture_assets/render_tape.py"
  require_dir "$fixture_workdir"
  require_dir "$fixture_home"
}

copy_dir_contents() {
  local source="$1"
  local target="$2"
  mkdir -p "$target"
  cp -R "$source"/. "$target"/
}

wait_for_file() {
  local path="$1"
  local tries=100
  while (( tries > 0 )); do
    [[ -s "$path" ]] && return 0
    sleep 0.05
    tries=$((tries - 1))
  done
  return 1
}

shell_quote_args() {
  local quoted=""
  printf -v quoted '%q ' "$@"
  printf '%s\n' "${quoted% }"
}

render_tape() {
  local path="$1"
  local home="$2"
  local db_path="$3"
  local config_path="$4"
  local pevo_cmd="$5"

  python3 "$capture_assets/render_tape.py" \
    --template "$tape_template" \
    --output "$path" \
    --psychevo-home "$home" \
    --psychevo-db "$db_path" \
    --psychevo-config "$config_path" \
    --pevo-cmd "$pevo_cmd"
}

check_demo_artifacts() {
  local out_dir="$1"
  local missing=()
  for file in 01-model-picker.png 02-running-thinking.png 03-final-ledger.png 04-shell-mode.png 05-long-markdown-bottom-scroll.png 06-reasoning-only-collapsed.png 07-reasoning-only-bottom-scroll.png 08-visible-write-preamble.png 09-interrupted-exec-command.png 10-agent-tool-running.png 11-agent-session-running.png 12-agent-parent-completed.png 12-agents-running.png 13-agents-available.png 14-agent-actions.png 15-agent-run-prompt.png 16-clarify-panel.png 17-clarify-other-inline.png 18-clarify-result.png; do
    if [[ ! -s "$out_dir/$file" ]]; then
      missing+=("$file")
    fi
  done
  if (( ${#missing[@]} > 0 )); then
    printf 'error: VHS did not write expected screenshot(s): %s\n' "${missing[*]}" >&2
    exit 1
  fi
}

demo() {
  check_capture_assets
  check_demo_deps

  if [[ -z "${PEVO_BIN:-}" ]]; then
    cargo build -p psychevo-cli
  fi
  [[ -x "$pevo_bin" ]] || die "pevo binary is not executable: $pevo_bin"

  local timestamp
  timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
  local out_dir="$repo_root/.local/.psychevo-dev/tui-shots/$timestamp"
  local home="$out_dir/home"
  local workdir="$repo_root/.local/.psychevo-dev/tui-capture-work"
  local port_file="$out_dir/mock-provider.port"
  local request_log="$out_dir/mock-provider-requests.ndjson"
  local tape="$out_dir/pevo-tui-demo.tape"
  rm -rf "$workdir"
  mkdir -p "$home" "$workdir"
  copy_dir_contents "$fixture_workdir" "$workdir"

  git -C "$workdir" init -b main >/dev/null
  printf 'fixture.txt\n.psychevo/\n' > "$workdir/.git/info/exclude"

  python3 -u "$mock_provider_script" "$port_file" "$request_log" &
  server_pid="$!"
  trap cleanup_mock_server EXIT
  trap 'cleanup_and_exit 130' INT
  trap 'cleanup_and_exit 143' TERM
  wait_for_file "$port_file" || die "mock provider did not start"
  local port
  port="$(cat "$port_file")"

  PSYCHEVO_HOME="$home" "$pevo_bin" init >/dev/null
  copy_dir_contents "$fixture_home" "$home"
  cat > "$home/config.toml" <<EOF
model = "mock/mock-model"

[provider.mock.options]
base_url = "http://127.0.0.1:$port/v1"
api_key_env = "TEST_PROVIDER_KEY"

[provider.mock.models.mock-model]
reasoning_effort = "high"

[provider.mock.models.mock-model.limit]
context = 64000

[provider.mock.models.other-model]
reasoning_effort = "medium"

[provider.mock.models.other-model.limit]
context = 32000
EOF

  local pevo_cmd
  pevo_cmd="$(shell_quote_args env -u NO_COLOR TERM=xterm-256color COLORTERM=truecolor CLICOLOR_FORCE=1 "$pevo_bin" tui --dir "$workdir" -m mock/mock-model --variant high --debug)"
  render_tape "$tape" "$home" "$home/state.db" "$home/config.toml" "$pevo_cmd"

  (
    cd "$out_dir"
    PATH="$(dirname "$pevo_bin"):$repo_root/target/debug:$PATH" vhs "$tape"
  )
  check_demo_artifacts "$out_dir"

  printf 'wrote TUI capture artifacts: %s\n' "$out_dir"
  cleanup_mock_server
  trap - EXIT INT TERM
}

case "${1:-}" in
  install-deps)
    install_deps
    ;;
  demo)
    demo
    ;;
  -h|--help|help|"")
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
