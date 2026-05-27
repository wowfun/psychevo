---
name: 330. SWE-bench Benchmark Integration Attachment
psychevo_self_edit: deny
---

Define SWE-bench style benchmark integration.

This attachment is part of [330 Benchmark Integrations](spec.md).

## Scope

- SWE-style dataset loading
- repository base-state preparation
- ephemeral patch generation for official scoring
- result import

## Bridge Shape

The SWE-bench bridge uses official dataset and harness paths when available.
Dataset rows are translated into coding tasks with issue text, repository
identity, base commit or base state, and evaluator expectations.

The candidate modifies an isolated workspace. When the official harness expects
a patch, the bridge may generate a temporary patch from the final workspace and
pass it to the harness. That patch is not retained as a report artifact unless
a later coding spec opts into raw patch retention.

## Results

Official harness outcomes are imported into the common score model with task
identity, benchmark split, harness metadata, pass/fail result, and diagnostic
details. Harness logs may be kept locally as diagnostic artifacts.

Real SWE-bench data access and full split execution are opt-in. The default
behavior for live official runs is a small sample or explicit task limit.

## Related Topics

- [350 SWE-style Tasks](../350-coding-evaluation/task-families.md)
- [350 Scoring](../350-coding-evaluation/scoring.md)
- [095 Official Bridges](../095-evaluation-framework/official-bridges.md)
