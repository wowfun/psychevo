#!/bin/sh
set -eu

DEFAULT_REPO_URL="https://github.com/wowfun/psychevo.git"
DEFAULT_REPO_REF="main"

repo_url="${PEVO_INSTALL_REPO:-$DEFAULT_REPO_URL}"
repo_ref="${PEVO_INSTALL_REF:-$DEFAULT_REPO_REF}"
source_arg=""
run_init=1
dry_run=0
check_only=0
offline=0
install_peval=0
install_web=1
web_dist_arg=""
web_dist_dir=""
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
  --check           Check dependencies and environment readiness only
  --offline         Require a local checkout and use offline Cargo/pnpm installs
  --web-dist <path> Install prebuilt Web UI assets from an existing dist folder
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
    if have_cmd rm; then
      rm -rf "$tmp_dir"
    fi
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

developer_install_check_hint() {
  if [ "${source_origin:-}" = "local" ] && [ -n "${source_dir:-}" ]; then
    printf ' In a Psychevo checkout, run cargo xtask doctor deps check --only install for a full dependency report.'
  fi
}

confirm() {
  prompt=$1
  if [ ! -t 0 ]; then
    return 1
  fi
  printf '%s [y/N] ' "$prompt" >&2
  IFS= read -r answer || answer=""
  case "$answer" in
    y|Y|yes|YES|Yes) return 0 ;;
    *) return 1 ;;
  esac
}

add_path_dir() {
  dir=$1
  [ -n "$dir" ] || return 0
  [ -d "$dir" ] || return 0
  if ! path_contains_dir "$dir"; then
    PATH="$dir:${PATH:-}"
    export PATH
  fi
}

refresh_tool_path() {
  add_path_dir "$(cargo_bin_dir)"
  if is_windows_shell; then
    if [ -n "${HOME:-}" ]; then
      add_path_dir "$HOME/AppData/Local/pnpm"
      add_path_dir "$HOME/AppData/Roaming/npm"
    fi
    add_path_dir "/c/Program Files/nodejs"
    add_path_dir "/c/Program Files (x86)/nodejs"
  fi
}

extract_quoted_value() {
  value=$1
  case "$value" in
    *\"*\")
      value=${value#*\"}
      value=${value%%\"*}
      printf '%s\n' "$value"
      ;;
    *)
      return 1
      ;;
  esac
}

workspace_rust_version() {
  [ -n "${source_dir:-}" ] && [ -f "$source_dir/Cargo.toml" ] || return 1
  while IFS= read -r line; do
    case "$line" in
      rust-version*)
        extract_quoted_value "$line"
        return 0
        ;;
    esac
  done <"$source_dir/Cargo.toml"
  return 1
}

workspace_pnpm_version() {
  [ -n "${source_dir:-}" ] && [ -f "$source_dir/package.json" ] || return 1
  while IFS= read -r line; do
    case "$line" in
      *\"packageManager\"*\"pnpm@\"*)
        value=${line#*pnpm@}
        value=${value%%\"*}
        printf '%s\n' "$value"
        return 0
        ;;
    esac
  done <"$source_dir/package.json"
  return 1
}

clean_version() {
  value=$1
  value=${value#rustc }
  value=${value#cargo }
  value=${value#pnpm }
  value=${value#v}
  value=${value%% *}
  value=${value%%-*}
  value=${value%%+*}
  printf '%s\n' "$value"
}

version_ge() {
  have=$(clean_version "$1")
  need=$(clean_version "$2")

  h1=${have%%.*}
  hrest=${have#*.}
  if [ "$hrest" = "$have" ]; then hrest=0; fi
  h2=${hrest%%.*}
  hrest=${hrest#*.}
  if [ "$hrest" = "$h2" ]; then hrest=0; fi
  h3=${hrest%%.*}

  n1=${need%%.*}
  nrest=${need#*.}
  if [ "$nrest" = "$need" ]; then nrest=0; fi
  n2=${nrest%%.*}
  nrest=${nrest#*.}
  if [ "$nrest" = "$n2" ]; then nrest=0; fi
  n3=${nrest%%.*}

  h1=${h1:-0}; h2=${h2:-0}; h3=${h3:-0}
  n1=${n1:-0}; n2=${n2:-0}; n3=${n3:-0}

  if [ "$h1" -gt "$n1" ]; then return 0; fi
  if [ "$h1" -lt "$n1" ]; then return 1; fi
  if [ "$h2" -gt "$n2" ]; then return 0; fi
  if [ "$h2" -lt "$n2" ]; then return 1; fi
  [ "$h3" -ge "$n3" ]
}

node_version_supported() {
  version=$(clean_version "$1")
  major=${version%%.*}
  rest=${version#*.}
  if [ "$rest" = "$version" ]; then rest=0; fi
  minor=${rest%%.*}
  rest=${rest#*.}
  if [ "$rest" = "$minor" ]; then rest=0; fi
  patch=${rest%%.*}
  major=${major:-0}; minor=${minor:-0}; patch=${patch:-0}

  if [ "$major" -eq 20 ]; then
    version_ge "$version" "20.19.0"
    return $?
  fi
  if [ "$major" -eq 22 ]; then
    version_ge "$version" "22.13.0"
    return $?
  fi
  [ "$major" -ge 24 ]
}

node_requirement_hint() {
  printf 'Node.js 20.19+, 22.13+, or 24+'
}

command_version() {
  command=$1
  shift
  if ! have_cmd "$command"; then
    return 1
  fi
  "$command" "$@" 2>/dev/null | {
    IFS= read -r line || line=""
    clean_version "$line"
  }
}

pnpm_repair_hint() {
  required=$1
  if have_cmd corepack; then
    printf 'Run: corepack enable && corepack prepare pnpm@%s --activate' "$required"
  elif have_cmd npm; then
    printf 'Run: npm install -g pnpm@%s' "$required"
  else
    printf 'Install Node.js Corepack or npm, then install pnpm@%s' "$required"
  fi
}

redact_url_credentials() {
  value=$1
  case "$value" in
    *://*@*) printf '%s\n' "${value%%://*}://***@${value#*@}" ;;
    *) printf '%s\n' "$value" ;;
  esac
}

print_env_value() {
  name=$1
  eval "value=\${$name:-}"
  if [ -n "$value" ]; then
    printf '  %s: %s\n' "$name" "$(redact_url_credentials "$value")" >&2
  else
    printf '  %s: (unset)\n' "$name" >&2
  fi
}

print_enterprise_diagnostics() {
  reason=$1
  printf '\nEnterprise network diagnostics (%s):\n' "$reason" >&2
  printf '  repo_url: %s\n' "$repo_url" >&2
  if have_cmd git; then
    git_proxy=$(git config --get http.proxy 2>/dev/null || true)
    [ -n "$git_proxy" ] || git_proxy="(unset)"
    printf '  git http.proxy: %s\n' "$(redact_url_credentials "$git_proxy")" >&2
  else
    printf '  git http.proxy: git unavailable\n' >&2
  fi
  if have_cmd npm; then
    npm_registry=$(npm config get registry 2>/dev/null || true)
    [ -n "$npm_registry" ] || npm_registry="(unset)"
    printf '  npm registry: %s\n' "$npm_registry" >&2
  else
    printf '  npm registry: npm unavailable\n' >&2
  fi
  if have_cmd pnpm; then
    pnpm_registry=$(pnpm config get registry 2>/dev/null || true)
    [ -n "$pnpm_registry" ] || pnpm_registry="(unset)"
    printf '  pnpm registry: %s\n' "$pnpm_registry" >&2
  else
    printf '  pnpm registry: pnpm unavailable\n' >&2
  fi
  cargo_config_found=0
  if [ -n "${source_dir:-}" ] && [ -f "$source_dir/.cargo/config.toml" ]; then
    cargo_config_found=1
    printf '  cargo config: %s\n' "$source_dir/.cargo/config.toml" >&2
  fi
  if [ -n "${CARGO_HOME:-}" ] && [ -f "$CARGO_HOME/config.toml" ]; then
    cargo_config_found=1
    printf '  cargo config: %s\n' "$CARGO_HOME/config.toml" >&2
  elif [ -n "${HOME:-}" ] && [ -f "$HOME/.cargo/config.toml" ]; then
    cargo_config_found=1
    printf '  cargo config: %s\n' "$HOME/.cargo/config.toml" >&2
  fi
  if [ "$cargo_config_found" -eq 0 ]; then
    printf '  cargo config: (not found)\n' >&2
  fi
  print_env_value HTTP_PROXY
  print_env_value HTTPS_PROXY
  print_env_value ALL_PROXY
  print_env_value NO_PROXY
  print_env_value SSL_CERT_FILE
  print_env_value GIT_SSL_CAINFO
  print_env_value NODE_EXTRA_CA_CERTS
  print_env_value CARGO_HTTP_CAINFO
  print_env_value CARGO_HTTP_PROXY
}

windows_build_tools_available() {
  have_cmd cl || have_cmd link || have_cmd gcc || have_cmd clang || have_cmd cc || have_cmd vswhere
}

check_status=0

check_line() {
  name=$1
  status=$2
  detail=$3
  printf '%s: %s' "$name" "$status"
  if [ -n "$detail" ]; then
    printf ' - %s' "$detail"
  fi
  printf '\n'
  case "$status" in
    ok|skipped|info|warn) ;;
    *) check_status=1 ;;
  esac
}

print_check_report() {
  check_status=0
  printf 'pevo install check\n'
  printf 'platform: %s\n' "$(platform_name)"
  printf 'mode: %s\n' "$source_origin"
  printf 'offline: %s\n' "$offline"
  if valid_source_dir "${source_dir:-}"; then
    printf 'source: %s\n' "$source_dir"
  else
    printf 'source: unavailable (run from a checkout or pass --source for version checks)\n'
  fi

  if [ "$source_origin" = "clone" ]; then
    if have_cmd git; then
      check_line "git" "ok" "$(command -v git)"
    else
      check_line "git" "missing" "required to clone Psychevo"
    fi
  else
    check_line "git" "skipped" "local source checkout selected"
  fi

  if have_cmd cargo; then
    cargo_version=$(command_version cargo --version || true)
    check_line "cargo" "ok" "$cargo_version"
  else
    check_line "cargo" "missing" "$(manual_rust_hint)"
  fi
  if have_cmd rustc; then
    rust_version=$(command_version rustc --version || true)
    required_rust=$(workspace_rust_version || true)
    if [ -n "$required_rust" ] && ! version_ge "$rust_version" "$required_rust"; then
      check_line "rustc" "outdated" "found $rust_version, requires $required_rust"
    else
      detail=$rust_version
      [ -n "$required_rust" ] && detail="$detail, requires $required_rust"
      check_line "rustc" "ok" "$detail"
    fi
  else
    check_line "rustc" "missing" "$(manual_rust_hint)"
  fi

  if is_windows_shell; then
    if windows_build_tools_available; then
      check_line "windows-build-tools" "ok" "found cl/link/gcc/clang/cc/vswhere"
    else
      check_line "windows-build-tools" "missing" "install Visual Studio Build Tools or a compatible MinGW setup"
    fi
  elif have_cmd cc || have_cmd gcc || have_cmd clang; then
    check_line "native-build-tools" "ok" "found cc/gcc/clang"
  else
    check_line "native-build-tools" "missing" "$(native_build_hint)"
  fi

  if [ "$install_web" -eq 0 ]; then
    check_line "node" "skipped" "--no-web supplied"
    check_line "pnpm" "skipped" "--no-web supplied"
  elif [ -n "$web_dist_dir" ]; then
    check_line "node" "skipped" "--web-dist supplied"
    check_line "pnpm" "skipped" "--web-dist supplied"
  else
    if have_cmd node; then
      node_version=$(command_version node --version || true)
      if node_version_supported "$node_version"; then
        check_line "node" "ok" "$node_version"
      else
        check_line "node" "outdated" "found $node_version, requires $(node_requirement_hint)"
      fi
    else
      check_line "node" "missing" "$(manual_web_hint)"
    fi
    required_pnpm=$(workspace_pnpm_version || true)
    [ -n "$required_pnpm" ] || required_pnpm="11.8.0"
    if have_cmd pnpm; then
      pnpm_version=$(command_version pnpm --version || true)
      if [ "$pnpm_version" = "$required_pnpm" ]; then
        check_line "pnpm" "ok" "$pnpm_version"
      else
        check_line "pnpm" "warn" "found $pnpm_version, recommended $required_pnpm"
      fi
    else
      check_line "pnpm" "missing" "requires $required_pnpm. $(pnpm_repair_hint "$required_pnpm")"
    fi
  fi

  print_enterprise_diagnostics "check"
  return "$check_status"
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
    refresh_tool_path
    return 0
  fi
  die "winget is not available. $(manual_rust_hint)"
}

ensure_rust_version() {
  have_cmd rustc || die "rustc is required. $(manual_rust_hint)"
  required_rust=$(workspace_rust_version || true)
  [ -n "$required_rust" ] || return 0
  rust_version=$(command_version rustc --version || true)
  if version_ge "$rust_version" "$required_rust"; then
    return 0
  fi
  if [ "$dry_run" -eq 1 ]; then
    return 0
  fi
  if have_cmd rustup && confirm "Rust $rust_version is installed, but Psychevo requires Rust $required_rust. Update Rust with rustup now?"; then
    info "Updating Rust with rustup..."
    rustup update stable
    refresh_tool_path
    rust_version=$(command_version rustc --version || true)
    if version_ge "$rust_version" "$required_rust"; then
      return 0
    fi
  fi
  die "Rust $required_rust or newer is required; found ${rust_version:-unknown}. $(manual_rust_hint)"
}

ensure_cargo() {
  if have_cmd cargo; then
    ensure_rust_version
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
  refresh_tool_path
  have_cmd cargo || die "cargo is still not available in this shell. $(manual_rust_hint)"
  ensure_rust_version
}

ensure_native_build_tools() {
  if [ "$dry_run" -eq 1 ]; then
    return 0
  fi
  if is_windows_shell; then
    if windows_build_tools_available; then
      return 0
    fi
    printf '%s\n' "Windows native build tools were not detected on PATH." >&2
    printf '\n' >&2
    printf '%s\n' "Install Visual Studio Build Tools with the C++ workload, or install a compatible" >&2
    printf '%s\n' "MinGW/clang toolchain, then restart Git Bash and rerun this script." >&2
    if confirm "Continue anyway and let cargo attempt the build?"; then
      return 0
    fi
    die "Windows native C/C++ build tools are required to build pevo from source."
  fi
  if have_cmd cc || have_cmd gcc || have_cmd clang; then
    return 0
  fi
  die "a native C compiler/linker is required to build pevo from source. $(native_build_hint)$(developer_install_check_hint)"
}

ensure_node_version() {
  if ! have_cmd node; then
    die "Node.js is required to build Web UI assets. $(manual_web_hint)$(developer_install_check_hint)"
  fi
  node_version=$(command_version node --version || true)
  if node_version_supported "$node_version"; then
    return 0
  fi
  die "Node.js ${node_version:-unknown} is installed, but $(node_requirement_hint) is required to build Web UI assets. Install a supported Node.js version, then rerun this script, or use --no-web to install only the CLI."
}

ensure_pnpm_version() {
  required_pnpm=$(workspace_pnpm_version || true)
  [ -n "$required_pnpm" ] || required_pnpm="11.8.0"
  if have_cmd pnpm; then
    pnpm_version=$(command_version pnpm --version || true)
    if [ "$pnpm_version" = "$required_pnpm" ]; then
      return 0
    fi
    info "warning: pnpm ${pnpm_version:-unknown} is installed; pnpm $required_pnpm is recommended for this checkout. Continuing and letting pnpm validate the lockfile/build."
    return 0
  else
    problem="pnpm $required_pnpm is required to build Web UI assets."
  fi

  if [ "$dry_run" -eq 1 ]; then
    return 0
  fi

  if have_cmd corepack && confirm "$problem Activate pnpm $required_pnpm with Corepack now?"; then
    corepack enable
    corepack prepare "pnpm@$required_pnpm" --activate
    refresh_tool_path
  elif have_cmd npm && confirm "$problem Install pnpm $required_pnpm globally with npm now?"; then
    npm install -g "pnpm@$required_pnpm"
    refresh_tool_path
  else
    die "$problem $(pnpm_repair_hint "$required_pnpm"), then rerun this script, or use --no-web to install only the CLI.$(developer_install_check_hint)"
  fi

  if have_cmd pnpm; then
    pnpm_version=$(command_version pnpm --version || true)
    if [ "$pnpm_version" != "$required_pnpm" ]; then
      info "warning: pnpm ${pnpm_version:-unknown} is installed; pnpm $required_pnpm is recommended for this checkout. Continuing and letting pnpm validate the lockfile/build."
    fi
    return 0
  fi
  die "pnpm $required_pnpm is still not available in this shell. $(pnpm_repair_hint "$required_pnpm")$(developer_install_check_hint)"
}

ensure_web_toolchain() {
  if [ "$install_web" -eq 0 ] || [ -n "$web_dist_dir" ]; then
    return 0
  fi
  ensure_node_version
  ensure_pnpm_version
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
  printf 'offline: %s\n' "$offline"
  printf 'source: %s\n' "$source_display"
  if [ -n "$web_dist_dir" ]; then
    printf 'web_dist: %s\n' "$web_dist_dir"
  fi
  if [ "$source_origin" = "clone" ]; then
    printf 'clone_command: git clone --depth 1 --branch %s %s %s\n' \
      "$(shell_quote "$repo_ref")" \
      "$(shell_quote "$repo_url")" \
      "$(shell_quote "$source_display")"
  fi
  if [ "$offline" -eq 1 ]; then
    printf 'install_command: cargo install --locked --offline --path %s --force\n' \
      "$(shell_quote "$source_display/crates/psychevo-cli")"
  else
    printf 'install_command: cargo install --locked --path %s --force\n' \
      "$(shell_quote "$source_display/crates/psychevo-cli")"
  fi
  printf 'pevo_binary: %s\n' "$(candidate_pevo_bin)"
  if [ "$install_web" -eq 1 ]; then
    if [ -n "$web_dist_dir" ]; then
      printf 'web_install_command: (skipped; --web-dist supplied)\n'
      printf 'web_build_command: (skipped; --web-dist supplied)\n'
      printf 'web_asset_source: %s\n' "$web_dist_dir"
    elif [ "$offline" -eq 1 ]; then
      printf 'web_install_command: cd %s && pnpm install --frozen-lockfile --offline\n' \
        "$(shell_quote "$source_display")"
      printf 'web_build_command: cd %s && pnpm --offline --filter @psychevo/workbench build\n' \
        "$(shell_quote "$source_display")"
      printf 'web_asset_source: %s\n' "$source_display/apps/workbench/dist"
    else
      printf 'web_install_command: cd %s && pnpm install --frozen-lockfile\n' \
        "$(shell_quote "$source_display")"
      printf 'web_build_command: cd %s && pnpm --filter @psychevo/workbench build\n' \
        "$(shell_quote "$source_display")"
      printf 'web_asset_source: %s\n' "$source_display/apps/workbench/dist"
    fi
    printf 'web_asset_target: %s\n' "$(web_asset_target_for_bin "$(candidate_pevo_bin)")"
  else
    printf 'web_asset_install: (skipped)\n'
  fi
  if [ "$install_peval" -eq 1 ]; then
    if [ "$offline" -eq 1 ]; then
      printf 'peval_install_command: cargo install --locked --offline --path %s --force\n' \
        "$(shell_quote "$source_display/crates/psychevo-eval")"
    else
      printf 'peval_install_command: cargo install --locked --path %s --force\n' \
        "$(shell_quote "$source_display/crates/psychevo-eval")"
    fi
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
    --check)
      check_only=1
      shift
      ;;
    --offline)
      offline=1
      shift
      ;;
    --web-dist)
      [ "$#" -gt 1 ] || die "--web-dist requires a value"
      web_dist_arg=$2
      shift 2
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

[ "$check_only" -eq 1 ] && [ "$dry_run" -eq 1 ] && die "--check cannot be combined with --dry-run"
[ "$install_web" -eq 0 ] && [ -n "$web_dist_arg" ] && die "--web-dist cannot be used with --no-web"

if [ -n "$web_dist_arg" ]; then
  web_dist_dir=$(normalize_dir "$web_dist_arg") || die "Web UI dist directory does not exist: $web_dist_arg"
  [ -f "$web_dist_dir/index.html" ] || die "--web-dist directory must contain index.html: $web_dist_dir"
fi

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

if [ "$offline" -eq 1 ] && [ "$source_origin" = "clone" ]; then
  die "--offline requires an existing Psychevo source checkout; run from a checkout or pass --source"
fi

if [ "$check_only" -eq 1 ]; then
  print_check_report
  exit $?
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
  if ! git clone --depth 1 --branch "$repo_ref" "$repo_url" "$source_dir"; then
    print_enterprise_diagnostics "git clone failed"
    die "git clone failed. For enterprise networks, use --repo-url or PEVO_INSTALL_REPO with an internal mirror, or configure Git proxy/CA settings."
  fi
fi

valid_source_dir "$source_dir" || die "not a Psychevo source checkout: $source_dir"
ensure_cargo
ensure_native_build_tools
ensure_web_toolchain

info "Installing pevo from $source_dir..."
if [ "$offline" -eq 1 ]; then
  if cargo install --locked --offline --path "$source_dir/crates/psychevo-cli" --force; then
    cargo_status=0
  else
    cargo_status=1
  fi
else
  if cargo install --locked --path "$source_dir/crates/psychevo-cli" --force; then
    cargo_status=0
  else
    cargo_status=1
  fi
fi
if [ "$cargo_status" -ne 0 ]; then
  print_enterprise_diagnostics "cargo install failed"
  if is_windows_shell; then
    die "cargo install failed. On Windows Git Bash/MSYS/MINGW, install Rust and native C/C++ build tools such as Visual Studio Build Tools or a compatible MinGW setup, then rerun this script."
  fi
  die "cargo install failed."
fi

pevo_bin=$(resolve_pevo_bin) || die "pevo was installed, but the binary could not be found."

info "Verifying pevo..."
"$pevo_bin" --help >/dev/null

if [ "$install_web" -eq 1 ]; then
  if [ -n "$web_dist_dir" ]; then
    info "Using prebuilt Workbench assets from $web_dist_dir..."
    web_source="$web_dist_dir"
  else
    info "Building Workbench assets..."
    if [ "$offline" -eq 1 ]; then
      if ! (CDPATH= cd "$source_dir" && pnpm install --frozen-lockfile --offline); then
        print_enterprise_diagnostics "pnpm install failed"
        die "pnpm install failed."
      fi
      if ! (CDPATH= cd "$source_dir" && pnpm --offline --filter @psychevo/workbench build); then
        print_enterprise_diagnostics "pnpm build failed"
        die "pnpm build failed."
      fi
    else
      if ! (CDPATH= cd "$source_dir" && pnpm install --frozen-lockfile); then
        print_enterprise_diagnostics "pnpm install failed"
        die "pnpm install failed."
      fi
      if ! (CDPATH= cd "$source_dir" && pnpm --filter @psychevo/workbench build); then
        print_enterprise_diagnostics "pnpm build failed"
        die "pnpm build failed."
      fi
    fi
    web_source="$source_dir/apps/workbench/dist"
  fi
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
  if [ "$offline" -eq 1 ]; then
    if cargo install --locked --offline --path "$source_dir/crates/psychevo-eval" --force; then
      peval_cargo_status=0
    else
      peval_cargo_status=1
    fi
  else
    if cargo install --locked --path "$source_dir/crates/psychevo-eval" --force; then
      peval_cargo_status=0
    else
      peval_cargo_status=1
    fi
  fi
  if [ "$peval_cargo_status" -ne 0 ]; then
    print_enterprise_diagnostics "peval cargo install failed"
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
