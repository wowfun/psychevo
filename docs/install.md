# Installation Guide

Psychevo currently documents source installs only. The installer runs from an
existing checkout, builds the local `pevo` binary with Cargo, builds Workbench
Web UI assets with pnpm, copies those assets beside the installed binary, and
runs `pevo init`.

## Recommended Path

Install the prerequisites for your platform first, then install from a checkout:

```bash
git clone https://github.com/wowfun/psychevo.git
cd psychevo
sh scripts/install.sh --check
sh scripts/install.sh
```

`--check` is read-only. It reports the tools, versions, platform, and network
settings the installer will use. It does not build, install, initialize, repair,
or copy files.

After installation, run the first-run wizard and local diagnostics:

```bash
pevo setup
pevo doctor
```

Then try a normal turn and the Web UI:

```bash
pevo run "summarize this repository"
pevo web
```

## Upgrade From a Source Checkout

For source installs, update the checkout yourself, then rerun the same
checkout-local installer:

```bash
git pull --ff-only
sh scripts/install.sh
```

There is intentionally no `pevo update` command yet. That name is reserved for
a future binary release updater with download and verification semantics.

## Prerequisites

The source installer expects these tools:

| Requirement | Needed for |
|-------------|------------|
| `git` | Cloning Psychevo before running the checkout-local installer. |
| Rust/Cargo | Building `pevo`. First-party workspaces require Rust 1.97.0 or newer and use edition 2024. |
| Native C compiler/linker | Source builds on Unix, macOS, WSL, and Windows Git Bash. |
| Node.js and `pnpm` | Workbench Web UI asset builds. |

For Web UI builds, use Node.js `20.19+`, `22.13+`, or `24+`. The repository
recommends `pnpm@11.8.0`. If another pnpm version is already installed, the
installer prints a warning and lets `pnpm install --frozen-lockfile` validate
the checkout. Installer pnpm subprocesses ignore Corepack project-version
enforcement and use installer-scoped `pmOnFail=warn` so a usable system pnpm
does not have to download the recommended version or fail during preflight.

The installer does not install Rust, Node.js, pnpm, system package manager
dependencies, Xcode Command Line Tools, Visual Studio Build Tools, MinGW, proxy
settings, registry settings, or CA configuration. It reports missing or outdated
tools and exits.

## Install Contract

The installer has only two user-facing options:

```bash
sh scripts/install.sh --check
sh scripts/install.sh --help
```

Normal installation runs the equivalent of:

```bash
cargo install --locked --path crates/psychevo-cli --force
pnpm install --frozen-lockfile
pnpm --filter @psychevo/workbench build
pevo init
```

It copies `apps/workbench/dist` into the install-share directory beside the
Cargo binary, normally `~/.cargo/share/psychevo/web`.

The install script intentionally does not model CLI-only, offline, prebuilt Web
asset, alternate source, or remote clone installation modes. Use the underlying
commands when you need those workflows:

```bash
cargo install --locked --path crates/psychevo-cli --force
pnpm install --frozen-lockfile
pnpm --filter @psychevo/workbench build
```

For offline or enterprise installs, configure Cargo, npm, pnpm, proxy, registry,
and CA settings outside the installer, then run the same checkout-local install
path.

## Diagnostics

Use the installer check before changing the host:

```bash
sh scripts/install.sh --check
```

Normal installs print `pevo install:` progress lines to stderr before each
host-dependent stage. If a company Windows machine appears to hang, the last
printed line usually identifies the area to inspect: Cargo/Rust, native build
tools, Node.js, pnpm, Cargo install, Workbench build, asset copy, or profile
initialization.

The installer asks Corepack to use the system pnpm for its subprocesses instead
of downloading the repository's recommended pnpm version, and it treats pnpm
package-manager version mismatches as warnings for that installer run. If pnpm
still invokes Corepack and fails with a certificate error, fix the Node/Corepack
trust path before rerunning the installer. Common options are:

```bash
export NODE_EXTRA_CA_CERTS='D:/path/to/company-root-ca.pem'
corepack prepare pnpm@11.8.0 --activate
```

or install/activate pnpm from an internal npm registry managed by your company.

On Windows Git Bash/MSYS/MINGW, the installer defaults its `cargo install`
subprocess to `CARGO_HTTP_CHECK_REVOKE=false` when you have not already set that
variable. This avoids common company-network failures such as
`CRYPT_E_NO_REVOCATION_CHECK` while keeping the change scoped to the current
installer run. If your security policy requires revocation checks, run the
installer with:

```bash
CARGO_HTTP_CHECK_REVOKE=true sh scripts/install.sh
```

For persistent Cargo use outside the installer, configure Cargo explicitly:

```toml
[http]
check-revoke = false
```

For intermittent Cargo registry timeouts, the installer gives only its
`cargo install` subprocess more tolerant defaults:
`CARGO_HTTP_TIMEOUT=120` and `CARGO_NET_RETRY=10`. Existing user values are
preserved, and nothing is written to Cargo config. If many crates download but a
few requests repeatedly time out, try a manual rerun with HTTP/2 multiplexing
disabled for that process:

```bash
CARGO_HTTP_MULTIPLEXING=false sh scripts/install.sh
```

If timeouts continue, configure Cargo proxy, CA, or a company crates mirror
outside the installer.

On Windows Git Bash/MSYS/MINGW, a source reinstall can fail after compilation
with `failed to move ... pevo.exe` and `os error 5` if Windows is still holding
the installed binary open. The installer first tries to stop the managed
Gateway. If replacement still fails, close running `pevo`, TUI, Web, Gateway,
or `serve` processes and rerun `sh scripts/install.sh`. If it still fails after
all `pevo` processes are closed, inspect endpoint protection or permission
policy for Cargo's bin directory.

If a Windows install fails while compiling `landlock`, update to a newer
checkout and rerun the installer. Native Windows source installs do not compile
Linux Landlock; Windows sandbox enforcement remains unsupported and fails closed
when enabled.

From a checkout with Cargo available, use the fuller dependency report:

```bash
cargo xtask doctor deps check --only install
```

For a shell trace, use:

```bash
sh -x scripts/install.sh 2>&1 | tee pevo-install.log
```

When Cargo or pnpm steps fail, the installer prints an enterprise network
diagnostics block. It reports npm/pnpm registry, Cargo config presence, proxy
variables, and CA-related variables. It does not edit any of those settings.

If Cargo's bin directory is missing from `PATH`, the installer prints an
`export PATH=...` command. Add it to your shell profile if you want `pevo`
available in new shells.

## Platform Notes

On Ubuntu or WSL, install the common system pieces first:

```bash
sudo apt update
sudo apt install git curl build-essential
```

Install Rust from <https://rustup.rs/>. Install Node.js with your normal Node
manager or OS package source, then enable or install pnpm:

```bash
corepack enable
corepack prepare pnpm@11.8.0 --activate
```

On macOS, install Xcode Command Line Tools before the source build:

```bash
xcode-select --install
```

On Windows, use Git Bash/MSYS/MINGW for the installer. The script looks for
Windows build tools such as Visual Studio Build Tools, MinGW, `gcc`, `clang`,
`cc`, `cl`, `link`, or `vswhere`. If none are found, it fails with guidance. It
never installs those build tools or edits shell profiles.

For enterprise networks, configure Cargo, npm, pnpm, proxies, registries, and CA
settings outside the installer.

## Development Without Installing

Run the CLI from source:

```bash
cargo run -p psychevo-cli -- --help
```

Run the Workbench dev server:

```bash
pnpm --filter @psychevo/workbench dev
```
