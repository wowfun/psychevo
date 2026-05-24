---
name: 340. OpenCode Agent Evaluation Attachment
psychevo_self_edit: deny
---

Define the OpenCode candidate adapter for evaluation.

This attachment is part of [340 Agent Evaluation](spec.md).

## Scope

- wrapper adapter expectations for OpenCode
- readiness checks
- model and config isolation
- trajectory collection sources

## Wrapper Strategy

The OpenCode adapter starts from a manifest-resolved command rather than a
hard-coded binary path. Presets may default to `opencode`, but the manifest can
override command, arguments, working directory, environment, config root, and
collector.

Readiness checks should catch missing binaries, broken package installation,
unavailable postinstall artifacts, missing provider mappings, and unsupported
headless invocation modes before cases execute.

## Fairness

OpenCode runs use an isolated config/home root by default. Provider credentials
and model names enter through the evaluation allowlist and canonical model
mapping, not through implicit user shell state.

The adapter should select the OpenCode agent mode that matches the intended
coding task permissions. If the underlying OpenCode version exposes separate
planning and build agents, the manifest must record which mode was used.

## Trajectory

Preferred collection uses OpenCode's session event stream or session store when
available. OpenCode's event names are normalized into the canonical trajectory
model while retaining source event kind as metadata.

When no event source is available, stdout/stderr and final workspace state may
produce a reduced trajectory with an explicit lossy collector diagnostic.

## Related Topics

- [340 Agent Evaluation](spec.md)
- [090 Adapters](../090-evaluation/adapters.md)
- [095 Manifest](../095-evaluation-framework/manifest.md)
