---
name: 340. Agent Evaluation
psychevo_self_edit: deny
---

# 340. Agent Evaluation

Define concrete agent evaluation adapters for the first comparison set:
command, generic ACP, and built-in ACP profiles for Psychevo, OpenCode, and
Hermes.

## Scope

- shared fairness rules for agent evaluation adapters
- preset and manifest behavior for built-in public agent kinds
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

The `command` adapter is the local process baseline for host-run benchmarks.
It starts the configured command in the task workspace, expands prompt and path
templates, and imports optional JSONL stdout events.

The `acp` adapter is a generic ACP stdio client. It starts the configured ACP
server command, performs initialize/session setup, sends the task prompt through
`session/prompt`, records raw ACP JSONL and normalized trajectory events, and
treats a successful prompt response as agent completion. It must use
initialize capability negotiation before optional model, mode, or permission
requests and must fail clearly when requested capabilities are unavailable.
Deterministic tests may use a local ACP fixture process; real provider
validation remains opt-in through the configured ACP server.

`psychevo-acp`, `opencode-acp`, and `hermes-acp` are public ACP profile kinds.
They normalize to the generic ACP adapter at execution time while preserving
their manifest-visible profile identity in reports. Their configuration reuses
`[agents.acp]`. The legacy `psychevo`, `opencode`, and `hermes` wrapper kinds
are removed interfaces and should fail with migration guidance.

ACP profiles support `preinstalled`, `install_command`, and profile-default
install strategies. Container-backed benchmarks run ACP servers inside the
task container. Host-installed OpenCode or Hermes binaries may be used to
prepare container caches, but real benchmark execution must not run those ACP
servers on the host. Host-run `psychevo-acp` inherits the user's normal
Psychevo config, credentials, and environment by default so local live evals use
the same provider setup as `pevo acp`; manifests may still override
`PSYCHEVO_HOME`, `PSYCHEVO_DB`, or `PSYCHEVO_CONFIG` explicitly. Container
`psychevo-acp` runs, and host/container OpenCode or Hermes profile runs, use
per-run state directories for home/config/cache isolation.

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
