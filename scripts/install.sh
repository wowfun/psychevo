#!/bin/sh
set -eu

DEFAULT_REPO_URL="https://github.com/wowfun/psychevo.git"
DEFAULT_REPO_REF="main"

repo_url="${PEVO_INSTALL_REPO:-$DEFAULT_REPO_URL}"
repo_ref="${PEVO_INSTALL_REF:-$DEFAULT_REPO_REF}"
source_arg=""
run_init=1
dry_run=0
install_peval=0
install_web=1
tmp_dir=""

usage() {
  cat <<'EOF'
usage: install.sh [options]

Install pevo from source. Use --with-peval to install the evaluation CLI too.

Options:
  --repo-url <url>  Git repository to clone when no local checkout is found
  --ref <ref>       Git branch or tag to clone
  --source <path>   Install from a local Psychevo source checkout
  --with-peval      Also install and verify the peval evaluation CLI
  --no-web          Skip building and installing Web UI assets
  --no-init         Skip post-install pevo init
  --dry-run         Print the resolved install plan without making changes
  -h, --help        Show this help

Environment defaults:
  PEVO_INSTALL_REPO
  PEVO_INSTALL_REF
EOF
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

shell_quote() {
  printf "'"
  printf '%s' "$1" | sed "s/'/'\\\\''/g"
  printf "'"
}

valid_source_dir() {
  [ -f "$1/Cargo.toml" ] && [ -f "$1/crates/psychevo-cli/Cargo.toml" ]
}

normalize_dir() {
  (CDPATH= cd "$1" 2>/dev/null && pwd -P)
}

find_source_from_cwd() {
  dir=$(pwd -P)
  while :; do
    if valid_source_dir "$dir"; then
      printf '%s\n' "$dir"
      return 0
    fi
    parent=$(dirname "$dir")
    if [ "$parent" = "$dir" ]; then
      return 1
    fi
    dir=$parent
  done
}

uname_value() {
  if [ -n "${PEVO_INSTALL_UNAME:-}" ]; then
    printf '%s\n' "$PEVO_INSTALL_UNAME"
  else
    uname -s 2>/dev/null || printf 'unknown\n'
  fi
}

is_windows_shell() {
  case "$(uname_value)" in
    MINGW*|MSYS*|CYGWIN*) return 0 ;;
    *) return 1 ;;
  esac
}

is_wsl() {
  if [ -n "${PEVO_INSTALL_WSL:-}" ] || [ -n "${WSL_DISTRO_NAME:-}" ] || [ -n "${WSL_INTEROP:-}" ]; then
    return 0
  fi
  if [ -r /proc/version ] && grep -qi 'microsoft\|wsl' /proc/version 2>/dev/null; then
    return 0
  fi
  return 1
}

platform_name() {
  if is_windows_shell; then
    printf 'windows-git-bash\n'
  elif is_wsl; then
    printf 'wsl\n'
  else
    printf 'unix\n'
  fi
}

cargo_bin_dir() {
  if [ -n "${CARGO_INSTALL_ROOT:-}" ]; then
    printf '%s/bin\n' "$CARGO_INSTALL_ROOT"
  elif [ -n "${CARGO_HOME:-}" ]; then
    printf '%s/bin\n' "$CARGO_HOME"
  elif [ -n "${HOME:-}" ]; then
    printf '%s/.cargo/bin\n' "$HOME"
  else
    printf '.cargo/bin\n'
  fi
}

pevo_bin_suffix() {
  if is_windows_shell; then
    printf '.exe\n'
  else
    printf '\n'
  fi
}

candidate_pevo_bin() {
  printf '%s/pevo%s\n' "$(cargo_bin_dir)" "$(pevo_bin_suffix)"
}

candidate_peval_bin() {
  printf '%s/peval%s\n' "$(cargo_bin_dir)" "$(pevo_bin_suffix)"
}

web_asset_target_for_bin() {
  bin_dir=$(dirname "$1")
  printf '%s/../share/psychevo/web\n' "$bin_dir"
}

path_contains_dir() {
  case ":${PATH:-}:" in
    *":$1:"*) return 0 ;;
    *) return 1 ;;
  esac
}

make_temp_dir() {
  base="${TMPDIR:-/tmp}"
  if have_cmd mktemp; then
    mktemp -d "$base/pevo-install.XXXXXX"
  else
    dir="$base/pevo-install.$$"
    mkdir -p "$dir"
    printf '%s\n' "$dir"
  fi
}

cleanup() {
  if [ -n "$tmp_dir" ] && [ -d "$tmp_dir" ]; then
    rm -rf "$tmp_dir"
  fi
}

manual_rust_hint() {
  if is_windows_shell; then
    printf 'Install Rust for Windows from https://rustup.rs/ or with winget install --id Rustlang.Rustup -e, then restart Git Bash and rerun this script.'
  else
    printf 'Install Rust from https://rustup.rs/ and rerun this script.'
  fi
}

native_build_hint() {
  case "$(uname_value)" in
    Darwin*)
      printf 'Install Xcode Command Line Tools with xcode-select --install, or install a C compiler toolchain, then rerun this script.'
      ;;
    Linux*)
      if is_wsl; then
        printf 'Install a Linux C compiler toolchain inside WSL, for example sudo apt install build-essential, then rerun this script.'
      else
        printf 'Install a C compiler toolchain, for example build-essential on Debian/Ubuntu or the equivalent for your distribution, then rerun this script.'
      fi
      ;;
    *)
      printf 'Install a native C compiler toolchain that provides cc, gcc, or clang, then rerun this script.'
      ;;
  esac
}

manual_web_hint() {
  printf 'Install Node.js and pnpm, then rerun this script, or use --no-web to install only the CLI.'
}

install_rust_unix() {
  have_cmd curl || die "curl is required to install Rust with rustup. $(manual_rust_hint)"
  info "Installing Rust with rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  if [ -n "${HOME:-}" ] && [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1090
    . "$HOME/.cargo/env"
  else
    PATH="$(cargo_bin_dir):${PATH:-}"
    export PATH
  fi
}

install_rust_windows() {
  if have_cmd winget; then
    info "Installing Rust with winget..."
    winget install --id Rustlang.Rustup -e
    PATH="$(cargo_bin_dir):${PATH:-}"
    export PATH
    return 0
  fi
  die "winget is not available. $(manual_rust_hint)"
}

ensure_cargo() {
  if have_cmd cargo; then
    return 0
  fi
  if [ "$dry_run" -eq 1 ]; then
    return 0
  fi
  if [ ! -t 0 ]; then
    die "cargo is required. $(manual_rust_hint)"
  fi
  printf 'Rust/Cargo is required to build pevo. Install Rust now? [y/N] ' >&2
  IFS= read -r answer || answer=""
  case "$answer" in
    y|Y|yes|YES|Yes)
      if is_windows_shell; then
        install_rust_windows
      else
        install_rust_unix
      fi
      ;;
    *)
      die "cargo is required. $(manual_rust_hint)"
      ;;
  esac
  have_cmd cargo || die "cargo is still not available in this shell. $(manual_rust_hint)"
}

ensure_native_build_tools() {
  if [ "$dry_run" -eq 1 ] || is_windows_shell; then
    return 0
  fi
  if have_cmd cc || have_cmd gcc || have_cmd clang; then
    return 0
  fi
  die "a native C compiler/linker is required to build pevo from source. $(native_build_hint)"
}

ensure_web_toolchain() {
  if [ "$install_web" -eq 0 ]; then
    return 0
  fi
  if ! have_cmd node; then
    die "Node.js is required to build Web UI assets. $(manual_web_hint)"
  fi
  if have_cmd pnpm; then
    return 0
  fi
  die "pnpm is required to build Web UI assets. $(manual_web_hint)"
}

resolve_pevo_bin() {
  candidate=$(candidate_pevo_bin)
  if [ -x "$candidate" ]; then
    printf '%s\n' "$candidate"
    return 0
  fi
  found=$(command -v pevo 2>/dev/null || true)
  if [ -n "$found" ]; then
    printf '%s\n' "$found"
    return 0
  fi
  found=$(command -v pevo.exe 2>/dev/null || true)
  if [ -n "$found" ]; then
    printf '%s\n' "$found"
    return 0
  fi
  return 1
}

resolve_peval_bin() {
  candidate=$(candidate_peval_bin)
  if [ -x "$candidate" ]; then
    printf '%s\n' "$candidate"
    return 0
  fi
  found=$(command -v peval 2>/dev/null || true)
  if [ -n "$found" ]; then
    printf '%s\n' "$found"
    return 0
  fi
  found=$(command -v peval.exe 2>/dev/null || true)
  if [ -n "$found" ]; then
    printf '%s\n' "$found"
    return 0
  fi
  return 1
}

print_path_hint_if_needed() {
  bin_dir=$(cargo_bin_dir)
  if path_contains_dir "$bin_dir"; then
    return 0
  fi
  cat >&2 <<EOF

Cargo's bin directory is not on PATH for this shell:
  $bin_dir

For this session, run:
  export PATH="$(shell_quote "$bin_dir"):\$PATH"

Add that line to your shell profile if you want installed commands available in new shells.
EOF
}

print_plan() {
  source_display=$source_dir
  if [ "$source_origin" = "clone" ] && [ "$dry_run" -eq 1 ]; then
    source_display="<temporary>/psychevo"
  fi
  printf 'pevo install dry run\n'
  printf 'platform: %s\n' "$(platform_name)"
  printf 'mode: %s\n' "$source_origin"
  printf 'repo_url: %s\n' "$repo_url"
  printf 'repo_ref: %s\n' "$repo_ref"
  printf 'with_peval: %s\n' "$install_peval"
  printf 'with_web: %s\n' "$install_web"
  printf 'source: %s\n' "$source_display"
  if [ "$source_origin" = "clone" ]; then
    printf 'clone_command: git clone --depth 1 --branch %s %s %s\n' \
      "$(shell_quote "$repo_ref")" \
      "$(shell_quote "$repo_url")" \
      "$(shell_quote "$source_display")"
  fi
  printf 'install_command: cargo install --locked --path %s --force\n' \
    "$(shell_quote "$source_display/crates/psychevo-cli")"
  printf 'pevo_binary: %s\n' "$(candidate_pevo_bin)"
  if [ "$install_web" -eq 1 ]; then
    printf 'web_install_command: cd %s && pnpm install --frozen-lockfile\n' \
      "$(shell_quote "$source_display")"
    printf 'web_build_command: cd %s && pnpm --filter @psychevo/workbench build\n' \
      "$(shell_quote "$source_display")"
    printf 'web_asset_source: %s\n' "$source_display/apps/workbench/dist"
    printf 'web_asset_target: %s\n' "$(web_asset_target_for_bin "$(candidate_pevo_bin)")"
  else
    printf 'web_asset_install: (skipped)\n'
  fi
  if [ "$install_peval" -eq 1 ]; then
    printf 'peval_install_command: cargo install --locked --path %s --force\n' \
      "$(shell_quote "$source_display/crates/psychevo-eval")"
    printf 'peval_binary: %s\n' "$(candidate_peval_bin)"
  fi
  if [ "$run_init" -eq 1 ]; then
    printf 'init_command: %s init\n' "$(shell_quote "$(candidate_pevo_bin)")"
  else
    printf 'init_command: (skipped)\n'
  fi
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo-url)
      [ "$#" -gt 1 ] || die "--repo-url requires a value"
      repo_url=$2
      shift 2
      ;;
    --ref)
      [ "$#" -gt 1 ] || die "--ref requires a value"
      repo_ref=$2
      shift 2
      ;;
    --source)
      [ "$#" -gt 1 ] || die "--source requires a value"
      source_arg=$2
      shift 2
      ;;
    --with-peval)
      install_peval=1
      shift
      ;;
    --no-web)
      install_web=0
      shift
      ;;
    --no-init)
      run_init=0
      shift
      ;;
    --dry-run)
      dry_run=1
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

source_origin="clone"
source_dir=""

if [ -n "$source_arg" ]; then
  source_dir=$(normalize_dir "$source_arg") || die "source directory does not exist: $source_arg"
  valid_source_dir "$source_dir" || die "not a Psychevo source checkout: $source_dir"
  source_origin="local"
else
  if source_dir=$(find_source_from_cwd 2>/dev/null); then
    source_origin="local"
  else
    source_origin="clone"
    if [ "$dry_run" -eq 1 ]; then
      source_dir="<temporary>/psychevo"
    fi
  fi
fi

if [ "$dry_run" -eq 1 ]; then
  print_plan
  exit 0
fi

trap cleanup EXIT HUP INT TERM

if [ "$source_origin" = "clone" ]; then
  have_cmd git || die "git is required to clone Psychevo. Install git and rerun this script."
  tmp_dir=$(make_temp_dir)
  source_dir="$tmp_dir/psychevo"
  info "Cloning Psychevo from $repo_url ($repo_ref)..."
  git clone --depth 1 --branch "$repo_ref" "$repo_url" "$source_dir"
fi

valid_source_dir "$source_dir" || die "not a Psychevo source checkout: $source_dir"
ensure_cargo
ensure_native_build_tools
ensure_web_toolchain

info "Installing pevo from $source_dir..."
if ! cargo install --locked --path "$source_dir/crates/psychevo-cli" --force; then
  if is_windows_shell; then
    die "cargo install failed. On Windows Git Bash/MSYS/MINGW, install Rust and native C/C++ build tools such as Visual Studio Build Tools or a compatible MinGW setup, then rerun this script."
  fi
  die "cargo install failed."
fi

pevo_bin=$(resolve_pevo_bin) || die "pevo was installed, but the binary could not be found."

info "Verifying pevo..."
"$pevo_bin" --help >/dev/null

if [ "$install_web" -eq 1 ]; then
  info "Building Workbench assets..."
  (CDPATH= cd "$source_dir" && pnpm install --frozen-lockfile)
  (CDPATH= cd "$source_dir" && pnpm --filter @psychevo/workbench build)
  web_source="$source_dir/apps/workbench/dist"
  [ -f "$web_source/index.html" ] || die "Workbench build did not produce $web_source/index.html"
  web_target=$(web_asset_target_for_bin "$pevo_bin")
  info "Installing Workbench assets to $web_target..."
  rm -rf "$web_target"
  mkdir -p "$web_target"
  cp -R "$web_source/." "$web_target/"
else
  info "Skipping Web UI assets because --no-web was supplied."
fi

if [ "$install_peval" -eq 1 ]; then
  [ -f "$source_dir/crates/psychevo-eval/Cargo.toml" ] || die "source checkout does not contain crates/psychevo-eval/Cargo.toml"
  info "Installing peval from $source_dir..."
  if ! cargo install --locked --path "$source_dir/crates/psychevo-eval" --force; then
    if is_windows_shell; then
      die "peval cargo install failed. On Windows Git Bash/MSYS/MINGW, install Rust and native C/C++ build tools such as Visual Studio Build Tools or a compatible MinGW setup, then rerun this script."
    fi
    die "peval cargo install failed."
  fi
  peval_bin=$(resolve_peval_bin) || die "peval was installed, but the binary could not be found."
  info "Verifying peval..."
  "$peval_bin" --help >/dev/null
fi

if [ "$run_init" -eq 1 ]; then
  info "Initializing Psychevo home..."
  "$pevo_bin" init
else
  info "Skipping pevo init because --no-init was supplied."
fi

print_path_hint_if_needed

cat <<EOF

pevo is installed:
  $pevo_bin
EOF

if [ "$install_peval" -eq 1 ]; then
  cat <<EOF
peval is installed:
  $peval_bin

EOF
fi

cat <<EOF
Try:
  pevo --help
  pevo
EOF

if [ "$install_web" -eq 1 ]; then
  cat <<'EOF'
  pevo web
EOF
else
  cat <<'EOF'

Web UI assets were skipped. To add them later, install Node.js and pnpm,
then rerun this installer from a Psychevo checkout without --no-web or run:
  pevo setup
EOF
fi

if [ "$install_peval" -eq 1 ]; then
  cat <<'EOF'
  peval --help
EOF
fi
