---
name: 200. pevo install Script
psychevo_self_edit: deny
---

Define the checkout-local source-install helper script for the `pevo` product
CLI.

This attachment is part of [200 pevo CLI](spec.md).

`scripts/install.sh` is a POSIX-compatible shell script. It must run with `sh`,
`bash`, and Windows Git Bash/MSYS/MINGW shells. It installs from the current
Psychevo checkout only; it does not clone repositories, install from arbitrary
source paths, install release binaries, or model package-manager workflows.

## Scope

- checkout-local full source install helper
- source-checkout reinstall and upgrade UX
- Rust/Cargo, native compiler, Node.js, and pnpm dependency detection
- Workbench asset build and install
- post-install `pevo` verification and global home initialization
- Git Bash/MSYS/MINGW script compatibility boundaries

Out of scope:

- crates.io publishing, binary releases, package managers, or uninstall/update
  commands
- remote clone, Git mirror, source override, offline, CLI-only, or prebuilt Web
  asset installer modes
- provider credential prompts, API-key validation, or live provider probes
- automatic shell profile edits
- automatic Rust, Visual Studio Build Tools, MinGW, Xcode, Homebrew, apt, yum,
  Node.js, Corepack, npm, or pnpm installation

## Script Contract

Accepted flags:

- `--check` prints dependency, version, and environment-readiness diagnostics
  without installing, building Web assets, copying files, initializing, or
  attempting toolchain repairs.
- `-h, --help` prints usage.

The script has no installer-owned environment variables. It may use standard
host/tool environment such as `PATH`, `HOME`, `CARGO_HOME`,
`CARGO_INSTALL_ROOT`, proxy variables, and CA variables.

The script finds the source checkout by walking upward from the process cwd and
requires a workspace root containing `Cargo.toml` and
`crates/psychevo-cli/Cargo.toml`. If no checkout is found, it must fail before
any install action with a short `git clone ... && cd psychevo` style hint.

Normal installation always runs the same full product path:

```bash
cargo install --locked --path crates/psychevo-cli --force
pnpm install --frozen-lockfile
pnpm --filter @psychevo/workbench build
pevo init
```

After the Workbench build, the script copies `apps/workbench/dist` into
`$(dirname pevo)/../share/psychevo/web`. This install-share location is the
stable Web UI asset location for source installs.

Normal installation must print short stderr progress breadcrumbs before each
potentially slow or host-dependent stage. The breadcrumb prefix is
`pevo install:` and must include enough stage context to identify where a hang
occurred.

Rerunning `scripts/install.sh` from an updated checkout is the supported source
upgrade path. The script must not expose `pevo update`; that command name is
reserved for a future release updater with binary download and verification
semantics.

## Dependency Handling

When `cargo` or `rustc` is missing, the script fails with a manual Rust
installation hint. It must not install Rust automatically. When `cargo` is
present, the script checks `rustc --version` against the root `Cargo.toml`
`rust-version`; outdated Rust is a hard failure.

Unix, macOS, and WSL source builds require a native C compiler/linker toolchain
before `cargo install` runs. The script must fail early when no `cc`, `gcc`, or
`clang` command is available, with a short platform-appropriate hint. Windows
Git Bash/MSYS/MINGW source builds require `cl`, `link`, `gcc`, `clang`, `cc`, or
`vswhere`; missing Windows build tools are a hard failure with Visual Studio
Build Tools or compatible MinGW/clang guidance.

Node.js and pnpm are required because the install script always builds
Workbench assets. The script checks Node.js against the current frontend
requirement, with supported lines `20.19+`, `22.13+`, or `24+`. Missing or
unsupported Node.js is a hard failure.

The repository root `packageManager` declaration is the recommended pnpm
version for source installs, not an exact installer gate. If `pnpm` is present
but differs from the root `packageManager` declaration, the script prints a
warning and continues; `pnpm install --frozen-lockfile` remains the real
compatibility gate. The installer runs its pnpm subprocesses with Corepack
project-version enforcement disabled so a corporate machine with a usable older
system pnpm does not have to download the recommended pnpm version during
preflight. If `pnpm` is present but version detection still fails, including a
failing Corepack shim, the script treats pnpm as unusable and fails before Cargo
install or Web build steps. The failure must keep the underlying stderr visible
and point to Corepack/npm registry, proxy, and CA remediation paths. The
installer must not modify user Corepack, npm, pnpm, registry, proxy, or CA
configuration.

When the script reports missing native or Web build prerequisites, it may point
developers to `cargo xtask doctor deps check --only install` for a complete
non-mutating dependency report. It must not give that `xtask` hint for missing
Cargo bootstrap failures where `xtask` cannot run.

Before running `cargo install` on Windows Git Bash/MSYS/MINGW, the script may
best-effort stop the managed Gateway by running the existing installed
`pevo.exe gateway stop`. This preflight is diagnostic and cleanup-oriented: it
must ignore missing `pevo.exe` and failed stop attempts, and it must not stop
unmanaged user processes.

If `cargo install` fails under Windows Git Bash/MSYS/MINGW while replacing the
installed `pevo.exe` with an access-denied move error, the failure text must
identify the target binary as locked and guide the user to close running
`pevo`, TUI, Web, Gateway, or `serve` processes before rerunning the installer.
If the replacement failure persists after those processes are closed, the text
may point to endpoint protection or permission policy as the next area to
inspect. This locked-binary case must not be reported as a network, Rust, or
native build-tool failure.

Other `cargo install` failures under Windows Git Bash/MSYS/MINGW must mention
that Rust and native C/C++ build tools, such as Visual Studio Build Tools or a
compatible MinGW setup, may be required. Windows Git Bash/MSYS/MINGW installs
may default the `cargo install` subprocess to `CARGO_HTTP_CHECK_REVOKE=false`
so corporate networks that block certificate revocation checks can still fetch
the registry index. This default is scoped to the installer subprocess only;
the script must not write Cargo configuration, shell profiles, or other
persistent settings. If the user explicitly sets `CARGO_HTTP_CHECK_REVOKE`, the
script must preserve that value.

The installer may also give the `cargo install` subprocess more tolerant
network defaults for intermittent registry fetches: `CARGO_HTTP_TIMEOUT=120`
and `CARGO_NET_RETRY=10`. These defaults are scoped to the Cargo subprocess
only, are not written to Cargo configuration, and must not override user-set
environment values. The installer must not default
`CARGO_HTTP_MULTIPLEXING=false`; users may set that manually when a corporate
proxy mishandles HTTP/2 multiplexing.

When Cargo install or pnpm install/build steps fail, the script prints a compact
enterprise-network diagnostics block, except for the Windows locked-binary
replacement case described above. The block reports relevant npm/pnpm
registry, Cargo registry/source configuration presence, proxy variables, and
CA-related environment variables, including effective Cargo install network
values for `CARGO_HTTP_TIMEOUT`, `CARGO_NET_RETRY`,
`CARGO_HTTP_LOW_SPEED_LIMIT`, `CARGO_HTTP_MULTIPLEXING`, and
`CARGO_HTTP_CHECK_REVOKE`. It must not modify Git, npm, pnpm, Cargo, proxy,
registry, mirror, or CA configuration.

Enterprise diagnostics collection itself is a named install stage so users can
distinguish a failing build step from a hang while reading proxy, registry, or
CA configuration.

## Post-Install Behavior

After `cargo install` succeeds, the script locates `pevo` or `pevo.exe`, runs
`pevo --help`, builds and installs Workbench assets, and runs `pevo init`.

If installation is interrupted by `INT` or `TERM`, the script reports the
current install stage before exiting. This message is best-effort diagnostic
output and must not replace failure handling.

`pevo init` is idempotent and must not overwrite existing `config.toml` or
`.env` files. The install script must not write raw API keys.

If Cargo's bin directory is not on `PATH`, the script prints an `export
PATH=...` command and a short note that the user should add it to their shell
profile. It must not edit profiles automatically.

Final success guidance should suggest `pevo --help`, `pevo`, and `pevo web`.

## Related Topics

- [200 pevo CLI](spec.md) defines the product CLI surface.
- [200 pevo init](pevo-init.md) defines global home initialization.
- [200 Testing](testing.md) defines acceptance coverage.
