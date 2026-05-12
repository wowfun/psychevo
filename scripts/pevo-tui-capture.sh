#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

write_mock_server() {
  local path="$1"
  cat > "$path" <<'PY'
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

port_file = Path(sys.argv[1])
request_log = Path(sys.argv[2])


class Handler(BaseHTTPRequestHandler):
    request_count = 0

    def log_message(self, fmt, *args):
        return

    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        body = self.rfile.read(length).decode("utf-8", errors="replace")
        Handler.request_count += 1
        with request_log.open("a", encoding="utf-8") as out:
            out.write(json.dumps({"index": Handler.request_count, "path": self.path, "body": body}) + "\n")

        if self.path.rstrip("/") != "/v1/chat/completions":
            self.send_response(404)
            self.end_headers()
            return

        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache")
        self.end_headers()

        if "Title this user request" in body or "Generate a concise title" in body:
            self.send_event({
                "id": "resp_tui_capture_title",
                "model": "mock-model",
                "choices": [{
                    "delta": {
                        "content": "Inspect Fixture Ledger"
                    },
                    "finish_reason": "stop"
                }]
            }, delay=0.1)
        elif "Interrupted bash fixture" in body:
            self.send_event({
                "id": "resp_tui_capture_interrupt",
                "model": "mock-model",
                "choices": [{
                    "delta": {
                        "content": "Starting a bash command that should be interrupted."
                    },
                    "finish_reason": None
                }]
            }, delay=0.2)
            self.send_event({
                "id": "resp_tui_capture_interrupt",
                "model": "mock-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_interrupted_bash",
                            "function": {
                                "name": "bash",
                                "arguments": "{\"command\":\"sleep 60\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            })
        elif "visible-write-output.md" in body:
            self.send_event({
                "id": "resp_tui_capture_visible_write_final",
                "model": "mock-model",
                "choices": [{
                    "delta": {
                        "content": "VISIBLE_WRITE_FINAL"
                    },
                    "finish_reason": "stop"
                }]
            }, delay=0.2)
        elif "Visible write preamble fixture" in body:
            self.send_event({
                "id": "resp_tui_capture_visible_write",
                "model": "mock-model",
                "choices": [{
                    "delta": {
                        "content": "Now I have all the data needed. Let me write the complete report."
                    },
                    "finish_reason": None
                }]
            }, delay=1.0)
            self.send_event({
                "id": "resp_tui_capture_visible_write",
                "model": "mock-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_visible_write",
                            "function": {
                                "name": "write",
                                "arguments": "{\"path\":\"visible-write-output.md\",\"content\":\"VISIBLE_WRITE_FINAL\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            })
        elif "Reasoning-only table bottom scroll fixture" in body:
            rows = "\n".join(
                f"| {index} | **Thinking Story {index}** - reasoning-only Markdown table row used to validate bottom scrolling | {200 + index} |"
                for index in range(1, 22)
            )
            content = (
                "Reasoning-only final report\n\n"
                "| # | Topic | Score |\n"
                "|---|---|---|\n"
                f"{rows}\n\n"
                "REASONING_ONLY_BOTTOM_MARKER: metadata below must remain reachable."
            )
            self.send_event({
                "id": "resp_tui_capture_reasoning_only",
                "model": "mock-model",
                "choices": [],
                "usage": {
                    "prompt_tokens": 260,
                    "completion_tokens": 160,
                    "total_tokens": 420
                }
            }, delay=0.2)
            self.send_event({
                "id": "resp_tui_capture_reasoning_only",
                "model": "mock-model",
                "choices": [{
                    "delta": {"reasoning_content": content},
                    "finish_reason": "stop"
                }]
            })
        elif "Long markdown bottom scroll fixture" in body:
            rows = "\n".join(
                f"| {index} | **Story {index}** - long Markdown table row used to validate transcript bottom scrolling | {100 + index} |"
                for index in range(1, 34)
            )
            content = (
                "# Long Markdown Scroll Fixture\n\n"
                "| # | Topic | Score |\n"
                "|---|---|---|\n"
                f"{rows}\n\n"
                "LONG_MARKDOWN_BOTTOM_MARKER: metadata below must remain reachable."
            )
            self.send_event({
                "id": "resp_tui_capture_long",
                "model": "mock-model",
                "choices": [],
                "usage": {
                    "prompt_tokens": 240,
                    "completion_tokens": 180,
                    "total_tokens": 420
                }
            }, delay=0.2)
            self.send_event({
                "id": "resp_tui_capture_long",
                "model": "mock-model",
                "choices": [{
                    "delta": {"content": content},
                    "finish_reason": "stop"
                }]
            })
        elif Handler.request_count == 1:
            self.send_event({
                "id": "resp_tui_capture_1",
                "model": "mock-model",
                "choices": [{
                    "delta": {"content": "I'll inspect fixture.txt before summarizing the ledger."},
                    "finish_reason": None
                }]
            }, delay=0.2)
            self.send_event({
                "id": "resp_tui_capture_1",
                "model": "mock-model",
                "choices": [{
                    "delta": {"reasoning_content": "Inspecting fixture.txt and the TUI ledger..."},
                    "finish_reason": None
                }]
            }, delay=0.4)
            self.send_event({
                "id": "resp_tui_capture_1",
                "model": "mock-model",
                "choices": [{
                    "delta": {"reasoning_content": "Preparing a bash tool call."},
                    "finish_reason": None
                }]
            }, delay=0.4)
            self.send_event({
                "id": "resp_tui_capture_1",
                "model": "mock-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_bash_fixture",
                            "function": {
                                "name": "bash",
                                "arguments": "{\"command\":\"sleep 2 && cat fixture.txt\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            })
        else:
            self.send_event({
                "id": "resp_tui_capture_2",
                "model": "mock-model",
                "choices": [],
                "usage": {
                    "prompt_tokens": 120,
                    "completion_tokens": 38,
                    "total_tokens": 158
                }
            }, delay=0.2)
            self.send_event({
                "id": "resp_tui_capture_2",
                "model": "mock-model",
                "choices": [{
                    "delta": {
                        "content": "SNAPSHOT_DEMO_FINAL: fixture.txt was read and the evidence ledger is ready."
                    },
                    "finish_reason": "stop"
                }]
            })
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    def send_event(self, value, delay=0.0):
        self.wfile.write(("data: " + json.dumps(value) + "\n\n").encode("utf-8"))
        self.wfile.flush()
        if delay:
            time.sleep(delay)


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
port_file.write_text(str(server.server_address[1]), encoding="utf-8")
server.serve_forever()
PY
}

json_quote() {
  python3 -c 'import json, sys; print(json.dumps(sys.argv[1]))' "$1"
}

shell_quote_args() {
  local quoted=""
  printf -v quoted '%q ' "$@"
  printf '%s\n' "${quoted% }"
}

write_tape() {
  local path="$1"
  local out_dir="$2"
  local home="$3"
  local db_path="$4"
  local workdir="$5"
  local config_path="$6"
  local pevo_cmd="$7"

  cat > "$path" <<EOF
Require bash
Require python3
Set Shell bash
Set Width 1200
Set Height 720
Set FontSize 18
Env TERM "xterm-256color"
Env COLORTERM "truecolor"
Env CLICOLOR_FORCE "1"
Env PSYCHEVO_HOME $(json_quote "$home")
Env PSYCHEVO_DB $(json_quote "$db_path")
Env PSYCHEVO_CONFIG $(json_quote "$config_path")
Env TEST_PROVIDER_KEY "test-key"
Type $(json_quote "$pevo_cmd")
Enter
Wait+Screen /Ask pevo|pevo/
Type "/model"
Enter
Wait+Screen /Add provider/
Sleep 500 ms
Screenshot $(json_quote "$out_dir/01-model-picker.png")
Escape
Type "Inspect the snapshot harness and read fixture.txt"
Enter
Wait+Screen /Running sleep 2 && cat fixture.txt/
Sleep 200 ms
Screenshot $(json_quote "$out_dir/02-running-thinking.png")
Wait+Screen /SNAPSHOT_DEMO_FINAL/
Sleep 300 ms
Ctrl+B
Sleep 300 ms
Screenshot $(json_quote "$out_dir/03-final-ledger.png")
Escape
Sleep 100 ms
Type "!"
Sleep 200 ms
Screenshot $(json_quote "$out_dir/04-shell-mode.png")
Escape
Sleep 200 ms
Type "Long markdown bottom scroll fixture"
Enter
Wait+Screen /LONG_MARKDOWN_BOTTOM_MARKER/
Sleep 300 ms
PageUp 8
Sleep 100 ms
Down 80
Wait+Screen /LONG_MARKDOWN_BOTTOM_MARKER/
Sleep 300 ms
Screenshot $(json_quote "$out_dir/05-long-markdown-bottom-scroll.png")
Type "/new"
Enter
Wait+Screen /Ask pevo/
Sleep 200 ms
Type "Reasoning-only table bottom scroll fixture"
Enter
Wait+Screen /Thinking Story 1/
Sleep 300 ms
Screenshot $(json_quote "$out_dir/06-reasoning-only-collapsed.png")
Ctrl+T
Space
Sleep 200 ms
PageUp 8
Sleep 100 ms
Down 80
Wait+Screen /REASONING_ONLY_BOTTOM_MARKER/
Sleep 300 ms
Screenshot $(json_quote "$out_dir/07-reasoning-only-bottom-scroll.png")
Escape
Type "Visible write preamble fixture"
Enter
Wait+Screen /Changing files/
Sleep 300 ms
Screenshot $(json_quote "$out_dir/08-visible-write-preamble.png")
Wait+Screen /VISIBLE_WRITE_FINAL/
Sleep 200 ms
Type "Interrupted bash fixture"
Enter
Wait+Screen /Running sleep 60/
Sleep 300 ms
Escape
Wait+Screen /interrupted/
Sleep 300 ms
Screenshot $(json_quote "$out_dir/09-interrupted-bash.png")
Ctrl+D
EOF
}

check_demo_artifacts() {
  local out_dir="$1"
  local missing=()
  for file in 01-model-picker.png 02-running-thinking.png 03-final-ledger.png 04-shell-mode.png 05-long-markdown-bottom-scroll.png 06-reasoning-only-collapsed.png 07-reasoning-only-bottom-scroll.png 08-visible-write-preamble.png 09-interrupted-bash.png; do
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
  local server_script="$out_dir/mock_provider.py"
  local tape="$out_dir/pevo-tui-demo.tape"
  rm -rf "$workdir"
  mkdir -p "$home" "$workdir"

  cat > "$workdir/fixture.txt" <<'EOF'
pevo TUI visual regression fixture
line 02: stable local file evidence
line 03: used by the read tool in the VHS demo
EOF
  git -C "$workdir" init -b main >/dev/null
  printf 'fixture.txt\n' > "$workdir/.git/info/exclude"

  write_mock_server "$server_script"
  python3 -u "$server_script" "$port_file" "$request_log" &
  server_pid="$!"
  trap cleanup_mock_server EXIT
  trap 'cleanup_and_exit 130' INT
  trap 'cleanup_and_exit 143' TERM
  wait_for_file "$port_file" || die "mock provider did not start"
  local port
  port="$(cat "$port_file")"

  PSYCHEVO_HOME="$home" "$pevo_bin" init >/dev/null
  cat > "$home/config.jsonc" <<EOF
{
  "model": "mock/mock-model",
  "provider": {
    "mock": {
      "options": {
        "base_url": "http://127.0.0.1:$port/v1",
        "api_key_env": "TEST_PROVIDER_KEY"
      },
      "models": {
        "mock-model": {
          "reasoning_effort": "high",
          "limit": { "context": 64000 }
        },
        "other-model": {
          "reasoning_effort": "medium",
          "limit": { "context": 32000 }
        }
      }
    }
  }
}
EOF
  cat > "$home/.env" <<'EOF'
TEST_PROVIDER_KEY=test-key
EOF

  local pevo_cmd
  pevo_cmd="$(shell_quote_args env -u NO_COLOR TERM=xterm-256color COLORTERM=truecolor CLICOLOR_FORCE=1 "$pevo_bin" tui --dir "$workdir" -m mock/mock-model --variant high --debug)"
  write_tape "$tape" "$out_dir" "$home" "$home/state.db" "$workdir" "$home/config.jsonc" "$pevo_cmd"

  (
    cd "$repo_root"
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
