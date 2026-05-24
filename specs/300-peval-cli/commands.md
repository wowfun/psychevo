---
name: 300. peval CLI Commands Attachment
psychevo_self_edit: deny
---

Define command behavior for the `peval` binary.

This attachment is part of [300 peval CLI](spec.md).

## Scope

- command family purposes
- offline versus execution behavior
- high-consequence command boundaries
- expected structured output posture

## Commands

The first `peval` implementation exposes `init`, `doctor`, `list`, `check`,
`run`, `report`, `compare`, `replay`, and `dataset`. Commands that need an
evaluation config accept `--config/-c <path-to-eval.toml>` and otherwise
discover `eval.toml` from process cwd. The former `--project` selector is not
part of the public surface.

`peval init [--root <dir>] [--force]` creates user-level evaluation store
configuration at `$PSYCHEVO_HOME/peval.toml`. It defaults the root to
`$HOME/.local/evals`, writes an absolute root path, creates the store skeleton,
and is independent of `pevo init`. Re-running against the same root is
idempotent; changing the root requires `--force`.

`peval doctor` inspects local readiness. It checks installed commands,
configured sidecar support, known agent preset readiness, Docker availability
when requested, provider credential allowlists, and cache/output writability.
It does not execute benchmark tasks.

`peval list` enumerates discoverable suites, adapters, presets, reports, runs,
datasets, or artifacts from configured locations. Listing is observational and
must not download official datasets unless the user explicitly asks for remote
refresh behavior in a later spec.

`peval check` validates manifests and suite structure. It expands factors,
validates schema versions, verifies scorer declarations, checks output paths,
and resolves adapter readiness far enough to report setup problems. It is the
default command for CI and local spec conformance because it stays offline.

`peval run` executes an expanded matrix. It records every case outcome,
continues after per-case setup/runtime/scoring failures, and writes structured
artifacts before generating optional report output.

Run artifacts default to `<peval-root>/<namespace>/<run-id>` under the
persistent evaluation store. The namespace comes from the evaluation config's
safe store-relative `output_root`, or defaults to `runs/<project-slug>`. The run
id is generated when omitted and may be supplied explicitly for deterministic
local validation. `peval run` exits successfully only when all expanded cases
pass; failure details remain available in structured artifacts and JSON or
human output.

`peval report` reads existing artifacts and renders reports. It accepts an
explicit run artifact root, a store selector, or `latest`. `latest` resolves
through the persistent store and may be filtered by suite, agent, and run
status. When an evaluation config is available, `latest` is scoped to that
config's namespace; otherwise it is global across the store. It never reruns
agents or scorers.

`peval compare` compares two or more existing run artifact roots or store
selectors. Comparison uses structured summaries and case results, not ad hoc
parsing of human logs. It is artifact-only and never runs setup, agents, or
scorers.

`peval replay` reads trajectories and artifacts to reconstruct execution for
diagnosis. It accepts the same explicit-root and store-selector inputs as
`report`. Replay is artifact-only and observational; it must not re-execute
the agent or scorer.

`peval dataset import <path>` registers a local benchmark payload in the
persistent store. The first implementation records a manifest and references or
links the source payload; it does not copy large datasets by default and does
not download official benchmark data.

`peval list --kind runs` and `peval list --kind datasets` are store-only and do
not require an evaluation config. Suite, agent, task, and all-listing modes need
an evaluation config because they report manifest-defined inventory.

## Machine Output

Commands that support machine output should emit one parseable stream or
document per invocation. Error JSON uses a stable error type and message, and
should include the command phase when the failure came from readiness, manifest
validation, execution, scoring, report rendering, or artifact loading.

## Related Topics

- [300 Reporting](reporting.md)
- [095 Manifest](../095-evaluation-framework/manifest.md)
- [095 Execution](../095-evaluation-framework/execution.md)
