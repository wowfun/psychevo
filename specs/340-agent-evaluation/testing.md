---
name: 340. Agent Evaluation Testing
psychevo_self_edit: deny
---

Define deterministic validation for agent integration specs.

## Scope

- agent preset readiness behavior
- manifest override behavior
- collector normalization tests
- deterministic fake candidate behavior
- fairness and isolation checks

Out of scope:

- real Psychevo, OpenCode, or Hermes live benchmark execution
- provider API calls
- third-party package installation

## Deterministic Coverage

Tests should use fake commands and fake native adapters that mimic each
integration's observable behavior. They should prove that presets resolve to
manifest-equivalent settings and that manifest overrides take precedence.

Fake candidates should model observable agent behavior without calling real
providers. They can edit task workspaces, emit trajectory events, simulate tool
calls, return final answers, time out, or fail intentionally. The behavior must
be deterministic from manifest input and must not read user config, require API
keys, use current time for assertions, or depend on host-specific environment
variables beyond explicit test inputs.

Readiness tests should cover missing command, failing command, missing
collector source, unsupported model mapping, and unavailable credential
allowlist entries.

Collector tests should normalize representative Psychevo, OpenCode, and Hermes
events or exports into canonical trajectory events while preserving lossy
diagnostics when source data is incomplete.

OpenCode and Hermes wrapper tests should prove that concrete manifest kinds
execute through the shared wrapper path, preserve adapter identity, normalize
representative events into trajectories, and contribute metrics without
requiring real OpenCode or Hermes binaries.

Agent integration coverage should include successful coding behavior, incorrect
final workspace behavior, runtime failure, timeout, and lossy trajectory source
handling.

Isolation tests should verify that temporary config roots and allowlisted
environment variables are used instead of the user's normal home state.

## Related Topics

- [340 Psychevo](psychevo.md)
- [340 OpenCode](opencode.md)
- [340 Hermes](hermes.md)
- [300 peval CLI Testing](../300-peval-cli/testing.md)
- [350 Coding Evaluation Testing](../350-coding-evaluation/testing.md)
