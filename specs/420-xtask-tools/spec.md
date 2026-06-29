---
name: 420. Xtask Tools
psychevo_self_edit: deny
---

Define Psychevo's repo-owned `xtask` tooling outside CI/CD profiles. This topic
owns local diagnostics and explicit host bootstrap commands that support local
workflows without becoming CI workflow profiles.

## Scope

This topic owns:

- `cargo xtask init dev-env` repo-local development home initialization
- `cargo xtask doctor deps` host dependency checks and explicit installation
- `cargo xtask doctor large-files` repository structure diagnostics
- development/test host prerequisite groups used by visual, browser, and live
  validation workflows
- checkout-local source-install prerequisite diagnostics that mirror the
  standalone product installer without replacing it
- machine-readable and human-readable diagnostic output

Out of scope:

- CI/CD profile selection, run artifact collection, and profile evidence; these
  belong to [410 CI/CD Workflows](../410-ci-cd-workflows/spec.md)
- product diagnostics exposed by `pevo doctor`
- product installation itself; `scripts/install.sh` remains the standalone
  source installer for checkout, clone, and `curl | sh` flows
- installing core language/runtime toolchains such as Rust, Node.js, or pnpm
- hosted CI provider setup

## Init Dev Env Interface

The dev-env initializer is:

- `cargo xtask init dev-env [--home <path>] [--json]`

With no `--home`, it initializes `.local/.psychevo-dev` under the repository
root. It builds the local `pevo` binary and runs `pevo init` with
`PSYCHEVO_HOME` set to the selected dev home. It does not read or copy
credentials from the user's normal home. After initialization, the caller is
responsible for editing `<dev-home>/config.toml` and `<dev-home>/.env`.

The initializer is a checkout-local developer tool. It does not replace
`scripts/install.sh`, and it must not expose live validation selection through
environment-variable knobs.

`cargo xtask live --env shared` uses this dev home as the live runtime home and
state location. `cargo xtask live --env isolated` still reads this dev home's
config and `.env`, but uses per-check runtime home and state paths.

## Doctor Deps Interface

The host dependency interface is:

- `cargo xtask doctor deps check [--only all|core|install|sqlite|vhs|playwright] [--json]`
- `cargo xtask doctor deps install --only all|sqlite|vhs|playwright`

`check` is non-mutating. It reports missing tools and install hints but exits
successfully when dependencies are missing. It exits non-zero only for invalid
arguments or internal errors. `--json` emits structured scope rows with status,
missing commands, and install hints.

`install` is explicit host mutation. It may install packages, configure package
repositories, or install browser dependencies only after the caller selects the
install subcommand. It supports Debian/Ubuntu `apt-get` systems for v1 and
fails with a concise platform message elsewhere.

Dependency groups:

- `core`: `cargo`, `node`, and `pnpm`; checked but never installed by xtask.
- `install`: `git`, `cargo`, a native C compiler/linker exposed as `cc`,
  `gcc`, or `clang`, `node`, and `pnpm`; checked only and used as the complete
  local source-install preflight for developers in a checkout.
- `sqlite`: `sqlite3`.
- `vhs`: `vhs`, `ttyd`, `ffmpeg`, `python3`, and `git`.
- `playwright`: `pnpm` availability plus Playwright Chromium install guidance.
- `all`: development/test dependency groups (`core`, `sqlite`, `vhs`, and
  `playwright`). It intentionally does not include the `install` preflight,
  which is a product installer diagnostic rather than a CI/test prerequisite
  group.

`cargo xtask doctor deps install --only install` must fail with a concise
message explaining that source-install prerequisites must be installed by the
user, platform package manager, or upstream installers. Hosted CI or future
release automation may call the `check --only install` interface, but must not
make the standalone product installer depend on `xtask`.

CI/CD profiles may consume these checks as host prerequisite guidance, but they
must not invoke `install` implicitly. For example, the `visual` CI profile may
fail preflight with a pointer to `cargo xtask doctor deps install --only vhs`.

## Doctor Large Files Interface

The repository large-file diagnostic interface is:

- `cargo xtask doctor large-files [--root <path>]... [--json]`
- `cargo xtask doctor large-files [--root <path>]... --prod-limit <lines> --test-limit <lines> --generated-limit <lines>`

`large-files` is non-mutating. With no `--root` values, it scans `apps`,
`crates`, `packages`, `specs`, and `tools`. It ignores common generated/cache
output directories: `target`, `dist`, `node_modules`, `coverage`,
`test-results`, and `.local`.

The diagnostic classifies files as:

- `generated`: files under a `generated` path segment and direct JSON schema
  files under `packages/protocol/schema/`.
- `test`: files under `tests` or `e2e` path segments, `*.test.*` files, and
  `*.spec.*` files outside the top-level `specs/` tree.
- `production`: all other scanned files.

Default line thresholds mirror [001 Architecture](../001-architecture/spec.md):
`production <= 900`, `test <= 1200`, and `generated <= 900`. Human output keeps
the compact inventory format grouped by category and descending line count.
`--json` emits roots, limits, and oversized file rows for automation. The command
exits zero when no oversized files are found and non-zero when any scanned file
exceeds its category limit.

The command is a manual/local diagnostic in v1. It must not be added to
`changed` or `rust-broad` until the current repository either satisfies the
limits or intentionally adopts a reviewed baseline.

Oversized-file remediation must preserve the module's existing external
interface where callers already have a stable seam. Production files should be
split by semantic responsibilities behind that interface, not by arbitrary line
count chunks. Test files should be split by observable behavior groups and keep
assertions at the public interface. Generated outputs should be reduced by
splitting the generator output or schema grouping that owns them; manual edits
to generated artifacts are not a durable remediation.

Raising thresholds, adding path allowlists, or excluding owned source trees is
not considered remediation unless a spec in the owning topic explicitly adopts
a reviewed baseline for that file family.

## Related Topics

- [410 CI/CD Workflows](../410-ci-cd-workflows/spec.md) defines workflow
  profiles that consume host prerequisites.
- [065 CI/CD](../065-ci-cd/spec.md) defines deterministic workflow and
  artifact rules.
- [001 Architecture](../001-architecture/spec.md) defines the large-file limits.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines product-facing `pevo doctor`.
- [200 pevo install](../200-pevo-cli/install.md) defines the standalone source
  installer whose prerequisites are mirrored by the `install` diagnostic scope.
