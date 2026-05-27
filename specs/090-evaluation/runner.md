---
name: 090. Evaluation Runner Attachment
psychevo_self_edit: deny
---

Define the generic evaluation runner lifecycle shared by concrete framework,
CLI, and domain specs.

This attachment is part of [090 Evaluation](spec.md).

## Scope

- task-set expansion into executable cases
- attempt lifecycle
- environment and evaluator phases
- generic failure classes
- result aggregation boundaries

Out of scope:

- command-line process behavior
- concrete async runtime or Rust trait names
- benchmark-specific evaluator checks or official harness commands

## Lifecycle

An evaluation runner expands selected task sets and candidates into cases,
prepares an environment for each case, executes the selected candidate, runs
the selected evaluator, records artifacts, and aggregates results.

The generic attempt lifecycle is:

1. validate task-set, candidate, factor, and task configuration
2. prepare the environment
3. start trajectory capture
4. invoke the candidate
5. collect final workspace or harness state needed for scoring
6. run evaluator checks or import benchmark score
7. write structured result and trajectory artifacts
8. release or retain environment resources according to retention policy

## Matrix Expansion

Factors are expanded before execution. The runner must preserve the expanded
factor values in each case result so reports can compare agents, prompts,
models, toolsets, skills, permissions, and benchmark splits without relying on
file names or command output.

Matrix expansion must be deterministic for identical manifest inputs.

## Failure Classes

The runner records failures instead of discarding the case:

- `setup_error`: configuration, dataset, dependency, or environment setup failed
- `runtime_error`: the candidate process or native adapter failed during agent
  execution
- `scoring_error`: evaluator, oracle, or harness import failed after execution
- `timeout`: the case exceeded a configured bound
- `cancelled`: caller or control plane stopped the run
- `skipped`: the case was intentionally not executed

Lower specs may add domain-specific details, but aggregate reports must be able
to distinguish setup, runtime, and scoring failures.

## Environment Boundary

The runner owns the evaluation environment lifecycle. Candidate adapters may
execute tools or commands inside that environment, but they must not choose a
different task workspace, host state root, or credential source without an
explicit manifest or framework rule.

## Related Topics

- [090 Adapters](adapters.md)
- [090 Artifacts](artifacts.md)
- [095 Execution](../095-evaluation-framework/execution.md)
