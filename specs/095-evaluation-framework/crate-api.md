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
- extension points for adapters and view renderers

Out of scope:

- exact symbol names for the first implementation
- CLI argument parsing
- benchmark-specific field inventories

## Crate Boundary

`psychevo-eval` is a Rust workspace crate that exposes a library API and also
hosts the `peval` binary entrypoint. Library modules must be usable without
spawning the CLI.

The core library surface should include types or traits for:

- service context, service capabilities, and structured diagnostics
- manifests and schema versions
- task sets, tasks, candidates, factors, cases, and attempts
- runner requests and cell execution summaries
- environment providers
- agent adapters
- benchmark adapters
- evaluators
- collectors and trajectory events
- artifact writers and view inputs

The service facade is the preferred public API for product callers. Direct
runner and renderer helper functions are implementation details unless a later
spec explicitly promotes them. The current service surface exposes concrete
request and DTO types for workspace initialization, registry resolution,
readiness/list/check/run flows, datasets, and dynamic views.

Persisted artifact types and view DTO types are distinct. Artifact documents are
the physical source of truth. View DTOs are logical projections for CLI and
future local Web surfaces and carry their own schema version.

The public API is stable-priority. The first implementation may mark specific
modules experimental, but persisted schema and core type meanings should be
versioned instead of renamed casually.

## Dependency Direction

The framework core must not depend on `psychevo-runtime`, `psychevo-cli`, or
product CLI modules. A Psychevo native adapter may depend on `psychevo-runtime`
through an explicit feature or module boundary.

The framework may use workspace dependencies for serialization, TOML/JSON
handling, async execution, temporary directories, process management, and
view generation when those dependencies do not force runtime or CLI coupling.

## API Behavior

Library callers should be able to:

- load and validate manifests
- expand a matrix without executing candidates
- run deterministic local cases with fake adapters
- write artifacts to a caller-selected output root
- read artifacts for view rendering and comparison
- render HTML, Markdown, and JSON views from stored artifacts
- compare stored cell facts without executing agents or evaluators
- build typed view models over existing cells without reading raw trajectory or
  log bodies by default
- load report profiles from eval, workspace, and user config layers
- derive bounded timelines and ATIF trajectories from local trajectory event
  streams on demand
- serve local read-only viewer APIs over cell facts and explicit diagnostic
  artifact reads
- run explicit cached analysis actions through configured peval agents when a
  caller grants execute/write capabilities

APIs that can execute live agents, contact providers, download official
benchmark data, or spawn sidecar processes must make that behavior explicit in
their request options.

## Related Topics

- [095 Manifest](manifest.md)
- [095 Execution](execution.md)
- [090 Schema](../090-evaluation/schema.md)
