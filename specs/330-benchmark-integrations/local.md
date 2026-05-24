---
name: 330. Local Benchmark Integration Attachment
psychevo_self_edit: deny
---

Define local suite and task behavior.

This attachment is part of [330 Benchmark Integrations](spec.md).

## Scope

- local path benchmark sources
- task directory expectations
- local setup and scorer scripts
- JSONL prompt/task sources for prompt A/B

## Local Suites

Local suites are the fastest path for deterministic development. They live on
disk and do not require network access or official dataset credentials.

A local task source may be a task directory collection or a JSONL task file.
Directory tasks are preferred for coding-loop and SWE-style fixtures because
they can include a starting workspace and scorer script. JSONL is useful for
prompt A/B cases where the task body is lightweight and setup is shared.

In the first implementation, a directory project root is discovered through
`eval.toml`; suite manifests point at local `tasks/**/task.toml` entries.
The local Rust SWE-style fixture is the default deterministic bridge example.

## Task Content

A local task should provide:

- task identity
- initial prompt or instruction file
- initial workspace source
- setup command when needed
- scorer command or oracle declaration
- timeout and environment requirements when they differ from suite defaults

Local scorers follow the coding scorer contract: exit code for scorer failure,
JSON stdout for task result.

The local bridge must not retain code diffs or patch artifacts by default.

## Related Topics

- [350 Task Families](../350-coding-evaluation/task-families.md)
- [350 Scoring](../350-coding-evaluation/scoring.md)
- [355 Fixtures](../355-coding-fixtures/fixtures.md)
