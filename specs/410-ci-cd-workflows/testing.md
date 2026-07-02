---
name: 410. CI/CD Workflows Testing
psychevo_self_edit: deny
---

Define deterministic validation for the local CI/CD workflow runner. Tests for
this topic must not require hosted CI providers, live credentials, publishing
permissions, package registries, or external deployment targets.

## Deterministic Coverage

Runner tests should exercise profile listing, plan generation, JSON output,
unknown-profile errors, live opt-in rejection, and artifact-only package
planning without executing heavyweight build steps. Tests should also cover the
default CI artifact retention helper so old numeric run directories are pruned
while non-run entries are left untouched.

Live registry tests should exercise list/plan JSON shape, default smoke
selection, repeated `--check`, repeated `--suite`, `--all` expansion, CLI
provider selection, unknown provider/check rejection, blocked prerequisite
classification, shared/isolated environment-mode planning, and the native
provider-smoke NDJSON verifier.

The workflow runner should keep command planning testable without spawning
subprocesses. Subprocess execution tests should use small deterministic commands
or narrow smoke paths, not the full broad workspace gate.

## Required Narrow Validation

For runner behavior changes:

```sh
cargo test -p psychevo-xtask
```

For command-shape smoke validation:

```sh
cargo xtask ci list --json
cargo xtask ci plan --profile changed --json
cargo xtask ci plan --profile visual --json
cargo xtask live list --json
cargo xtask live plan --json
cargo xtask live plan --env isolated --json
cargo xtask live plan --suite web --json
cargo xtask live plan --all --json
cargo xtask ci plan --profile live --live-env isolated --json
```

For Rust broad gate changes:

```sh
cargo xtask ci run --profile rust-broad
```

## Scenario Coverage

- `ci list` includes `changed`, `rust-broad`, `web`, `visual`, `live`, and
  `package`.
- `ci plan --profile changed --json` emits a parseable plan without executing
  steps.
- `ci run --profile live` fails before provider work unless the caller passes
  explicit live opt-in.
- `ci plan --profile live --json` uses the runner-owned
  `xtask-internal single-provider-live` step rather than a shell helper.
- `live plan` defaults to the `smoke` suite and can expand specific checks,
  repeated suites, or all registered checks.
- Unknown live checks, suites, and providers fail with explicit errors.
- `cargo xtask live run` is live opt-in by command name and does not require an
  extra `--live` flag.
- `live plan --json` defaults to `environment.mode = "shared"`.
- `live plan --env isolated --json` reports isolated mode without changing
  selected checks.
- `ci plan/run --profile live --live-env isolated` mirrors the live registry
  environment mode, and `--live-env` is rejected for non-live profiles.
- `ci plan --profile package --json` marks the profile as artifact-only CD and
  includes no publish, deploy, upload, tag, or hosted-release step.
- No legacy shell entrypoint exists for Rust broad validation; the
  `rust-broad` profile is selected through `cargo xtask ci` directly.
- Default CI artifact retention keeps the 10 newest numeric run directories
  under `.local/.psychevo-dev/ci/` and ignores non-numeric entries.
- `ci plan --profile visual --json` exposes the runner-owned `tui-vhs-demo`
  and `workbench-visual` steps and does not call public shell capture
  entrypoints.
- `live plan --all --json` includes `pevo-acp-server-live`.
- `live plan --suite acp --json` includes `pevo-acp-server-live` alongside the
  registered OpenCode ACP checks.

When VHS dependencies are installed, run `cargo xtask ci run --profile visual`
and review artifacts under `.local/.psychevo-dev/ci/<run-id>/visual/`.
Workbench visual screenshots should be under
`.local/.psychevo-dev/ci/<run-id>/visual/workbench/screenshots/`. When host
prerequisites are missing, report the blocked dependency set instead of
treating the profile as product failure, and point to the relevant
`cargo xtask doctor deps install --only ...` command.

## Broad Validation

When changes touch shared Rust workflow execution or CLI parsing, run the
relevant narrow validation first. Use `cargo xtask ci run --profile rust-broad`
when the change affects Rust workspace confidence or when requested explicitly.

## Related Topics

- [410 CI/CD Workflows](./spec.md) defines the behavior under test.
- [065 CI/CD](../065-ci-cd/spec.md) defines the shared CI/CD foundation.
- [420 Xtask Tools](../420-xtask-tools/spec.md) defines host prerequisite
  diagnostics.
