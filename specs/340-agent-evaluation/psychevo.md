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

## ACP Profile

The public Psychevo evaluation path is `kind = "psychevo-acp"`. It starts a
Psychevo ACP stdio server and evaluates it through the shared ACP adapter. The
legacy `kind = "psychevo"` wrapper path is removed.

The profile maps manifest fields to ACP setup explicitly:

- model and reasoning effort
- run mode and permission mode
- agent selection
- skills and skill disabling
- MCP server inputs
- toolset policy when exposed by runtime
- host database, workdir, config path, and inherited environment by default

For host-run benchmarks the profile defaults to the same Psychevo home, config,
database, provider credentials, and environment that `pevo acp` would use from
the shell. Eval manifests may explicitly override `PSYCHEVO_HOME`,
`PSYCHEVO_DB`, or `PSYCHEVO_CONFIG` for isolated test runs. For
container-backed benchmarks the profile first uses an explicit manifest `pevo`
path when present. Otherwise it may build the workspace `pevo` binary on the
host and copy it into the container. The server runs with per-run
container-local `PSYCHEVO_HOME`, `PSYCHEVO_DB`, and `PSYCHEVO_CONFIG`.

## Trajectory

Psychevo trajectory capture uses ACP notifications, prompt responses, and
Psychevo-specific ACP metadata where available. The collector records
normalized events and preserves the ACP session id as diagnostic metadata.

The host adapter may read Psychevo's normal local SQLite state when the
manifest does not request an isolated state path. Isolated test runs must set
explicit state paths and must not inspect the user's normal Psychevo state.

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
