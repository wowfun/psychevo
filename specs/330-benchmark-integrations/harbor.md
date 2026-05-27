---
name: 330. Harbor Benchmark Integration Attachment
psychevo_self_edit: deny
---

Define Harbor/Terminal-Bench style benchmark integration.

This attachment is part of [330 Benchmark Integrations](spec.md).

## Scope

- Harbor registry and task metadata bridge
- official harness delegation
- artifact import
- small-sample defaults

## Bridge Shape

The Harbor bridge reads official registry or task metadata through the official
tooling path when available. It translates task instructions, environment
requirements, timeouts, and scoring hooks into coding task cases.

Environment setup and scoring may be delegated to Harbor when that preserves
official compatibility. `psychevo-eval` remains responsible for agent
selection, matrix expansion, output roots, artifact import, and report inputs.

## Results

Harbor job artifacts and evaluator outputs are imported into canonical case
results. Native Harbor logs may be retained as diagnostic artifacts, but report
generation uses the imported structured result.

Real Harbor execution is an opt-in integration path. Default validation uses
fixture payloads that resemble Harbor outputs without downloading official
tasks or starting real harness jobs.

## Related Topics

- [095 Official Bridges](../095-evaluation-framework/official-bridges.md)
- [095 Sidecar](../095-evaluation-framework/sidecar.md)
- [330 Testing](testing.md)
