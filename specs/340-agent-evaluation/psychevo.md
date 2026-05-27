---
name: 340. Psychevo Agent Evaluation Attachment
psychevo_self_edit: deny
---

Define the Psychevo candidate adapter for evaluation.

This attachment is part of [340 Agent Evaluation](spec.md).

## Scope

- native `psychevo-runtime` adapter expectations
- structured mapping from evaluation manifest to runtime options
- event and artifact collection
- runtime impact limits

## Native Adapter

The Psychevo adapter should use stable runtime APIs instead of shelling through
`pevo` when the required runtime capability is available. The adapter maps
manifest fields to runtime options explicitly:

- model and reasoning effort
- run mode and permission mode
- agent selection
- skills and skill disabling
- MCP server inputs
- toolset policy when exposed by runtime
- isolated database, workdir, config path, and inherited environment

The adapter owns evaluation-specific setup around runtime. Runtime itself must
not gain benchmark orchestration, report rendering, or evaluation-matrix
awareness for this adapter.

The first implementation may shell through `pevo run` while preserving the
manifest-visible Psychevo adapter identity. Deterministic validation configures
`pevo run` against a local OpenAI-compatible mock provider; real provider
validation uses the same adapter path with user-supplied provider credentials.

## Trajectory

Psychevo trajectory capture should use runtime stream events and session export
data where needed. The collector records normalized events and preserves the
runtime session id as diagnostic metadata.

The adapter may read Psychevo's local SQLite state for the isolated evaluation
database. It must not inspect the user's normal Psychevo state unless the
manifest explicitly points at that state.

## Performance Boundary

Adding this adapter must not slow ordinary `pevo run`, `pevo tui`, or ACP
startup. Evaluation hooks should be passive runtime outputs or explicitly
enabled adapter calls.

Psychevo adapter validation is an ordinary `peval run` against a selected
benchmark task set and agent. Test runs use an isolated Psychevo home, database,
and local mock provider; real runs may point at the user's configured provider.
The structured evaluation evaluator still decides whether the case passes.

## Related Topics

- [100 Coding Agent](../100-coding-agent/spec.md)
- [200 pevo CLI](../200-pevo-cli/spec.md)
- [095 Crate API](../095-evaluation-framework/crate-api.md)
