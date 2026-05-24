---
name: 095. Evaluation Framework
psychevo_self_edit: deny
---

# 095. Evaluation Framework

Define the `psychevo-eval` framework layer. This topic turns the evaluation
foundation contracts into a reusable library and adapter framework without
owning command-line product behavior or coding-specific benchmark semantics.

## Scope

- `psychevo-eval` crate responsibility and public API expectations
- framework manifests and execution orchestration
- optional Python sidecar boundary
- official benchmark bridge strategy
- validation expectations for framework users

Out of scope:

- `peval` command spelling and output; see [300 peval CLI](../300-peval-cli/spec.md)
- coding task families and coding scorers; see
  [350 Coding Evaluation](../350-coding-evaluation/spec.md)
- concrete Psychevo, OpenCode, Hermes, Harbor, or SWE-bench adapter details
- runtime-owned agent loop behavior

## Framework Contract

`psychevo-eval` is the neutral evaluation framework for Psychevo. It owns the
library API, manifest parsing, run orchestration, adapter contracts, artifact
writing, and bridge points needed to evaluate multiple agents against multiple
benchmark sources.

The framework core must remain independent of `psychevo-runtime`. Runtime
integration belongs behind explicit adapter modules or features so the
evaluation crate can evaluate non-Psychevo agents without pulling runtime
concerns into the foundation model.

## Validation

The framework should be validated through deterministic fake agents, fake
benchmarks, schema roundtrips, fixture sidecar payloads, and local scorer
fixtures. Real provider calls, live agents, official benchmark downloads, and
external harness execution are opt-in validation paths owned by concrete CLI or
domain specs.

## Attachments

- [Crate API](crate-api.md) defines stable public library surface expectations.
- [Manifest](manifest.md) defines suite, agent, run, and factor manifest
  contracts.
- [Execution](execution.md) defines framework orchestration, environment, and
  failure handling.
- [Sidecar](sidecar.md) defines optional Python sidecar boundaries.
- [Official Bridges](official-bridges.md) defines official benchmark bridge
  strategy.

## Related Topics

- [090 Evaluation](../090-evaluation/spec.md)
- [090 Adapters](../090-evaluation/adapters.md)
- [300 peval CLI](../300-peval-cli/spec.md)
- [350 Coding Evaluation](../350-coding-evaluation/spec.md)
