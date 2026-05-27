---
name: 340. Agent Evaluation
psychevo_self_edit: deny
---

# 340. Agent Evaluation

Define concrete agent evaluation adapters for the first comparison set: Psychevo,
OpenCode, and Hermes.

## Scope

- shared fairness rules for agent evaluation adapters
- preset and manifest behavior for the first three agents
- event collection and result normalization expectations
- readiness diagnostics for local agent executables and configs

Out of scope:

- generic adapter abstractions; see [090 Adapters](../090-evaluation/adapters.md)
- domain task-family semantics; see
  [350 Coding Evaluation](../350-coding-evaluation/spec.md) for the first
  domain
- benchmark source integration; see
  [330 Benchmark Integrations](../330-benchmark-integrations/spec.md)

## Shared Rules

Each agent evaluation adapter resolves to a manifest-equivalent configuration
before execution. Presets are conveniences, not hidden behavior. Reports must
show the resolved agent identity, canonical model, adapter kind, and collector
source.

Fair comparison defaults to the same canonical model, same task workspace,
same timeout, same network policy, same credential allowlist, and same
workspace isolation policy across agents.

Agent evaluation adapters should prefer event streams or native session exports
for trajectory capture. Stdout/stderr parsing is a fallback, not the preferred
source of truth.

Wrapper adapters for OpenCode and Hermes are concrete adapter kinds, not fake
agents. They share process execution, command-template expansion, isolated
home/config handling, and lossy collector fallback internals while preserving
their adapter identity in manifests, reports, diagnostics, and facts.

Adapter validation uses the same command path as real usage. Deterministic
tests configure local mock providers or fake wrapper commands; real provider
validation remains opt-in through credentials and provider configuration, not a
separate live gate.

## Attachments

- [Psychevo](psychevo.md) defines the native Psychevo adapter.
- [OpenCode](opencode.md) defines the OpenCode wrapper and collector strategy.
- [Hermes](hermes.md) defines the Hermes wrapper and collector strategy.
- [Testing](testing.md) defines deterministic adapter validation.

## Related Topics

- [095 Manifest](../095-evaluation-framework/manifest.md)
- [095 Execution](../095-evaluation-framework/execution.md)
- [350 Coding Evaluation](../350-coding-evaluation/spec.md)
