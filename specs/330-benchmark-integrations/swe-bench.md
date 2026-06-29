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

The ACP candidate runs in a containerized authoring environment rooted at the
official SWE-bench cwd, normally `/testbed`. The prompt contains the
instance problem statement plus minimal cwd instructions. It must not
include hidden tests, `FAIL_TO_PASS`, or `PASS_TO_PASS`.

After the ACP agent finishes, the bridge collects `git diff` from the authoring
container as the candidate patch. Official scoring runs in a fresh verifier
container through the local SWE-bench Python harness by applying that patch and
running the official eval script. This separates agent side effects from the
official verifier environment while preserving SWE-bench scoring semantics.

Dataset access is local-first. Reading an existing local dataset or cache is
allowed by default; HuggingFace or other network downloads require explicit
opt-in.

## Results

Official harness outcomes are imported into the common score model with task
identity, benchmark split, harness metadata, pass/fail result, and diagnostic
details. The generated candidate patch, official harness logs, raw ACP JSONL,
and normalized trajectory are local diagnostic artifacts.

Real SWE-bench data access and full split execution are opt-in. The default
behavior for live official runs is a small sample or explicit task limit.

## Related Topics

- [350 SWE-style Tasks](../350-coding-evaluation/task-families.md)
- [350 Scoring](../350-coding-evaluation/scoring.md)
- [095 Official Bridges](../095-evaluation-framework/official-bridges.md)
