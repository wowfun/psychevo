---
name: 095. Evaluation Framework Official Bridges Attachment
psychevo_self_edit: deny
---

Define how `psychevo-eval` integrates with official benchmark APIs, registries,
datasets, and harnesses.

This attachment is part of [095 Evaluation Framework](spec.md).

## Scope

- official benchmark integration mode
- Rust, CLI, and Python bridge responsibilities
- canonical result import requirements
- default sample-size behavior

Out of scope:

- coding-specific official benchmark fields
- exact Harbor or SWE-bench command lines
- downloading benchmark data during default tests

## Integration Mode

Official benchmark integrations use a hybrid model. The framework may read
official dataset or registry metadata directly, while environment construction,
execution, or scoring may be delegated to official harnesses when that improves
compatibility.

Delegation does not make official harness output canonical by itself. The
bridge must import official results into the framework's structured run, case,
score, and artifact model.

## Dependency Mode

Official integrations may use both:

- a CLI bridge to official commands or harnesses
- the optional Python sidecar for dataset loading, harness orchestration, and
  richer analysis outputs

Rust remains the orchestrator for manifest validation, matrix expansion,
adapter selection, artifact placement, and report input generation.

## Defaults

Real official benchmark data access is not part of default deterministic
validation. When a user requests a real official benchmark run, the framework
defaults to a small sample or explicit task limit unless the manifest or CLI
explicitly selects a full split.

## Related Topics

- [095 Sidecar](sidecar.md)
- [330 Harbor](../330-benchmark-integrations/harbor.md)
- [330 SWE-bench](../330-benchmark-integrations/swe-bench.md)
