---
name: 200. pevo install Script
psychevo_self_edit: deny
---

Define the source-install helper script for the `pevo` product CLI.

This attachment is part of [200 pevo CLI](spec.md).

## Scope

- one-command source install helper
- repository checkout and local-source install selection
- Rust/Cargo dependency detection and guided installation
- post-install verification and global home initialization
- Git Bash/MSYS/MINGW script compatibility boundaries

Out of scope:

- crates.io publishing, binary releases, package managers, or uninstall/update
  commands
- provider credential prompts, API-key validation, or live provider probes
- automatic shell profile edits
- automatic Visual Studio Build Tools, MinGW, Xcode, Homebrew, apt, or yum
  installation

## Script Contract

`scripts/install.sh` is a POSIX-compatible shell script. It must run with
`sh`, `bash`, and Windows Git Bash/MSYS/MINGW shells.

The default install source is the current checkout when the process cwd is
inside a Psychevo repository. Otherwise, the script clones
`https://github.com/wowfun/psychevo.git` at `main` into a temporary directory
and installs from that clone.

Accepted flags and environment defaults:

- `--repo-url <url>` overrides the clone URL. `PEVO_INSTALL_REPO` is the
  environment default.
- `--ref <ref>` overrides the clone branch or tag. `PEVO_INSTALL_REF` is the
  environment default.
- `--source <path>` forces installation from a local Psychevo source tree.
- `--with-peval` also installs and verifies the `peval` evaluation CLI.
- `--no-web` skips building and installing Web UI assets.
- `--no-init` skips post-install `pevo init`.
- `--dry-run` prints the resolved plan and commands without cloning,
  installing, initializing, or requiring installed dependencies.
- `-h, --help` prints usage.

Default installation uses:

```bash
cargo install --locked --path crates/psychevo-cli --force
```

When `--with-peval` is supplied, installation also uses:

```bash
cargo install --locked --path crates/psychevo-eval --force
```

The script must validate that local source directories contain the workspace
root and `crates/psychevo-cli/Cargo.toml` before installing.

## Dependency Handling

When cloning is required, missing `git` is a hard failure with a short install
hint.

When `cargo` is missing and stdin is interactive, the script asks before trying
to install Rust:

- Unix and WSL use the rustup shell installer.
- Windows Git Bash/MSYS/MINGW prefers `winget install --id Rustlang.Rustup -e`
  when `winget` is available, then rechecks `cargo`.

When `cargo` is missing in a non-interactive shell, or the guided installation
cannot make `cargo` available in the current process, the script fails with a
manual Rust installation hint. The documented `curl | sh` install path is
non-interactive stdin, so it requires Rust/Cargo to already be available.

Unix, macOS, and WSL source builds require a native C compiler/linker toolchain
before `cargo install` runs. The script must fail early when no `cc`, `gcc`, or
`clang` command is available, with a short platform-appropriate hint. The script
must not install native compiler toolchains automatically.

Web UI asset installation is enabled by default. When it is enabled, missing
`node` or `pnpm` is a hard failure with a short hint to install Node.js/pnpm or
rerun with `--no-web`. The script does not install Node.js or pnpm
automatically. The supported `pnpm` version follows the repository root
`packageManager` declaration.

If `cargo install` fails under Windows Git Bash/MSYS/MINGW, the failure text must
mention that Rust and native C/C++ build tools, such as Visual Studio Build
Tools or a compatible MinGW setup, may be required.

## Post-Install Behavior

After `cargo install` succeeds, the script locates `pevo` or `pevo.exe`, runs
`pevo --help`, and by default runs `pevo init`. When `--with-peval` is
supplied, the script also locates `peval` or `peval.exe` and runs
`peval --help`.

Unless `--no-web` is supplied, the script builds Workbench assets from the
selected source checkout using:

```bash
pnpm install --frozen-lockfile
pnpm --filter @psychevo/workbench build
```

It then copies `apps/workbench/dist` into `$(dirname pevo)/../share/psychevo/web`.
This install-share location is the stable Web UI asset location for source
installs. Dry-run output includes the install, build, and copy commands.

The install script must not run `peval init` automatically. Evaluation store
setup remains an explicit user action through `peval init`, `--root`, or
`PEVAL_ROOT`.

`pevo init` is idempotent and must not overwrite existing `config.toml` or
`.env` files. The install script must not write raw API keys.

If Cargo's bin directory is not on `PATH`, the script prints an `export PATH=...`
command and a short note that the user should add it to their shell profile. It
must not edit profiles automatically.

Final success guidance must only suggest `pevo web` when Web UI assets were
installed. CLI-only installs should instead point users to rerun the install
script without `--no-web` or use `pevo setup` after installing Node.js and pnpm.

## Related Topics

- [200 pevo CLI](spec.md) defines the product CLI surface.
- [200 pevo init](pevo-init.md) defines global home initialization.
- [200 Testing](testing.md) defines acceptance coverage.
