---
name: 090. Evaluation
psychevo_self_edit: deny
---

# 090. Evaluation

Define Psychevo's foundation contracts for evaluating agents, benchmarks,
tasks, runs, results, and artifacts. This topic is the hard base layer for all
evaluation-related specs.

## Scope

- evaluation terminology shared by framework, CLI, and domain specs
- schema-versioning and artifact expectations for persisted evaluation data
- generic runner and adapter boundaries
- baseline privacy, retention, and live-run safety rules
- dependency boundaries between evaluation, runtime, CLI, and domain layers

Out of scope:

- concrete Rust crate APIs; see [095 Evaluation Framework](../095-evaluation-framework/spec.md)
- command spelling or process behavior for `peval`; see
  [300 peval CLI](../300-peval-cli/spec.md)
- coding-specific task semantics; see
  [350 Coding Evaluation](../350-coding-evaluation/spec.md)
- concrete Psychevo, OpenCode, Hermes, Harbor, or SWE-bench integration details

## Layering Contract

`090-evaluation` constrains every lower evaluation spec. Lower specs may refine
the foundation contracts for a concrete framework, CLI, or domain, but must not
silently redefine the terms, schema-versioning rules, privacy boundaries, or
adapter responsibilities defined here.

The evaluation spec layers are:

- `090-evaluation`: hard foundation contracts.
- `095-evaluation-framework`: `psychevo-eval` framework and library contracts.
- `300-peval-cli`: command-line projection of the framework.
- `330-benchmark-integrations`: concrete benchmark-source integration
  contracts.
- `340-agent-evaluation`: concrete agent-evaluation adapter contracts.
- `350-359`: coding-evaluation domain segment.
- Later domain segments use their own reserved ranges, such as `360-369` for a
  future non-coding evaluation domain.

## Terms

An evaluation `suite` is a named collection of tasks and run configuration. A
`task` is one benchmark unit with instructions, setup requirements, allowed
environment behavior, and scoring expectations. A `candidate` is the evaluated
agent or agent configuration. A `factor` is one matrix dimension, such as
agent, prompt, model, toolset, skill, permission mode, or benchmark split.

An evaluation `run` is one execution of an expanded matrix. A `case` is one
expanded candidate/task/factor combination. An `attempt` is one execution of a
case. A `score` is the normalized outcome produced by an oracle, scorer, or
benchmark harness. A `trajectory` is the time-ordered event record used for
analysis and replay.

## Dependency Boundaries

Evaluation is not part of the core agent loop. Foundation specs must keep
runtime, CLI, and benchmark responsibilities separate:

- `psychevo-runtime` owns stable agent execution capabilities and must not
  absorb benchmark orchestration logic.
- `psychevo-eval` may call stable runtime APIs through explicit adapters.
- `peval` owns command-line process behavior for evaluation.
- Domain specs own task semantics and concrete integrations.

## Safety Contract

Evaluation defaults must be deterministic and local where practical. Real
provider calls, live agent runs, network access for task workspaces, and access
to user credentials must be explicit in the concrete framework or CLI layer.

Local artifacts may retain raw diagnostic material for replay and debugging.
Reports and shareable exports must redact secrets, headers, and environment
values by default unless a lower spec defines an explicit raw-export opt-in.

## Attachments

- [Schema](schema.md) defines shared versioning and file-format expectations.
- [Runner](runner.md) defines the generic run lifecycle and failure model.
- [Adapters](adapters.md) defines generic agent, benchmark, and collector
  adapter responsibilities.
- [Artifacts](artifacts.md) defines persisted result, trajectory, retention,
  and privacy rules.

## Related Topics

- [001 Architecture](../001-architecture/spec.md)
- [060 Automation](../060-automation/spec.md)
- [095 Evaluation Framework](../095-evaluation-framework/spec.md)
- [300 peval CLI](../300-peval-cli/spec.md)
- [350 Coding Evaluation](../350-coding-evaluation/spec.md)
