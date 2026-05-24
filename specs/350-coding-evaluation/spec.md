---
name: 350. Coding Evaluation
psychevo_self_edit: deny
---

# 350. Coding Evaluation

Define the coding-evaluation domain. This topic explains what it means to
evaluate coding agents without binding the domain to any specific agent,
benchmark source, or command-line product behavior.

## Scope

- coding-loop, prompt A/B, and SWE-style task families
- coding workspace and scorer expectations
- coding-specific success and process metrics
- boundaries for code-change artifacts

Out of scope:

- concrete Psychevo, OpenCode, or Hermes adapter commands; see
  [340 Agent Evaluation](../340-agent-evaluation/spec.md)
- concrete local, Harbor, or SWE-bench benchmark bridges; see
  [330 Benchmark Integrations](../330-benchmark-integrations/spec.md)
- fixture inventory; see [355 Coding Fixtures](../355-coding-fixtures/spec.md)

## Domain Shape

Coding evaluation tasks use a single initial prompt and then let the agent run
its own tool loop. Setup scripts prepare the task workspace. Scorers or
official harnesses decide success after the agent attempt finishes.

The domain supports three first-class task families:

- coding-loop tasks for general repository modification and validation
- prompt A/B tasks for comparing prompts, system instructions, skills, or
  tool policies
- SWE-style tasks for issue-to-patch workflows judged by tests or an official
  harness

The first implementation slice validates the domain with one local Rust
SWE-style task, fake pass/fail agents, and a deterministic local scorer. It
does not claim benchmark coverage beyond proving the domain shape end to end.

## Workspace Rule

Each case starts from an isolated task workspace. The scorer may inspect the
final workspace, but coding reports do not persist code diffs or patch
artifacts by default. SWE-style scoring may generate an ephemeral patch for a
harness and then discard it after score import.

## Attachments

- [Task Families](task-families.md) defines the first coding task families.
- [Scoring](scoring.md) defines oracle, scorer, and process metric rules.
- [Testing](testing.md) defines domain validation expectations.

## Related Topics

- [090 Evaluation](../090-evaluation/spec.md)
- [095 Evaluation Framework](../095-evaluation-framework/spec.md)
- [340 Agent Evaluation](../340-agent-evaluation/spec.md)
- [330 Benchmark Integrations](../330-benchmark-integrations/spec.md)
