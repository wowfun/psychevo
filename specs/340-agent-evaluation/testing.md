---
name: 340. Agent Evaluation Testing
psychevo_self_edit: deny
---

Define deterministic validation for agent integration specs.

## Scope

- agent preset readiness behavior
- manifest override behavior
- collector normalization tests
- fairness and isolation checks

Out of scope:

- real Psychevo, OpenCode, or Hermes live benchmark execution
- provider API calls
- third-party package installation

## Deterministic Coverage

Tests should use fake commands and fake native adapters that mimic each
integration's observable behavior. They should prove that presets resolve to
manifest-equivalent settings and that manifest overrides take precedence.

Readiness tests should cover missing command, failing command, missing
collector source, unsupported model mapping, and unavailable credential
allowlist entries.

Collector tests should normalize representative Psychevo, OpenCode, and Hermes
events or exports into canonical trajectory events while preserving lossy
diagnostics when source data is incomplete.

Isolation tests should verify that temporary config roots and allowlisted
environment variables are used instead of the user's normal home state.

## Related Topics

- [340 Psychevo](psychevo.md)
- [340 OpenCode](opencode.md)
- [340 Hermes](hermes.md)
- [355 Coding Fixtures](../355-coding-fixtures/spec.md)
