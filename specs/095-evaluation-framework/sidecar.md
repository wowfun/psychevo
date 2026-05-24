---
name: 095. Evaluation Framework Sidecar Attachment
psychevo_self_edit: deny
---

Define the optional Python sidecar boundary for `psychevo-eval`.

This attachment is part of [095 Evaluation Framework](spec.md).

## Scope

- optional sidecar role
- dependency and readiness behavior
- communication and artifact boundaries
- cache ownership

Out of scope:

- Python package implementation details
- benchmark-specific harness commands
- report frontend design

## Role

The sidecar exists to use the Python benchmark ecosystem where it gives better
coverage than reimplementing everything in Rust. It may load official datasets,
call official harnesses, normalize official outputs, and generate richer report
data.

The Rust core must compile and run deterministic local framework tests without
the sidecar installed.

## Dependency Management

The framework treats Python, `uv`, benchmark packages, and sidecar dependencies
as optional managed capabilities. Readiness checks report missing dependencies
with clear installation or configuration guidance.

Sidecar availability must not be required for local fake suites, manifest
validation, artifact reading, or basic report rendering.

## Interface

Rust-to-sidecar communication must use versioned structured payloads. The
sidecar may be invoked as an external process or through a configured command,
but its outputs must be imported into framework result, task, and trajectory
models before they become canonical artifacts.

Sidecar logs are diagnostic artifacts. They are not the source of truth for
scoring unless a benchmark bridge imports them into structured result records.

## Cache

Sidecar downloads and generated benchmark caches should live in a user cache
location, separate from per-run artifacts. The selected cache path must be
visible in diagnostics and should be overrideable by callers.

## Related Topics

- [095 Official Bridges](official-bridges.md)
- [300 Commands](../300-peval-cli/commands.md)
- [330 Benchmark Integrations](../330-benchmark-integrations/spec.md)
