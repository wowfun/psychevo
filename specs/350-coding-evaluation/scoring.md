---
name: 350. Coding Evaluation Scoring Attachment
psychevo_self_edit: deny
---

Define scoring for coding evaluation tasks.

This attachment is part of [350 Coding Evaluation](spec.md).

## Scope

- scorer output contract for coding tasks
- oracle and process metrics
- diff-free coding report behavior
- failure classification

## Scorer Output

Custom coding scorers use exit code plus JSON stdout. Exit code failure means
the scorer itself failed. Successful scorer execution emits JSON with pass/fail,
numeric score when available, metrics, and diagnostic details.

The framework imports official harness results into the same score model even
when the external harness uses its own native files or process output.

## Oracle and Metrics

The oracle result is primary. A task that fails tests, fails an official
harness, or fails a deterministic checker is unsuccessful even if process
metrics look efficient.

Process metrics are secondary and help explain behavior. Useful first-slice
metrics include elapsed time, candidate status, timeout status, tool event
counts when available, token or cost data when available, and scorer duration.

## Diff-Free Reports

Coding evaluation does not persist patch or diff artifacts by default. Reports
should omit changed-file and lines-changed metrics unless a later coding spec
opts into collecting them without retaining raw patches.

Failed workspaces may be retained for diagnosis according to fixture or runner
policy. The retained workspace path is a local diagnostic pointer, not a
shareable report artifact.

## Related Topics

- [090 Artifacts](../090-evaluation/artifacts.md)
- [350 Task Families](task-families.md)
- [355 Coding Fixtures](../355-coding-fixtures/spec.md)
