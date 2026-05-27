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

## Wrapper Strategy

Hermes execution is adapter-driven from a manifest-resolved command. Presets
may default to `hermes`, but command, arguments, environment, config root,
working directory, and collector can be overridden.

The first implementation accepts an explicit command template and executes it
through the shared wrapper adapter path. Deterministic tests use fake Hermes
commands that emit representative process or JSONL observations; real Hermes
installation, provider credentials, and live execution are outside the default
validation path.

The first execution mode should support headless single-task prompts. If a
Hermes version supports both final-text oneshot output and richer session
exports, the adapter should prefer the richer source for trajectories while
keeping final answer parsing simple.

## Isolation

Hermes runs use an isolated home/profile/config root by default. Provider keys,
model settings, skills, toolsets, and yolo or approval behavior must be
manifest-visible when they influence a run.

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
