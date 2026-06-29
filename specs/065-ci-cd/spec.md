---
name: 065. CI/CD
psychevo_self_edit: deny
---

Define the foundation for Psychevo CI/CD: local workflow contracts,
validation profiles, artifact evidence, isolation, and live opt-in boundaries.
This spec owns validation and delivery automation semantics that apply across
topic testing specs, local developer runs, coding-agent repair loops, and future
hosted CI adapters.

CI/CD remains local-first in this slice. Hosted providers such as GitHub
Actions are adapters over these rules, not the source of truth. Continuous
delivery is artifact-only for v1: workflows may build, package, checksum, and
collect reviewable artifacts, but they must not publish public releases,
create hosted draft releases, deploy services, or mutate external channels.

## Scope

- CI/CD vocabulary used by topic testing specs and workflow runners
- deterministic, live opt-in, snapshot, visual, E2E, and package workflow
  boundaries
- local workflow profiles and their artifact/evidence responsibilities
- failure-output expectations for humans and coding agents
- repo-local development home usage for validation and artifact collection
- future hosted CI adapter principles

Out of scope:

- product workflow automations that schedule Psychevo turns
- hosted CI provider workflow files
- public release publishing, package registry upload, update channels, or
  deployment targets
- provider-specific live checks beyond the shared opt-in contract
- topic-specific acceptance matrices and concrete scenario coverage

## Vocabulary

CI workflow is an automated confidence check for repository state. A CI
workflow may run locally or in a hosted adapter, and should be deterministic by
default.

CD workflow is a delivery preparation workflow. In v1, CD is artifact-only:
it may produce build outputs, checksums, logs, and package inspection evidence,
but it must not publish, deploy, or create hosted release objects.

Profile is the named interface a caller selects, such as `changed`,
`rust-broad`, `web`, `visual`, `live`, or `package`. Profiles hide concrete
commands and evidence paths behind stable intent.

Narrow validation is a focused check for a changed subsystem or behavior. It
should minimize runtime and scope while exercising the closest meaningful
contract for the change.

Broad validation is a wider deterministic gate for repository confidence. A
topic may name a broad command, but the workflow runner owns how broad steps
are sequenced, logged, and reported.

Visual validation checks rendered projections such as screenshots, terminal
captures, traces, or browser snapshots. Visual evidence is valuable for user
experience regressions, but structured facts remain the preferred truth source
when available.

Live opt-in validation uses real providers, real credentials, live network
services, or other host/user-specific resources. Live workflows must never run
as part of deterministic default profiles. Live workflow selection belongs to a
repo-owned runner command, not ad hoc shell scripts, ignored-test incantations,
or live-specific environment-variable flags.

## Workflow Contracts

Workflow runners expose a small interface:

- list available profiles
- plan a selected profile without running it
- run a selected profile and collect evidence

The workflow runner owns command sequencing, per-step status, failure capture,
artifact paths, live opt-in enforcement, and serialization of steps that cannot
safely run together. Topic specs own acceptance scenarios and the closest
meaningful narrow commands.

Successful routine output should stay compact. Workflow runners should capture
normal stdout progress in logs without mirroring it to the terminal. Stdout
warning lines should still be mirrored as terminal diagnostics. Stdout error
lines should not be mirrored by default; errors should appear through stderr to
avoid duplicate diagnostics. Stderr is diagnostic output and should be mirrored
to the terminal while also being captured in logs.

Failing steps must preserve the command, exit status, log path, and bounded
captured output for a human or coding agent to start the next repair loop
without rerunning blindly.

Default workflows should avoid hidden dependencies on real user config,
credentials, persistent host state, global mocks, wall-clock timing, open
sockets, live services, and provider availability unless the selected profile
explicitly opts into them.

## Evidence And Artifacts

Structured truth comes first. When determining state, outcome, or semantic
correctness, workflows should prefer structured events, durable evidence,
database facts, result objects, machine-readable diffs, and explicit exit
status over terminal prose or rendered text.

Rendered terminal output, screenshots, pane captures, ANSI captures, visual
snapshots, and product screenshots are projection evidence. They should not be
the only source of truth when structured facts are available.

The repository-local development home is `.local/.psychevo-dev/` under the repo
root. CI/CD workflows may use it for isolated config, `.env`, SQLite state,
logs, sessions, snapshots, TUI/Web artifacts, live-validation cwds, and
workflow artifacts. It is an opt-in development convention, not a product
default profile and not a replacement for `~/.psychevo`.

Default CI workflow artifacts under `.local/.psychevo-dev/ci/` are retained for
only the most recent 10 numeric run directories. Cleanup is repository-local,
best-effort housekeeping; failures should be surfaced as warnings without
masking the selected profile's result. Explicit caller-provided artifact roots
are caller-owned and should not be pruned by the default retention policy.

Commands and scripts that rely on this home must set `PSYCHEVO_HOME`
explicitly. When a fixed config file should be used, they should set
`PSYCHEVO_CONFIG=.local/.psychevo-dev/config.toml`; when validation state should
be isolated from the dev home's normal state, they should set `PSYCHEVO_DB`
explicitly. Scripts must not automatically copy credentials from the user's
normal home or external auth stores into this directory.

Live validation artifacts use `.local/.psychevo-dev/ci/<run-id>/live/<check-id>/`
by default. The live runner must create per-check state directories and result
JSON so failed, blocked, and passed checks are reviewable without rerunning.
Provider choice, selected suites/checks, and artifact roots are command-line
runner inputs. Compatibility environment variables previously used as public
live selection knobs must not be part of the contract.

Live validation may run in a shared or isolated environment mode. Shared mode
uses the repo-local development home and state DB so validation can surface
problems caused by realistic persisted home state. Isolated mode still reads
configuration and credentials from the repo-local development home, but runs
with per-check home and state paths under the live artifact root. In both modes,
logs, result JSON, screenshots, and context files remain per-run/per-check
artifacts.

Substantial structured-output parsing belongs in repo-local helper programs or
the workflow runner, not long inline shell heredocs. Named CI/CD profiles
belong behind the workflow runner interface, not shell-script compatibility
entrypoints. Shell scripts may still own specialized fixture setup, environment
selection, and process wiring when a profile step delegates to them.

## Delivery Boundaries

Artifact-only CD may produce local build outputs, package trees, checksums,
manifests, logs, and inspection reports. It must keep those artifacts under an
explicit local artifact root and report the paths.

Artifact-only CD must not publish packages, push tags, create hosted release
objects, upload assets to release services, deploy Web assets, modify update
channels, or contact package registries except for ordinary dependency
resolution needed by the local build.

Hosted CI/CD adapters may be added later. They should call the same workflow
profiles where practical and preserve the local runner's evidence model instead
of inventing a second source of truth in provider-specific YAML.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream principle
  that execution leaves evidence.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable
  evidence semantics for inspectable agent-invocation facts.
- [060 Automation](../060-automation/spec.md) defines product workflow
  automation foundations.
- [070 Experience](../070-experience/spec.md) defines UX and DX defaults that
  CI/CD output and diagnostics should support.
- [410 CI/CD Workflows](../410-ci-cd-workflows/spec.md) defines the concrete
  local workflow runner surface.
