---
name: 330. Benchmark Integrations
psychevo_self_edit: deny
---

# 330. Benchmark Integrations

Define concrete benchmark source integrations. The first set covers local
`peval_agent` tasks, Harbor/Terminal-Bench style tasks, SWE-bench style tasks,
and Tau2 tasks.

## Scope

- local `peval_agent` task layout
- Harbor bridge expectations
- SWE-bench bridge expectations
- Tau2 bridge expectations
- benchmark source normalization into domain task families

Out of scope:

- agent execution; see [340 Agent Evaluation](../340-agent-evaluation/spec.md)
- generic official bridge policy; see
  [095 Official Bridges](../095-evaluation-framework/official-bridges.md)
- coding task semantics; see [350 Coding Evaluation](../350-coding-evaluation/spec.md)

## Shared Source Rules

Benchmark integrations translate source-specific data into the selected domain
task model before execution. Source metadata such as benchmark name, split,
native task id, upstream commit, harness version, and native evaluator identity
should be retained in case results.

Every typed source contributes a full source set whose id is the source id.
Nested sets are declared below the source and normalize to `source-id/set-id`.
Canonical task ids are always `source-id/native-task-id`; source-local filters
match native ids before prefixing.

`peval_agent` is the only built-in host runner. It copies each task's
`environment/` directory to an isolated workspace, runs the selected local
agent in that workspace, and then runs `tests/test.sh` from the workspace cwd.
Task `task.toml` files are required and must parse as TOML, but the directory
name remains the native task id.

Official benchmark integrations may delegate setup or scoring to official
harnesses. Delegated output becomes canonical only after import into
`psychevo-eval` result documents.

`harbor`, `swe_bench`, and `tau2` source declarations opt into official bridge
behavior. Default validation is local and deterministic; network/provider
probes, data downloads, and live service checks require an explicit live mode
or source-level opt-in. `peval check` may verify selected local tool
dependencies such as `uv` or Docker, but it must not download benchmark data by
default.

## Dataset Inventory

The user-level persistent evaluation store may keep local dataset inventory records under
`datasets/<dataset-id>/dataset.toml`. A dataset record describes the benchmark
kind, source, referenced or linked payload, loader, split, sample limit, cache
key, license, tags, and notes. Inventory records are local metadata; they do
not imply that official data has been downloaded or that a live benchmark
adapter is available.

The first dataset import path registers local payloads by reference or link to
avoid duplicating large data. Listing and dashboard views should report whether
the referenced payload is currently present.

## Attachments

- [Local](local.md) defines local task-set/task directory behavior.
- [Harbor](harbor.md) defines Harbor/Terminal-Bench style integration.
- [SWE-bench](swe-bench.md) defines SWE-bench style integration.
- [Testing](testing.md) defines deterministic bridge validation.

## Related Topics

- [095 Official Bridges](../095-evaluation-framework/official-bridges.md)
- [350 Coding Evaluation](../350-coding-evaluation/spec.md)
- [356 Pidx Coding Benchmark](../356-pidx-coding-benchmark/spec.md)
