---
name: 330. Benchmark Integrations Testing
psychevo_self_edit: deny
---

Define deterministic validation for benchmark integrations.

## Scope

- local task-set loading
- Harbor sample import
- SWE-bench sample import
- official bridge gating

Out of scope:

- real Harbor jobs
- real SWE-bench downloads or Docker harness execution
- live agent/provider execution

## Deterministic Coverage

Local integration tests should load v5 `peval_agent` task directories from
generated local projects, execute verifier checks, and import evaluator
results.

At least one local integration test should cover the full path from compact
manifest loading through fake candidate execution, evaluator scoring, artifact
writing, and view rendering.

Harbor tests should use stored sample metadata and result payloads that cover
successful import, harness failure, missing artifact, unsupported schema, and
skipped task behavior.

SWE-bench tests should use local miniature repositories or synthetic sample
payloads. They should cover base-state preparation, temporary patch generation
for scoring, evaluator import, and confirmation that patch artifacts are not
retained by default.

Tau2 tests should use local dry-run ACP/MCP fixtures with isolated per-case
state. They must not contact a live Tau2 service during default validation.

Official bridge tests that contact real registries, Hugging Face datasets,
Docker harnesses, or network services must be explicitly gated outside the
default validation path.

## Related Topics

- [330 Local](local.md)
- [330 Harbor](harbor.md)
- [330 SWE-bench](swe-bench.md)
- [350 Coding Evaluation Testing](../350-coding-evaluation/testing.md)
