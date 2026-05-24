---
name: 355. Coding Fixtures Attachment
psychevo_self_edit: deny
---

Define local fixture content for coding evaluation.

This attachment is part of [355 Coding Fixtures](spec.md).

## Scope

- fixture suite inventory
- fixture workspace expectations
- scorer fixture behavior
- retained artifact expectations

## Required Fixtures

The first fixture inventory should include:

- a coding-loop task that requires reading existing files and making a small
  deterministic edit
- a prompt A/B task whose oracle stays constant while factor expansion changes
  prompt or policy inputs
- a SWE-style task with an issue statement, base workspace, and test-based
  oracle

The first implementation slice may start with one local Rust SWE-style fixture
when the framework, CLI, artifact, report, compare, and replay paths are all
exercised end to end. Additional coding-loop and prompt A/B fixtures remain
part of the fixture inventory target.

Fixture workspaces should be tiny, self-contained, and fast. They should not
need network access, package installation from remote registries, system
services, global git config, or user credentials.

## Scorers

Fixture scorers produce JSON stdout and deterministic pass/fail outcomes. At
least one fixture scorer should be able to emit malformed output or an
intentional scorer failure for error-path coverage.

## Artifacts

Fixture runs should generate the same artifact classes as ordinary runs:
summary, case result, trajectory, scorer log, and derived report input. They
should not generate retained diff or patch artifacts by default.

## Related Topics

- [350 Scoring](../350-coding-evaluation/scoring.md)
- [330 Local](../330-benchmark-integrations/local.md)
