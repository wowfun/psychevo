#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
install=0
only="all"
apt_updated=0

usage() {
  cat <<'USAGE'
Usage:
  scripts/install-dev-test-deps.sh [--install] [--only all|sqlite|vhs|playwright]

Checks or installs local development and test dependencies. By default this
script only reports missing dependencies and exact install actions. Pass
--install to modify host packages or browser caches.

Options:
  --install      Install missing dependencies for the selected scope.
  --only <set>   Dependency set to check/install: all, sqlite, vhs, playwright.
                 Defaults to all.
  -h, --help     Show this help.

Dependency sets:
  sqlite      sqlite3 CLI used by browser/live validation.
  vhs         VHS TUI capture tools: vhs, ttyd, ffmpeg, python3, git.
  playwright  Playwright Chromium browser and system dependencies.
USAGE
}

info() {
  printf '%s\n' "$*" >&2
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

print_cmd() {
  printf '  %s\n' "$*" >&2
}

is_debian_like() {
  have_cmd apt-get || return 1
  if [[ ! -r /etc/os-release ]]; then
    return 1
  fi
  # shellcheck disable=SC1091
  . /etc/os-release
  [[ "${ID:-}" == "debian" || "${ID:-}" == "ubuntu" || " ${ID_LIKE:-} " == *" debian "* ]]
}

require_debian_install() {
  if ! is_debian_like; then
    die "--install currently supports Debian/Ubuntu systems with apt-get only. Install the reported tools with your platform package manager, then rerun without --install."
  fi
}

run_root() {
  if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
    "$@"
    return
  fi
  have_cmd sudo || die "sudo is required for --install on Debian/Ubuntu."
  sudo "$@"
}

apt_update_once() {
  require_debian_install
  if [[ "$apt_updated" -eq 0 ]]; then
    run_root apt-get update
    apt_updated=1
  fi
}

apt_install() {
  require_debian_install
  apt_update_once
  run_root apt-get install -y "$@"
}

install_charm_repo() {
  require_debian_install
  local bootstrap=()
  for cmd in curl gpg; do
    if ! have_cmd "$cmd"; then
      bootstrap+=("$cmd")
    fi
  done
  if ! have_cmd curl || ! have_cmd gpg || ! dpkg -s ca-certificates >/dev/null 2>&1; then
    apt_install ca-certificates curl gpg
  fi

  run_root mkdir -p /etc/apt/keyrings
  if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
    curl -fsSL https://repo.charm.sh/apt/gpg.key \
      | gpg --dearmor --yes -o /etc/apt/keyrings/charm.gpg
    printf '%s\n' 'deb [signed-by=/etc/apt/keyrings/charm.gpg] https://repo.charm.sh/apt/ * *' \
      > /etc/apt/sources.list.d/charm.list
  else
    curl -fsSL https://repo.charm.sh/apt/gpg.key \
      | sudo gpg --dearmor --yes -o /etc/apt/keyrings/charm.gpg
    printf '%s\n' 'deb [signed-by=/etc/apt/keyrings/charm.gpg] https://repo.charm.sh/apt/ * *' \
      | sudo tee /etc/apt/sources.list.d/charm.list >/dev/null
  fi
  apt_updated=0
  apt_update_once
}

need_scope() {
  local scope="$1"
  [[ "$only" == "all" || "$only" == "$scope" ]]
}

parse_args() {
  while (($# > 0)); do
    case "$1" in
      --install)
        install=1
        shift
        ;;
      --only)
        [[ $# -ge 2 ]] || die "--only requires one of: all, sqlite, vhs, playwright"
        only="$2"
        shift 2
        ;;
      --only=*)
        only="${1#--only=}"
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "unknown option: $1"
        ;;
    esac
  done

  case "$only" in
    all|sqlite|vhs|playwright) ;;
    *) die "--only must be one of: all, sqlite, vhs, playwright" ;;
  esac
}

report_core_tools() {
  local missing=()
  for cmd in cargo node pnpm; do
    if ! have_cmd "$cmd"; then
      missing+=("$cmd")
    fi
  done
  if ((${#missing[@]} > 0)); then
    info "core dev tools missing: ${missing[*]}"
    info "This script reports these tools but does not install them; use scripts/install.sh or your normal toolchain setup."
  else
    info "core dev tools: ok (cargo, node, pnpm)"
  fi
}

check_sqlite() {
  if have_cmd sqlite3; then
    info "sqlite: ok ($(sqlite3 --version | awk '{print $1}'))"
    return
  fi

  info "sqlite: missing sqlite3"
  info "install action:"
  print_cmd "scripts/install-dev-test-deps.sh --install --only sqlite"
  print_cmd "sudo apt-get update && sudo apt-get install -y sqlite3"
}

install_sqlite() {
  if have_cmd sqlite3; then
    info "sqlite: ok ($(sqlite3 --version | awk '{print $1}'))"
    return
  fi
  apt_install sqlite3
}

check_vhs() {
  local missing=()
  for cmd in vhs ttyd ffmpeg python3 git; do
    if ! have_cmd "$cmd"; then
      missing+=("$cmd")
    fi
  done
  if ((${#missing[@]} == 0)); then
    info "vhs: ok (vhs, ttyd, ffmpeg, python3, git)"
    return
  fi

  info "vhs: missing ${missing[*]}"
  info "install action:"
  print_cmd "scripts/install-dev-test-deps.sh --install --only vhs"
  print_cmd "sudo apt-get update"
  print_cmd "sudo apt-get install -y ca-certificates curl gpg python3 git"
  print_cmd "curl -fsSL https://repo.charm.sh/apt/gpg.key | sudo gpg --dearmor --yes -o /etc/apt/keyrings/charm.gpg"
  print_cmd "echo 'deb [signed-by=/etc/apt/keyrings/charm.gpg] https://repo.charm.sh/apt/ * *' | sudo tee /etc/apt/sources.list.d/charm.list >/dev/null"
  print_cmd "sudo apt-get update && sudo apt-get install -y vhs ttyd ffmpeg"
}

install_vhs() {
  local base_missing=()
  for cmd in python3 git; do
    if ! have_cmd "$cmd"; then
      base_missing+=("$cmd")
    fi
  done
  if ((${#base_missing[@]} > 0)); then
    apt_install "${base_missing[@]}"
  fi

  local charm_missing=()
  for cmd in vhs ttyd ffmpeg; do
    if ! have_cmd "$cmd"; then
      charm_missing+=("$cmd")
    fi
  done
  if ((${#charm_missing[@]} > 0)); then
    install_charm_repo
    apt_install vhs ttyd ffmpeg
  fi
}

check_playwright() {
  if ! have_cmd pnpm; then
    info "playwright: missing pnpm"
    info "install action after pnpm is available:"
    print_cmd "pnpm exec playwright install --with-deps chromium"
    return
  fi

  info "playwright: installed browser references"
  (cd "$repo_root" && pnpm exec playwright install --list) >&2
  info "playwright: dry-run install plan"
  (cd "$repo_root" && pnpm exec playwright install --dry-run chromium) >&2
  info "install action:"
  print_cmd "pnpm exec playwright install --with-deps chromium"
}

install_playwright() {
  require_debian_install
  have_cmd pnpm || die "pnpm is required for Playwright browser installation. Install Node.js/pnpm first."
  (cd "$repo_root" && pnpm exec playwright install --with-deps chromium)
}

main() {
  parse_args "$@"

  if [[ "$install" -eq 1 ]]; then
    info "Installing dev/test dependencies for scope: $only"
  else
    info "Checking dev/test dependencies for scope: $only"
  fi

  report_core_tools

  if need_scope sqlite; then
    if [[ "$install" -eq 1 ]]; then
      install_sqlite
      check_sqlite
    else
      check_sqlite
    fi
  fi

  if need_scope vhs; then
    if [[ "$install" -eq 1 ]]; then
      install_vhs
      check_vhs
    else
      check_vhs
    fi
  fi

  if need_scope playwright; then
    if [[ "$install" -eq 1 ]]; then
      install_playwright
      check_playwright
    else
      check_playwright
    fi
  fi
}

main "$@"
