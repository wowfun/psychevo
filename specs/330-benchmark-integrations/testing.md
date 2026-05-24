---
name: 330. Benchmark Integrations Testing
psychevo_self_edit: deny
---

Define deterministic validation for benchmark integrations.

## Scope

- local suite loading
- Harbor fixture import
- SWE-bench fixture import
- official bridge gating

Out of scope:

- real Harbor jobs
- real SWE-bench downloads or Docker harness execution
- live agent/provider execution

## Deterministic Coverage

Local integration tests should load task directories and JSONL prompt sources
from fixtures, execute fake setup/scorer commands, and import scorer JSON.

Harbor tests should use stored fixture metadata and result payloads that cover
successful import, harness failure, missing artifact, unsupported schema, and
skipped task behavior.

SWE-bench tests should use local miniature repositories or synthetic fixture
payloads. They should cover base-state preparation, temporary patch generation
for scoring, scorer import, and confirmation that patch artifacts are not
retained by default.

Official bridge tests that contact real registries, Hugging Face datasets,
Docker harnesses, or network services must be explicitly gated outside the
default validation path.

## Related Topics

- [330 Local](local.md)
- [330 Harbor](harbor.md)
- [330 SWE-bench](swe-bench.md)
- [355 Coding Fixtures](../355-coding-fixtures/spec.md)
