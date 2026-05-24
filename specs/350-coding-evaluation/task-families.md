---
name: 350. Coding Evaluation Task Families Attachment
psychevo_self_edit: deny
---

Define the coding task families supported by the first evaluation domain.

This attachment is part of [350 Coding Evaluation](spec.md).

## Scope

- coding-loop tasks
- prompt A/B tasks
- SWE-style tasks
- shared single-prompt task shape

## Shared Shape

A coding task presents one initial instruction to the candidate. The agent is
responsible for reading the repository, editing files, running commands, and
deciding when it is done. The evaluation harness does not conduct a multi-turn
conversation in the first slice.

Task setup may create files, install local dependencies, reset a repository,
or prepare fixture state before the agent starts. Setup is not part of the
candidate trajectory unless the domain adapter explicitly records it as task
context.

## Coding Loop

Coding-loop tasks measure ordinary coding-agent behavior: understanding a
repository, making a targeted change, using local tools, validating the change,
and returning an answer. Scoring usually runs local deterministic tests or
checks final file state.

## Prompt A/B

Prompt A/B tasks evaluate factor changes. The task body and oracle stay stable
while factors such as system prompt, mode prompt, skill selection, toolset,
model, or agent adapter change. Reports compare outcomes across factor values.

Prompt A/B is not a separate runner. It is matrix expansion applied to coding
tasks.

## SWE-style

SWE-style tasks represent an issue statement and a repository base state. The
candidate modifies the workspace. Scoring uses tests, an official harness, or a
domain scorer. If an official harness expects a patch, the evaluation system
may generate it temporarily without keeping it as a report artifact.

## Related Topics

- [350 Scoring](scoring.md)
- [330 SWE-bench](../330-benchmark-integrations/swe-bench.md)
