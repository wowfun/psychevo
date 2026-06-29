---
name: 356. Pidx Coding Benchmark Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for the checked-in
`pidx-coding` benchmark.

## Long-Term Acceptance Contract

- `pidx-coding` is a normal user-facing peval benchmark, not a test fixture.
- The benchmark manifest parses as `schema_version = 5` and declares the
  `pidx` `peval_agent` source set without embedding agent manifests.
- Canonical task ids are `pidx/patch-add`, `pidx/tool-state`, and
  `pidx/rust-swe-add`.
- The `pidx/smoke` set includes the tiny Python and stateful tasks for fast
  local checks.
- Each task is tiny, self-contained, and contains parseable `task.toml`,
  `instruction.md`, `environment/`, and `tests/test.sh`.
- Tests and smoke runs copy task environments into isolated temporary
  workspaces and do not mutate the checked-in benchmark tree.
- Deterministic validation does not require network access, package
  installation from remote registries, provider credentials, global git config,
  or live third-party agents.
- Views expose task family, failure class, evaluator message/details,
  trajectory links, and artifact links while omitting retained code diffs,
  patch artifacts, and case workspaces by default.

## Current Implementation Slice

CI/CD vocabulary and generic validation boundaries follow
[065 CI/CD](../065-ci-cd/spec.md).

This benchmark is validated through the `psychevo-eval` deterministic test
harness and CLI smoke coverage with fake agents or local evaluators. Live agent
or provider runs are opt-in benchmark experiments, not default validation.

## Scenario Matrix

- The checked-in benchmark manifest loads through normal peval benchmark
  discovery.
- Explicit `pidx` and `pidx/smoke` selection choose the intended task set.
- No-filter benchmark runs are not treated as the documented smoke path.
- Each canonical task verifies its required files and local test script.
- Generated temporary workspaces preserve task environment contents without
  writing back to the benchmark source tree.
- Fake-agent or local-evaluator runs classify pass, fail, evaluator failure,
  and timeout outcomes without live providers.
- `peval check --json` or an equivalent deterministic CLI path reports the
  public benchmark/task selection shape.
- View rendering includes diagnostic rows and links while keeping diffs,
  patches, and case workspaces absent by default.

## Validation Boundaries

- Tests should assert benchmark inventory and public peval behavior rather than
  private fixture-generator internals.
- Checked-in tasks are user-facing examples; generated temporary workspaces are
  internal validation assets.
- Live provider comparisons may be useful for evaluation experiments, but they
  must report selected agent config, isolated output roots, and retained
  artifacts separately from deterministic acceptance.
