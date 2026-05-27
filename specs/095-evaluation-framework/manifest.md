---
name: 095. Evaluation Framework Manifest Attachment
psychevo_self_edit: deny
---

Define framework manifest contracts for benchmarks, eval configs, registries,
task sets, agents, runs, and factor expansion.

This attachment is part of [095 Evaluation Framework](spec.md).

## Scope

- human-authored benchmark, eval config, and registry format
- schema-version requirements
- task-set, agent, benchmark, model, and factor declarations
- environment and credential allowlist declarations

Out of scope:

- exact CLI flag names
- external benchmark-native schemas
- domain-specific task-family fields

## Format

Framework-owned benchmark manifests, eval configs, and registry configs use
TOML. Dense benchmark task inventories use JSONL when each row is a task
record. External benchmark adapters may read their native source formats, but
must translate those sources into the framework's canonical task-set and task
model before execution.

Every manifest must identify its `schema_version`. Readers reject unsupported
versions before matrix expansion.

Current benchmark manifests, eval configs, and task rows use `schema_version =
4`. Workspace and user registry configs use `schema_version = 2`. Evaluator
result JSON uses schema v2.

## Benchmark Layout

Benchmarks are stable measurement definitions rooted at `benchmark.toml`.
They answer what is measured and how it is scored; they do not define which
agents are evaluated in a particular run.

Benchmark-relative manifests are organized as:

- `benchmark.toml` for benchmark identity, task sources, task-set definitions,
  and the benchmark-level evaluator declaration
- `tasks.jsonl` or another declared task source for dense task records
- `tasks/<task-id>/` for task-owned workspaces and non-executable local assets

All paths inside `benchmark.toml` resolve relative to the benchmark root. Task row
paths resolve relative to the row `dir` when present, otherwise relative to the
task source file directory. Unsupported `schema_version` is a hard validation
error with a clear diagnostic.

Benchmark manifests declare one benchmark-level evaluator:

- `[evaluator] kind = "local-coding"` for built-in local coding checks
- `[evaluator] kind = "tau2"` for declaration-only Tau2 benchmark metadata
- `[evaluator] kind = "swe-bench"` for declaration-only SWE-bench metadata

External evaluator declarations may include `[evaluator.args]` as free
metadata. They are valid for offline checks but are not executable until a
lower bridge spec implements the adapter.

Task source rows use `task_id`, problem data, workspace data, and
evaluator-specific `test_spec` data. Task rows do not declare arbitrary
commands, evaluator scripts, or fake-agent scripts. The evaluator kind owns
validation and interpretation of `test_spec`.

## Eval Configs And Registries

An eval config is any TOML file selected with `--config`. It owns the runnable
plan: eval id/name, one benchmark reference, explicit selected agents, and
explicit selected task-sets or tasks. A benchmark reference may be an id
resolved from registries or a direct path to `benchmark.toml`.

Registries are configuration layers, not command-managed state. Agent and
benchmark definitions resolve from highest to lowest priority: eval config,
workspace `peval.toml`, then user `$PSYCHEVO_HOME/peval-config.toml`. Inline
eval definitions override same-id workspace or user definitions. Workspace and
user configs use schema v2 and reject older schema versions clearly.

Eval configs must not imply "run everything." They must declare a non-empty
agent selection and at least one task-set or task selection before execution.
CLI filters may narrow this selection but must not expand beyond it.

## Manifest Concepts

A task-set manifest entry may declare:

- task-set identity, name, and description
- task ids included in the set
- benchmark split or sample selection metadata
- factor matrix entries that apply to all tasks in the set
- output and retention defaults

An inline agent manifest entry may declare:

- preset name or custom adapter kind
- command, arguments, working directory, and environment overrides for wrapper
  adapters
- native adapter options for in-process adapters
- collector source selection
- model mapping and provider credential allowlist
- readiness requirements

The first fake adapter is available in default builds and default validation.
The Psychevo adapter lives behind an explicit adapter boundary. Real versus
mock provider behavior is selected through the resolved agent and provider
configuration, not through a separate live gate.

Task-set entries do not bind agents. Matrix expansion combines the eval
config's selected task sets or tasks with the selected agents. If a CLI filter
is supplied, it narrows the eval config selection.

## Factor Expansion

Factors are first-class configuration. Agent comparison, prompt A/B, model
comparison, permission comparison, skill/toolset comparison, and benchmark split
selection all use the same expansion mechanism.

Expansion must be deterministic. Expanded case metadata must be recorded in
artifacts exactly enough for views to reconstruct the comparison matrix.

## Credentials and Environment

Manifests must use allowlists for credentials and host environment variables.
Implicit inheritance of user config, shell environment, or agent home state is
not the default framework behavior.

Concrete adapters may define named convenience presets, but presets still
resolve to explicit manifest-equivalent settings before execution.

## Related Topics

- [095 Execution](execution.md)
- [090 Schema](../090-evaluation/schema.md)
- [300 Commands](../300-peval-cli/commands.md)
