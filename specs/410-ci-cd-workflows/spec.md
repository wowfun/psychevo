---
name: 410. CI/CD Workflows
psychevo_self_edit: deny
---

Define Psychevo's concrete local CI/CD workflow runner. This topic implements
the provider-neutral CI/CD foundation from [065 CI/CD](../065-ci-cd/spec.md)
through repo-local `xtask` commands and keeps hosted CI provider files out of
scope for v1.

## Scope

This topic owns:

- local `cargo xtask ci` command behavior
- local `cargo xtask live` command behavior
- named workflow profiles and their planned steps
- local artifact root conventions for workflow runs
- live opt-in enforcement for workflow profiles
- lower-level helper scripts used by profile steps

Out of scope:

- GitHub Actions or other hosted CI provider workflow files
- public release publishing, hosted draft releases, deployments, update
  channels, or package registry upload
- user-customizable workflow manifests
- replacing topic-specific testing specs or acceptance matrices

## Runner Interface

The local runner exposes:

- `cargo xtask ci list`
- `cargo xtask ci plan --profile <profile>`
- `cargo xtask ci run --profile <profile>`

`list` prints available profiles. `plan` prints the ordered steps without
executing them. `run` executes the selected profile, reports compact progress,
and preserves step output in logs.

During `run`, normal stdout progress is captured to the step log without being
mirrored to the terminal. Stdout warning lines are mirrored to terminal
diagnostics. Stdout error lines are not mirrored by default; errors should
surface through stderr to avoid duplicate diagnostics. Stderr is mirrored to the
terminal and also captured in the step log. Failed steps report the log path and
a bounded log tail rather than dumping unbounded output.

All commands accept `--json` for machine-readable output. JSON output must
include profile ids, profile descriptions, step ids, command arrays, live
flags, artifact roots when available, and per-step status for executed runs.

The live registry exposes:

- `cargo xtask live list`
- `cargo xtask live plan [--env shared|isolated] [--check <id>]... [--suite <suite>]... [--all]`
- `cargo xtask live run [--env shared|isolated] [--check <id>]... [--suite <suite>]... [--all]`

`cargo xtask live run` is itself explicit live opt-in and does not require an
extra `--live` flag. With no selection, it runs the `smoke` suite. Provider
selection is a repeatable `--provider <id>` command-line argument; v1 supports
`xiaomi-token-plan` and `deepseek`, with `xiaomi-token-plan` as the default.
Live selection must not depend on public live-specific environment variables.
The generic CI profile remains guarded: `cargo xtask ci run --profile live`
must fail before provider work unless the caller also passes `--live`.
`cargo xtask ci plan --profile live` and `cargo xtask ci run --profile live
--live` accept `--live-env shared|isolated` and default to `shared`.

## Profiles

Initial profiles:

- `changed`: lightweight local confidence for the current checkout; v1 plans
  format checking and lets future work add diff-aware selection.
- `rust-broad`: Rust workspace broad gate; runs format, clippy, and tests.
- `web`: Workbench build, tests, and typecheck.
- `visual`: deterministic visual diagnostics using fake/local providers; v1
  owns the TUI/VHS capture workflow directly in `xtask`.
- `live`: opt-in live validation using explicit provider credentials.
- `package`: artifact-only CD profile that builds local reviewable artifacts
  and checksums without publishing or creating hosted release objects.

Workflow definitions are code-owned in `xtask` for v1. Do not add a public
TOML/YAML manifest until there are multiple real adapters or external
customization needs.

## Live Registry

Registered live checks:

- `provider-smoke`: native `xtask` provider smoke with two `pevo run --format
  json --include-reasoning` turns, `read` tool verification, `--continue`
  thread reuse verification, and token final-answer verification.
- `pevo-doctor-live`: `pevo doctor --live --json`.
- `runtime-provider-read`: runtime ignored live provider read-tool check.
- `runtime-model-fetch`: runtime ignored Xiaomi `/models` fetch/cache check.
- `gateway-automation-live`: gateway automation ignored live check.
- `web-composer-live`: Workbench real-provider composer check.
- `web-automation-live`: Workbench GUI automation live check.
- `web-subagent-live`: Workbench live subagent GUI check.
  The Workbench web live checks live in
  `apps/workbench/e2e/workbench.live.spec.ts`; the live registry must track
  that file when Workbench deterministic specs are split or renamed.
- `web-skill-live`: Workbench live-skill flow.
  Completion checks for live skill flows must scope running/streaming DOM state
  to the active Transcript region so shell, sidebar, or history running
  affordances cannot mask a completed assistant response.
- `opencode-acp-gui-live`: OpenCode ACP GUI live flow.
- `opencode-acp-delegate-live`: `@opencode` delegate live flow.

Suites:

- `smoke`: `provider-smoke`.
- `provider`: `provider-smoke`, runtime provider/catalog checks, and doctor
  live.
- `web`: Workbench composer, automation, and subagent live checks.
- `skill`: live skill check.
- `acp`: OpenCode ACP live checks.
- `automation`: gateway automation and Web automation live checks.
- `all`: all registered checks.

The live runner owns provider/model resolution, dev-home initialization checks,
artifact paths, environment-mode resolution, `PSYCHEVO_HOME`/`PSYCHEVO_CONFIG`/
`PSYCHEVO_DB` injection, and any implementation-only context files passed to
test harnesses. Missing host tools, missing fixture directories, missing config,
or missing credentials are reported as `blocked`, not silent skips.

Live environment modes:

- `shared` is the default. It sets `PSYCHEVO_HOME` to `.local/.psychevo-dev`,
  `PSYCHEVO_CONFIG` to `.local/.psychevo-dev/config.toml`, and `PSYCHEVO_DB` to
  `.local/.psychevo-dev/state.db`.
- `isolated` uses the same dev-home config and `.env`, but sets
  `PSYCHEVO_HOME` and `PSYCHEVO_DB` to per-check paths under
  `.local/.psychevo-dev/ci/<run-id>/live/<check-id>/`.

Plan and run JSON must include the selected environment mode. Run JSON must
also include the effective home, config, and DB paths for each check.

## Artifacts And Isolation

Workflow artifacts live under `.local/.psychevo-dev/ci/<run-id>/` unless the
caller selects an explicit artifact root. The runner creates separate output
paths for plans, step logs, package artifacts, checksums, and live/visual
diagnostics when those workflows run.

After a default-artifact-root run finishes or fails after creating its run
directory, the runner prunes `.local/.psychevo-dev/ci/` to the 10 most recent
numeric run directories. Non-numeric entries are ignored, and explicit
`--artifact-root` paths are not pruned.

The runner must set repo-local paths explicitly for steps that rely on
Psychevo state. Live profiles must not infer credentials from the user's normal
home, and must fail before running provider calls unless live execution is
explicitly allowed.

The `package` profile is artifact-only CD. It may build and checksum local
artifacts, but must not publish, deploy, tag, push, upload release assets,
create hosted draft releases, or mutate package registries.

## Script Adapters

Named CI/CD profiles must not be exposed through shell script adapters. Human,
agent, and future hosted-provider callers use `cargo xtask ci` directly so
there is one source of truth for profile selection, artifact-root reporting,
failure capture, and live opt-in policy.

Scripts that own specialized fixtures may remain callable as lower-level step
implementations when there is no native replacement yet. They are not CI/CD
profile entrypoints. TUI/VHS capture and live provider smoke are runner-owned
and are not exposed through public shell scripts.

Host prerequisite installation is not a CI/CD profile. The `visual` profile may
fail fast when VHS host tools are missing and point to `cargo xtask doctor deps
install --only vhs`, but it must not install packages implicitly.

## Related Topics

- [065 CI/CD](../065-ci-cd/spec.md) defines the shared CI/CD foundation.
- [060 Automation](../060-automation/spec.md) defines product automation
  foundations, which are separate from CI/CD workflows.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines user-facing CLI behavior.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines terminal visual surfaces used
  by the `visual` profile.
- [240 pevo Web](../240-pevo-web/spec.md) defines Workbench surfaces used by
  the `web` and `visual` profiles.
- [420 Xtask Tools](../420-xtask-tools/spec.md) defines host prerequisite
  diagnostics and explicit installation.
