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
- coding workspace and evaluator expectations
- coding-specific success and process metrics
- boundaries for code-change artifacts

Out of scope:

- concrete Psychevo, OpenCode, or Hermes adapter commands; see
  [340 Agent Evaluation](../340-agent-evaluation/spec.md)
- concrete local, Harbor, or SWE-bench benchmark bridges; see
  [330 Benchmark Integrations](../330-benchmark-integrations/spec.md)
- checked-in benchmark inventory; see
  [356 Pidx Coding Benchmark](../356-pidx-coding-benchmark/spec.md)

## Domain Shape

Coding evaluation tasks use problem data to produce a single initial prompt and
then let the agent run its own tool loop. The selected evaluator prepares or
copies the task workspace and decides success after the agent attempt finishes.

The domain supports three first-class task families:

- coding-loop tasks for general repository modification and validation
- prompt A/B tasks for comparing prompts, system instructions, skills, or
  tool policies
- SWE-style tasks for issue-to-patch workflows judged by tests or an official
  harness

The first checked-in benchmark is `pidx-coding`. It focuses on incremental
agent-behavior value rather than broad repository validation. It contains small
coding patch, stateful tool-use, and SWE-style tasks with typed local evaluator
checks. Runnable eval configs or registry entries pair that benchmark with
Psychevo, OpenCode, Hermes, or fake agents by explicit task-set/task and agent
selection.

## Workspace Rule

Each case starts from an isolated task workspace. The evaluator may inspect the
final workspace, but coding reports do not persist code diffs or patch
artifacts by default. SWE-style scoring may generate an ephemeral patch for a
harness and then discard it after score import.

Views should make local coding failures diagnosable without retaining the
workspace by default. View metadata includes task family, failure class,
evaluator details, trajectory links, and artifact links. Diff or patch artifacts
remain excluded unless an explicit retained-workspace or debug mode is added in
a later spec.

## Attachments

- [Task Families](task-families.md) defines the first coding task families.
- [Scoring](scoring.md) defines oracle, evaluator, and process metric rules.
- [Testing](testing.md) defines domain validation expectations.

## Related Topics

- [090 Evaluation](../090-evaluation/spec.md)
- [095 Evaluation Framework](../095-evaluation-framework/spec.md)
- [340 Agent Evaluation](../340-agent-evaluation/spec.md)
- [330 Benchmark Integrations](../330-benchmark-integrations/spec.md)
- [356 Pidx Coding Benchmark](../356-pidx-coding-benchmark/spec.md)
