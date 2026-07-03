#!/bin/sh
set -eu

check_only=0
current_step="starting"

usage() {
  cat <<'EOF'
usage: install.sh [options]

Install pevo from the current Psychevo source checkout.

Options:
  --check     Check dependencies and environment readiness only
  -h, --help  Show this help
EOF
}

info() {
  printf '%s\n' "$*" >&2
}

step() {
  current_step=$1
  if [ "$check_only" -eq 0 ]; then
    info "pevo install: $current_step..."
  fi
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

checkout_required_hint() {
  printf 'Run this script from inside a Psychevo checkout. For a fresh checkout, run: git clone https://github.com/wowfun/psychevo.git && cd psychevo && sh scripts/install.sh'
}

uname_value() {
  uname -s 2>/dev/null || printf 'unknown\n'
}

is_windows_shell() {
  case "$(uname_value)" in
    MINGW*|MSYS*|CYGWIN*) return 0 ;;
    *) return 1 ;;
  esac
}

is_wsl() {
  if [ -n "${WSL_DISTRO_NAME:-}" ] || [ -n "${WSL_INTEROP:-}" ]; then
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

handle_interrupt() {
  signal=$1
  if [ -n "${current_step:-}" ]; then
    info "pevo install: interrupted during $current_step"
  else
    info "pevo install: interrupted"
  fi
  trap - HUP INT TERM
  case "$signal" in
    INT) exit 130 ;;
    TERM) exit 143 ;;
    HUP) exit 129 ;;
    *) exit 1 ;;
  esac
}

manual_rust_hint() {
  if is_windows_shell; then
    printf 'Install Rust for Windows from https://rustup.rs/, then restart Git Bash and rerun this script.'
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
  printf 'Install Node.js and pnpm, then rerun this script.'
}

developer_install_check_hint() {
  printf ' In a Psychevo checkout, run cargo xtask doctor deps check --only install for a full dependency report.'
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
  [ -f "$source_dir/Cargo.toml" ] || return 1
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
  [ -f "$source_dir/package.json" ] || return 1
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
  output=$("$command" "$@") || return 1
  IFS='
'
  set -- $output
  unset IFS
  clean_version "${1:-}"
}

run_pnpm() {
  COREPACK_ENABLE_PROJECT_SPEC=0 COREPACK_ENABLE_DOWNLOAD_PROMPT=0 pnpm "$@"
}

cargo_install_http_timeout_default() {
  printf '120\n'
}

cargo_install_net_retry_default() {
  printf '10\n'
}

cargo_install_http_timeout_value() {
  if [ -n "${CARGO_HTTP_TIMEOUT+x}" ]; then
    printf '%s\n' "$CARGO_HTTP_TIMEOUT"
  else
    cargo_install_http_timeout_default
  fi
}

cargo_install_net_retry_value() {
  if [ -n "${CARGO_NET_RETRY+x}" ]; then
    printf '%s\n' "$CARGO_NET_RETRY"
  else
    cargo_install_net_retry_default
  fi
}

run_cargo_install() {
  cargo_http_timeout=$(cargo_install_http_timeout_value)
  cargo_net_retry=$(cargo_install_net_retry_value)
  if is_windows_shell && [ -z "${CARGO_HTTP_CHECK_REVOKE+x}" ]; then
    CARGO_HTTP_TIMEOUT="$cargo_http_timeout" \
      CARGO_NET_RETRY="$cargo_net_retry" \
      CARGO_HTTP_CHECK_REVOKE=false \
      cargo install "$@"
  else
    CARGO_HTTP_TIMEOUT="$cargo_http_timeout" \
      CARGO_NET_RETRY="$cargo_net_retry" \
      cargo install "$@"
  fi
}

detect_pnpm_version() {
  if ! have_cmd pnpm; then
    return 1
  fi
  output=$(run_pnpm --version) || return 1
  IFS='
'
  set -- $output
  unset IFS
  clean_version "${1:-}"
}

pnpm_repair_hint() {
  required=$1
  printf 'Install or activate pnpm %s with your approved Node.js package manager.' "$required"
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

print_effective_env_value() {
  name=$1
  default_value=$2
  default_label=$3
  eval "is_set=\${$name+x}"
  if [ -n "$is_set" ]; then
    eval "value=\${$name-}"
    if [ -n "$value" ]; then
      printf '  %s: %s\n' "$name" "$(redact_url_credentials "$value")" >&2
    else
      printf '  %s: (set empty)\n' "$name" >&2
    fi
  elif [ -n "$default_label" ]; then
    printf '  %s: %s (%s)\n' "$name" "$default_value" "$default_label" >&2
  else
    printf '  %s: (unset)\n' "$name" >&2
  fi
}

print_effective_cargo_revoke_value() {
  if is_windows_shell; then
    print_effective_env_value CARGO_HTTP_CHECK_REVOKE false "installer default for cargo install"
  else
    print_env_value CARGO_HTTP_CHECK_REVOKE
  fi
}

print_enterprise_diagnostics() {
  reason=$1
  step "collecting enterprise diagnostics"
  printf '\nEnterprise network diagnostics (%s):\n' "$reason" >&2
  if have_cmd npm; then
    npm_registry=$(npm config get registry 2>/dev/null || true)
    [ -n "$npm_registry" ] || npm_registry="(unset)"
    printf '  npm registry: %s\n' "$npm_registry" >&2
  else
    printf '  npm registry: npm unavailable\n' >&2
  fi
  if have_cmd pnpm; then
    pnpm_registry=$(run_pnpm config get registry 2>/dev/null || true)
    [ -n "$pnpm_registry" ] || pnpm_registry="(unset)"
    printf '  pnpm registry: %s\n' "$pnpm_registry" >&2
  else
    printf '  pnpm registry: pnpm unavailable\n' >&2
  fi
  cargo_config_found=0
  if [ -f "$source_dir/.cargo/config.toml" ]; then
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
  print_effective_env_value CARGO_HTTP_TIMEOUT "$(cargo_install_http_timeout_default)" "installer default for cargo install"
  print_effective_env_value CARGO_NET_RETRY "$(cargo_install_net_retry_default)" "installer default for cargo install"
  print_env_value CARGO_HTTP_LOW_SPEED_LIMIT
  print_env_value CARGO_HTTP_MULTIPLEXING
  print_effective_cargo_revoke_value
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
  printf 'source: %s\n' "$source_dir"

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
    if pnpm_version=$(detect_pnpm_version); then
      if [ "$pnpm_version" = "$required_pnpm" ]; then
        check_line "pnpm" "ok" "$pnpm_version"
      else
        check_line "pnpm" "warn" "found $pnpm_version, recommended $required_pnpm"
      fi
    else
      check_line "pnpm" "unusable" "pnpm --version failed; configure Corepack/npm registry or corporate CA settings"
    fi
  else
    check_line "pnpm" "missing" "requires $required_pnpm. $(pnpm_repair_hint "$required_pnpm")"
  fi

  print_enterprise_diagnostics "check"
  return "$check_status"
}

pnpm_unusable_hint() {
  required=$1
  printf 'pnpm exists on PATH, but `pnpm --version` failed. If this is a Corepack shim, configure corporate proxy/CA settings or pre-activate pnpm %s, then rerun this script.' "$required"
}

ensure_rust_version() {
  step "checking Rust version"
  have_cmd rustc || die "rustc is required. $(manual_rust_hint)"
  required_rust=$(workspace_rust_version || true)
  [ -n "$required_rust" ] || return 0
  rust_version=$(command_version rustc --version || true)
  if version_ge "$rust_version" "$required_rust"; then
    return 0
  fi
  die "Rust $required_rust or newer is required; found ${rust_version:-unknown}. $(manual_rust_hint)"
}

ensure_cargo() {
  step "checking Cargo"
  have_cmd cargo || die "cargo is required. $(manual_rust_hint)"
  ensure_rust_version
}

ensure_native_build_tools() {
  step "checking native build tools"
  if is_windows_shell; then
    if windows_build_tools_available; then
      return 0
    fi
    die "Windows native C/C++ build tools are required to build pevo from source. Install Visual Studio Build Tools with the C++ workload, or install a compatible MinGW/clang toolchain, then restart Git Bash and rerun this script."
  fi
  if have_cmd cc || have_cmd gcc || have_cmd clang; then
    return 0
  fi
  die "a native C compiler/linker is required to build pevo from source. $(native_build_hint)$(developer_install_check_hint)"
}

ensure_node_version() {
  step "checking Node.js"
  if ! have_cmd node; then
    die "Node.js is required to build Workbench assets. $(manual_web_hint)$(developer_install_check_hint)"
  fi
  node_version=$(command_version node --version || true)
  if node_version_supported "$node_version"; then
    return 0
  fi
  die "Node.js ${node_version:-unknown} is installed, but $(node_requirement_hint) is required to build Workbench assets. Install a supported Node.js version, then rerun this script."
}

ensure_pnpm_version() {
  step "checking pnpm"
  required_pnpm=$(workspace_pnpm_version || true)
  [ -n "$required_pnpm" ] || required_pnpm="11.8.0"
  if have_cmd pnpm; then
    pnpm_version=$(detect_pnpm_version) || die "$(pnpm_unusable_hint "$required_pnpm")$(developer_install_check_hint)"
    if [ "$pnpm_version" = "$required_pnpm" ]; then
      return 0
    fi
    info "warning: pnpm ${pnpm_version:-unknown} is installed; pnpm $required_pnpm is recommended for this checkout. Continuing and letting pnpm validate the lockfile/build."
    return 0
  fi
  die "pnpm $required_pnpm is required to build Workbench assets. $(pnpm_repair_hint "$required_pnpm")$(developer_install_check_hint)"
}

ensure_web_toolchain() {
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

while [ "$#" -gt 0 ]; do
  case "$1" in
    --check)
      check_only=1
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

source_dir=$(find_source_from_cwd 2>/dev/null) || die "$(checkout_required_hint)"

if [ "$check_only" -eq 1 ]; then
  print_check_report
  exit $?
fi

trap 'handle_interrupt HUP' HUP
trap 'handle_interrupt INT' INT
trap 'handle_interrupt TERM' TERM

step "using $(platform_name) source checkout at $source_dir"
step "validating source checkout"
valid_source_dir "$source_dir" || die "not a Psychevo source checkout: $source_dir"
ensure_cargo
ensure_native_build_tools
ensure_web_toolchain

step "installing pevo from $source_dir"
if run_cargo_install --locked --path "$source_dir/crates/psychevo-cli" --force; then
  cargo_status=0
else
  cargo_status=1
fi
if [ "$cargo_status" -ne 0 ]; then
  print_enterprise_diagnostics "cargo install failed"
  if is_windows_shell; then
    die "cargo install failed. On Windows Git Bash/MSYS/MINGW, install Rust and native C/C++ build tools such as Visual Studio Build Tools or a compatible MinGW setup. If registry fetches time out after partial progress, try CARGO_HTTP_MULTIPLEXING=false or configure Cargo proxy, CA, or registry mirror settings."
  fi
  die "cargo install failed. If registry fetches time out after partial progress, try CARGO_HTTP_MULTIPLEXING=false or configure Cargo proxy, CA, or registry mirror settings."
fi

pevo_bin=$(resolve_pevo_bin) || die "pevo was installed, but the binary could not be found."

step "verifying pevo"
"$pevo_bin" --help >/dev/null

step "building Workbench assets"
if ! (CDPATH= cd "$source_dir" && run_pnpm install --frozen-lockfile); then
  print_enterprise_diagnostics "pnpm install failed"
  die "pnpm install failed."
fi
if ! (CDPATH= cd "$source_dir" && run_pnpm --filter @psychevo/workbench build); then
  print_enterprise_diagnostics "pnpm build failed"
  die "pnpm build failed."
fi

web_source="$source_dir/apps/workbench/dist"
[ -f "$web_source/index.html" ] || die "Workbench build did not produce $web_source/index.html"
web_target=$(web_asset_target_for_bin "$pevo_bin")
step "installing Workbench assets to $web_target"
rm -rf "$web_target"
mkdir -p "$web_target"
cp -R "$web_source/." "$web_target/"

step "initializing Psychevo home"
"$pevo_bin" init

print_path_hint_if_needed

cat <<EOF

pevo is installed:
  $pevo_bin

Try:
  pevo --help
  pevo
  pevo web
EOF
