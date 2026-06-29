---
name: 420. Xtask Tools Testing
psychevo_self_edit: deny
---

Define validation for repo-owned `xtask` tooling outside CI/CD profiles.

## Required Validation

For `doctor deps` implementation changes:

```sh
cargo test -p psychevo-xtask
cargo xtask doctor deps check --only install --json
cargo xtask doctor deps check --only vhs --json
```

For dev-env initializer changes:

```sh
cargo test -p psychevo-xtask
cargo xtask init dev-env
```

For changes that affect CI visual prerequisite messaging:

```sh
cargo xtask ci plan --profile visual --json
```

For `doctor large-files` implementation changes:

```sh
cargo test -p psychevo-xtask
cargo xtask doctor large-files --root xtask --json
```

## Scenario Coverage

- `doctor deps check --json` emits parseable dependency rows without mutating
  host state.
- `init dev-env` resolves the default repo-local home, runs `pevo init` with
  explicit `PSYCHEVO_HOME`, and reports the config and `.env` paths the caller
  must prepare manually.
- Missing dependency reports include an install hint for the selected group.
- `doctor deps check --only install --json` reports `git`, `cargo`,
  `cc|gcc|clang`, `node`, and `pnpm` readiness for source installs without
  mutating host state.
- `doctor deps install --only core` is rejected because xtask does not install
  Rust, Node.js, or pnpm.
- `doctor deps install --only install` is rejected because xtask does not
  bootstrap the standalone product install prerequisites.
- Debian/Ubuntu install planning covers `sqlite`, `vhs`, and `playwright`
  without being exercised by deterministic tests.
- The visual CI profile missing-dependency message points to
  `cargo xtask doctor deps install --only vhs`.
- `doctor large-files` scans default roots including `tools`, classifies
  production, test, and generated files, ignores build/cache directories, emits
  stable JSON, and returns non-zero when oversized files are found.
- `doctor large-files` is not part of `changed` or `rust-broad` in v1 because
  the current inventory is a manual architecture diagnostic until the
  repository has no oversized files or a reviewed baseline.

## Related Topics

- [420 Xtask Tools](./spec.md) defines the behavior under test.
- [410 CI/CD Workflows](../410-ci-cd-workflows/spec.md) defines the visual
  workflow that consumes VHS host prerequisites.
