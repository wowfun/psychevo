---
name: 356. Pidx Coding Benchmark
psychevo_self_edit: deny
---

# 356. Pidx Coding Benchmark

Define the first user-facing coding benchmark used by `psychevo-eval`.

## Scope

- `pidx-coding` benchmark inventory
- benchmark-only manifest and task inventory
- deterministic `peval_agent` verifier scripts
- view diagnostics for coding observability

## Benchmark Inventory

`pidx-coding` lives under `crates/psychevo-eval/benchmarks/pidx-coding/` and
is a normal peval benchmark, not a test fixture. It does not include agent
manifests. Eval configs or registry configs pair it with Psychevo, OpenCode,
Hermes, or fake agents. It covers the minimum realistic coding behavior needed
for cross-agent evaluation:

- `coding-patch`: repair a small deterministic code defect
- `stateful-tool-use`: perform ordered state changes through tool use
- `swe-style`: issue-to-patch workflow judged by local tests

Benchmark tasks must be tiny and self-contained. Users explicitly select the
`pidx` source set or the nested `pidx/smoke` set and agent they intend to run;
no-filter benchmark runs are not the documented path.

The benchmark manifest is `benchmark.toml` with `schema_version = 5`, an
explicit benchmark id, and `[[sources.peval_agent]] id = "pidx"`. Each task is
a Harbor-like directory under `tasks/<native-task-id>/` containing:

- `task.toml`, which must parse as TOML
- `instruction.md`, which is the prompt shown to the agent
- `environment/`, copied into an isolated workspace
- `tests/test.sh`, the local verifier run from the workspace cwd

Canonical task ids are `pidx/patch-add`, `pidx/tool-state`, and
`pidx/rust-swe-add`. The `pidx/smoke` set includes the tiny Python and stateful
tasks for fast local checks.

Checked-in eval configs under `crates/psychevo-eval/templates/` are templates
for users or tests to copy into a workspace. They are not benchmark source
files and do not make a benchmark run by default.

## Views

Views should improve diagnosis without retaining code diffs or patch
artifacts by default. View rows include task family, failure class, evaluator
message/details, trajectory links, and artifact links. Case workspaces are not
retained by default.

## Related Topics

- [350 Coding Evaluation](../350-coding-evaluation/spec.md)
- [330 Local Benchmark Integration](../330-benchmark-integrations/local.md)
- [340 Agent Evaluation](../340-agent-evaluation/spec.md)
