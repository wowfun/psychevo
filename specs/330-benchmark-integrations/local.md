---
name: 330. Local Benchmark Integration Attachment
psychevo_self_edit: deny
---

Define local `peval_agent` task behavior.

This attachment is part of [330 Benchmark Integrations](spec.md).

## Scope

- local path benchmark sources
- task directory expectations
- local verifier-backed task data

## Local Task Sets

Local task sets are the fastest path for deterministic development. They live on
disk and do not require network access or official dataset credentials.

Generated local test projects are internal validation assets, not benchmark
claims. Checked-in user-facing benchmark projects live under
`crates/psychevo-eval/benchmarks/` and must be runnable as normal peval
benchmarks through eval configs.

`sources.peval_agent` scans task-owned directories under its configured path.
Each task directory contains `task.toml`, `instruction.md`, `environment/`, and
`tests/test.sh`.

Current local benchmarks are discovered through `benchmark.toml`. The benchmark
manifest declares typed sources and source-local sets. Agent definitions and
runnable selections live in eval configs or registry configs. Canonical task ids
are `source-id/native-task-id`; the full source set is `source-id`, and nested
sets are `source-id/set-id`.

## Task Content

A local task should provide:

- task identity
- instruction data
- initial workspace source in `environment/`
- verifier script in `tests/test.sh`
- timeout and environment requirements when they differ from task-set defaults

The local runner copies `environment/` into an isolated workspace, runs the
selected agent with that workspace as cwd, then runs `tests/test.sh` from the
workspace cwd. Verifier exit status produces normalized pass/fail results.
Optional `$PEVAL_LOGS/verifier/result.json` or `reward.txt` files may refine
score and diagnostics.

Local test and benchmark workspaces should be tiny, self-contained, and fast.
They should not need network access, package installation from remote
registries, system services, global git config, or user credentials.

The local bridge must not retain code diffs or patch artifacts by default.

## Related Topics

- [350 Task Families](../350-coding-evaluation/task-families.md)
- [350 Scoring](../350-coding-evaluation/scoring.md)
- [356 Pidx Coding Benchmark](../356-pidx-coding-benchmark/spec.md)
