---
name: 340. Hermes Agent Evaluation Attachment
psychevo_self_edit: deny
---

Define the Hermes candidate adapter for evaluation.

This attachment is part of [340 Agent Evaluation](spec.md).

## Scope

- wrapper adapter expectations for Hermes
- oneshot and session-based execution
- config isolation and credential allowlists
- trajectory collection and export strategy

## ACP Profile

The public Hermes evaluation path is `kind = "hermes-acp"`. It starts
`hermes-acp` through the shared ACP adapter. The legacy `kind = "hermes"`
wrapper path is removed.

Hermes execution is adapter-driven from manifest-resolved ACP configuration.
The default install follows the ACP registry shape with
`uvx hermes-agent[acp]==0.15.1 hermes-acp`, while command, arguments,
environment, install strategy, version, and cache behavior can be overridden.
Deterministic tests use fake ACP fixture commands; real Hermes installation,
provider credentials, and live execution are outside the default validation
path.

The first execution mode should support headless single-task prompts. If a
Hermes version supports both final-text oneshot output and richer session
exports, the adapter should prefer the richer source for trajectories while
keeping final answer parsing simple.

## Isolation

Hermes runs use an isolated home/profile/config root by default, including
`HERMES_HOME`. Provider keys, model settings, skills, toolsets, and approval
behavior must be manifest-visible when they influence a run.

## Trajectory

Hermes trajectory collection may combine process output, session exports,
local session databases, and Hermes-style conversation JSONL when available.
The collector marks whether the trajectory came from event stream, export,
database inspection, or lossy process parsing.

Hermes-style conversation export is useful as a derived training or analysis
format, but the canonical peval trajectory remains the normalized event stream.

## Related Topics

- [090 Artifacts](../090-evaluation/artifacts.md)
- [095 Sidecar](../095-evaluation-framework/sidecar.md)
- [340 Agent Evaluation](spec.md)
