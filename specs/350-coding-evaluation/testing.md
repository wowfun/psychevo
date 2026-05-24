---
name: 350. Coding Evaluation Testing
psychevo_self_edit: deny
---

Define deterministic validation for coding-evaluation task semantics.

## Scope

- task family validation
- scorer JSON handling
- ephemeral patch behavior for SWE-style tasks
- diff-free reporting assertions

Out of scope:

- live provider or third-party agent execution
- official benchmark downloads
- broad framework API testing outside coding semantics

## Deterministic Coverage

Tests should cover one minimal task in each first-slice family: coding-loop,
prompt A/B, and SWE-style. Each test uses fake candidates and local scorers.

Scorer tests should cover pass, fail, malformed JSON, scorer exit failure, and
timeout classification. SWE-style tests should prove that a temporary patch can
be produced for a scorer when needed without becoming a retained report
artifact.

Reports for coding tests should assert that diff fields and patch artifacts are
absent by default while oracle results and process metrics remain visible.

## Related Topics

- [350 Task Families](task-families.md)
- [350 Scoring](scoring.md)
- [355 Coding Fixtures](../355-coding-fixtures/spec.md)
