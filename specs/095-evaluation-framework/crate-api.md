---
name: 095. Evaluation Framework Crate API Attachment
psychevo_self_edit: deny
---

Define public library API expectations for `psychevo-eval`.

This attachment is part of [095 Evaluation Framework](spec.md).

## Scope

- crate boundary
- stable public type categories
- runtime dependency boundary
- extension points for adapters and reporters

Out of scope:

- exact symbol names for the first implementation
- CLI argument parsing
- benchmark-specific field inventories

## Crate Boundary

`psychevo-eval` is a Rust workspace crate that exposes a library API and also
hosts the `peval` binary entrypoint. Library modules must be usable without
spawning the CLI.

The core library surface should include types or traits for:

- manifests and schema versions
- suites, tasks, candidates, factors, cases, and attempts
- runner requests and run results
- environment providers
- agent adapters
- benchmark adapters
- scorers
- collectors and trajectory events
- artifact writers and report inputs

The first implementation exposes concrete public types for the controlled
vertical slice: `EvalProject`, `SuiteManifest`, `AgentManifest`,
`TaskManifest`, `RunRequest`, `RunSummary`, `CaseResult`, `ScoreResult`,
`TrajectoryEvent`, `ReportRequest`, `CompareRequest`, and `ReplayRequest`.
Additional helper types may remain module-private until another adapter or
benchmark bridge needs them.

The public API is stable-priority. The first implementation may mark specific
modules experimental, but persisted schema and core type meanings should be
versioned instead of renamed casually.

## Dependency Direction

The framework core must not depend on `psychevo-runtime`, `psychevo-cli`, or
product CLI modules. A Psychevo native adapter may depend on `psychevo-runtime`
through an explicit feature or module boundary.

The framework may use workspace dependencies for serialization, TOML/JSON
handling, async execution, temporary directories, process management, and
report generation when those dependencies do not force runtime or CLI coupling.

## API Behavior

Library callers should be able to:

- load and validate manifests
- expand a matrix without executing candidates
- run deterministic local cases with fake adapters
- write artifacts to a caller-selected output root
- read artifacts for reporting or comparison
- render HTML, Markdown, and JSON reports from stored artifacts
- compare stored run artifact roots without executing agents or scorers
- replay stored trajectory events without re-running a case

APIs that can execute live agents, contact providers, download official
benchmark data, or spawn sidecar processes must make that behavior explicit in
their request options.

## Related Topics

- [095 Manifest](manifest.md)
- [095 Execution](execution.md)
- [090 Schema](../090-evaluation/schema.md)
