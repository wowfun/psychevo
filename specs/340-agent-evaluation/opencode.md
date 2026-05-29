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

## ACP Profile

The public OpenCode evaluation path is `kind = "opencode-acp"`. It starts
`opencode acp` through the shared ACP adapter. The legacy `kind = "opencode"`
wrapper path is removed.

The profile starts from manifest-resolved ACP configuration. Presets may
install Node 22 and `opencode-ai@<profile-version>` in the task container, but
the manifest can override command, arguments, environment, install strategy,
version, and cache behavior. Deterministic tests use fake ACP fixture commands;
real OpenCode installation, provider credentials, and live execution are
outside the default validation path.

Readiness checks should catch missing binaries, broken package installation,
unavailable postinstall artifacts, missing provider mappings, and unsupported
headless invocation modes before cases execute.

## Fairness

OpenCode runs use an isolated config/home root by default. Provider credentials
and model names enter through manifest env templates or `provider/model`
inference, not through implicit user shell state.

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
