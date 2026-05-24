---
name: 330. Benchmark Integrations
psychevo_self_edit: deny
---

# 330. Benchmark Integrations

Define concrete benchmark source integrations. The first set covers local
suites, Harbor/Terminal-Bench style tasks, and SWE-bench style tasks.

## Scope

- local suite layout
- Harbor bridge expectations
- SWE-bench bridge expectations
- benchmark source normalization into domain task families

Out of scope:

- agent execution; see [340 Agent Evaluation](../340-agent-evaluation/spec.md)
- generic official bridge policy; see
  [095 Official Bridges](../095-evaluation-framework/official-bridges.md)
- domain fixture inventory; see [355 Coding Fixtures](../355-coding-fixtures/spec.md)

## Shared Source Rules

Benchmark integrations translate source-specific data into the selected domain
task model before execution. Source metadata such as benchmark name, split, task id,
upstream commit, harness version, and native scorer identity should be retained
in case results.

Official benchmark integrations may delegate setup or scoring to official
harnesses. Delegated output becomes canonical only after import into
`psychevo-eval` result documents.

## Attachments

- [Local](local.md) defines local suite/task directory behavior.
- [Harbor](harbor.md) defines Harbor/Terminal-Bench style integration.
- [SWE-bench](swe-bench.md) defines SWE-bench style integration.
- [Testing](testing.md) defines deterministic bridge validation.

## Related Topics

- [095 Official Bridges](../095-evaluation-framework/official-bridges.md)
- [350 Coding Evaluation](../350-coding-evaluation/spec.md)
- [355 Coding Fixtures](../355-coding-fixtures/spec.md)
