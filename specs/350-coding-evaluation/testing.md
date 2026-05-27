---
name: 350. Coding Evaluation Testing
psychevo_self_edit: deny
---

Define deterministic validation for coding-evaluation task semantics.

## Scope

- task family validation
- generated local test asset requirements
- evaluator result handling
- ephemeral patch behavior for SWE-style tasks
- diff-free reporting assertions

Out of scope:

- live provider or third-party agent execution
- official benchmark downloads
- broad framework API testing outside coding semantics

## Deterministic Coverage

Tests should cover one minimal task in each first-slice family: coding-loop,
prompt A/B, and SWE-style. Each test uses fake candidates and local evaluators.
These generated temporary projects are internal validation assets and are not
public benchmark claims.

Evaluator tests should cover pass, fail, malformed data, evaluator failure, and
timeout classification. SWE-style tests should prove that a temporary patch can
be produced for an evaluator when needed without becoming a retained report
artifact.

Generated test workspaces should be tiny, self-contained, and fast. They should
not need network access, package installation from remote registries, system
services, global git config, or user credentials.

Reports for coding tests should assert that diff fields and patch artifacts are
absent by default while oracle results and process metrics remain visible.

At least one test should cover a full local path from compact manifest loading
through fake candidate execution, evaluator scoring, artifact writing, and view
rendering.

## Related Topics

- [350 Task Families](task-families.md)
- [350 Scoring](scoring.md)
- [330 Local Benchmark Integration](../330-benchmark-integrations/local.md)
