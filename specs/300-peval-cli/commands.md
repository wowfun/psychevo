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

The first `peval` implementation exposes all seven top-level commands:
`doctor`, `list`, `check`, `run`, `report`, `compare`, and `replay`. Commands
accept a directory evaluation project rooted at `eval.toml`.

`peval doctor` inspects local readiness. It checks installed commands,
configured sidecar support, known agent preset readiness, Docker availability
when requested, provider credential allowlists, and cache/output writability.
It does not execute benchmark tasks.

`peval list` enumerates discoverable suites, adapters, presets, reports, or
artifacts from configured locations. Listing is observational and must not
download official datasets unless the user explicitly asks for remote refresh
behavior in a later spec.

`peval check` validates manifests and suite structure. It expands factors,
validates schema versions, verifies scorer declarations, checks output paths,
and resolves adapter readiness far enough to report setup problems. It is the
default command for CI and local spec conformance because it stays offline.

`peval run` executes an expanded matrix. It records every case outcome,
continues after per-case setup/runtime/scoring failures, and writes structured
artifacts before generating optional report output.

Run artifacts default to `target/peval/runs/<run-id>` under the project root.
The run id is generated when omitted and may be supplied explicitly for
deterministic local validation. `peval run` exits successfully only when all
expanded cases pass; failure details remain available in structured artifacts
and JSON or human output.

`peval report` reads existing artifacts and renders reports. It never reruns
agents or scorers.

`peval compare` compares two or more existing run artifact roots. Comparison
uses structured summaries and case results, not ad hoc parsing of human logs.
It is artifact-only and never runs setup, agents, or scorers.

`peval replay` reads trajectories and artifacts to reconstruct execution for
diagnosis. Replay is artifact-only and observational; it must not re-execute
the agent or scorer.

## Machine Output

Commands that support machine output should emit one parseable stream or
document per invocation. Error JSON uses a stable error type and message, and
should include the command phase when the failure came from readiness, manifest
validation, execution, scoring, report rendering, or artifact loading.

## Related Topics

- [300 Reporting](reporting.md)
- [095 Manifest](../095-evaluation-framework/manifest.md)
- [095 Execution](../095-evaluation-framework/execution.md)
