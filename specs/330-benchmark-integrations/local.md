---
name: 330. Local Benchmark Integration Attachment
psychevo_self_edit: deny
---

Define local task-set and task behavior.

This attachment is part of [330 Benchmark Integrations](spec.md).

## Scope

- local path benchmark sources
- task directory expectations
- local evaluator-backed task data
- JSONL prompt/task sources for prompt A/B

## Local Task Sets

Local task sets are the fastest path for deterministic development. They live on
disk and do not require network access or official dataset credentials.

Generated local test projects are internal validation assets, not benchmark
claims. Checked-in user-facing benchmark projects live under
`crates/psychevo-eval/benchmarks/` and must be runnable as normal peval
benchmarks through eval configs.

A local task source is a JSONL task file plus optional task-owned directories.
Task directories may contain starting workspaces or non-executable data.
Executable evaluator or fake-agent scripts are not task-owned source files in the
current local layout.

Current local benchmarks are discovered through `benchmark.toml`. The benchmark
manifest declares the benchmark-level evaluator, task sources, and task sets.
Agent definitions and runnable selections live in eval configs or registry
configs. JSONL task sources contain one task row per benchmark unit; task-owned
workspaces remain under `tasks/<task-id>/`.

## Task Content

A local task should provide:

- task identity
- problem statement or instruction data
- initial workspace source
- evaluator-specific typed `test_spec`
- timeout and environment requirements when they differ from task-set defaults

Local evaluator checks produce normalized evaluator results. Task rows must not
declare arbitrary commands; command execution is owned by the evaluator
implementation.

Local test and benchmark workspaces should be tiny, self-contained, and fast.
They should not need network access, package installation from remote
registries, system services, global git config, or user credentials.

The local bridge must not retain code diffs or patch artifacts by default.

## Related Topics

- [350 Task Families](../350-coding-evaluation/task-families.md)
- [350 Scoring](../350-coding-evaluation/scoring.md)
- [356 Pidx Coding Benchmark](../356-pidx-coding-benchmark/spec.md)
