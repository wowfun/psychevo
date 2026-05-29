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
TOML. Benchmark manifests declare typed source tables under `sources`; source
adapters translate their native layout into the framework's canonical task-set
and task model before execution.

Every manifest must identify its `schema_version`. Readers reject unsupported
versions before matrix expansion.

Current benchmark manifests and eval configs use `schema_version = 5`.
Workspace and user registry configs use `schema_version = 2`. Evaluator result
JSON uses schema v2. v4 benchmark, eval, and task-row manifests are no longer
accepted; diagnostics must point authors at the v5 manifest documentation.

## Benchmark Layout

Benchmarks are stable measurement definitions rooted at `benchmark.toml`.
They answer what is measured and how it is scored; they do not define which
agents are evaluated in a particular run.

Benchmark-relative manifests are organized as:

- `benchmark.toml` for benchmark identity and typed source definitions
- `tasks/<task-id>/` for `peval_agent` task directories
- source-native roots for official Harbor, SWE-bench, and Tau2 integrations

All paths inside `benchmark.toml` resolve relative to the benchmark root unless
the source contract says otherwise. Unsupported `schema_version` is a hard
validation error with a clear diagnostic.

Benchmark manifests declare one or more typed source arrays:

- `[[sources.peval_agent]]` for local host-run coding tasks
- `[[sources.harbor]]` for official Harbor execution
- `[[sources.swe_bench]]` for official SWE-bench execution
- `[[sources.tau2]]` for Tau2 ACP/MCP execution

Mixed source types are allowed in one benchmark. `peval check` must reject a
selected source and agent combination when the adapter cannot execute that
source deterministically.

Source and set identity is canonicalized after source normalization:

- `source-id/native-task-id` is the canonical task id.
- The source's full set id is `source-id`.
- Nested source set ids are `source-id/set-id`.
- Source-local `include` and `exclude` filters match native task ids, not
  source-prefixed ids.
- Source task ordering and `limit` selection are deterministic after sorting
  native task ids.

`peval_agent` scans task subdirectories. Each task directory must contain a
parseable `task.toml`, `instruction.md`, `environment/`, and `tests/test.sh`.
The directory name is the native task id. `environment/` is copied to an
isolated workspace, the selected agent runs with that workspace as cwd, and
`tests/test.sh` runs from the workspace cwd. The runner sets `PEVAL_WORKSPACE`,
`PEVAL_TASK_DIR`, `PEVAL_LOGS`, `PEVAL_TASK_ID`, and `PEVAL_SOURCE_ID`.

## Eval Configs And Registries

An eval config is any TOML file selected with `--config`. It owns the runnable
plan: eval id/name, one benchmark reference, explicit selected agents, and
explicit selected sets or tasks. A benchmark reference may be an id
resolved from registries or a direct path to `benchmark.toml`.

Registries are configuration layers, not command-managed state. Agent and
benchmark definitions resolve from highest to lowest priority: eval config,
workspace `peval.toml`, then user `$PSYCHEVO_HOME/peval-config.toml`. Inline
eval definitions override same-id workspace or user definitions. Workspace and
user configs use schema v2 and reject older schema versions clearly.

Eval configs must not imply "run everything." They must declare a non-empty
agent selection and at least one set or task selection before execution.
The selection key is `sets`; legacy `task_sets` is not accepted. CLI filters
may narrow this selection but must not expand beyond it.

Eval, workspace, and user configs may declare report profiles under
`[reports.<key>]`. A report profile is a named view/server configuration, not a
run definition. It may declare labeled jobs with `[[reports.<key>.jobs]]`, view
includes, primary matrix metric, output format and path, timestamped output
snapshots, columns, and an explicit analysis policy. Report profile lookup
merges the same config layers as registries: eval config first, then workspace
`peval.toml`, then user `$PSYCHEVO_HOME/peval-config.toml`. An eval-local
report with no jobs inherits that eval config's selected benchmark and filters.
Analysis policies may reference an agent id from the resolved registry; they
must be explicit because analysis can execute an agent and write cached
`analysis.md` and `analysis.json` files beside selected cells.

## Manifest Concepts

A source set entry may declare:

- set identity, name, and description
- include and exclude filters over native task ids
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

`kind = "command"` is the local-only public adapter for host-run tasks. It
supports `{workspace}`, `{task_dir}`, `{prompt}`, and `{prompt_file}` argument
templates, defaults to workspace cwd, defaults to a 600 second timeout, and may
emit JSONL stdout events. `kind = "acp"` is the generic ACP stdio adapter.
`kind = "psychevo"` remains a preset shortcut backed by ACP or a compatible
wrapper where needed. The fake adapter is available in default builds and
default validation.

Set entries do not bind agents. Matrix expansion combines the eval config's
selected sets or tasks with the selected agents. If a CLI filter
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
