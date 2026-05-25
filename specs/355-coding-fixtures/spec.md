---
name: 355. Coding Fixtures
psychevo_self_edit: deny
---

# 355. Coding Fixtures

Define deterministic fixture suites for validating the coding-evaluation
domain, agent evaluation, benchmark bridges, and `peval` CLI without live
providers or official benchmark downloads.

## Scope

- minimal coding fixture suite requirements
- fake candidate behavior
- deterministic scorer behavior
- fixture artifact expectations

Out of scope:

- real Psychevo, OpenCode, or Hermes runs
- official Harbor or SWE-bench datasets
- performance benchmarking

## Fixture Role

Fixtures are not benchmark claims. They are small, deterministic examples that
prove the evaluation framework can load tasks, expand factors, run fake
candidates, score outcomes, write artifacts, and render reports.

The first fixture set is the `local-coding` project. It covers one
coding-loop task, one prompt A/B task, and one SWE-style task. It includes
passing and failing fake candidate paths and keeps all scoring local and
deterministic. Older `local-rust-swe` references are historical and should be
renamed or migrated to `local-coding` rather than remaining the main fixture
path.

## Attachments

- [Fixtures](fixtures.md) defines the local task fixture shape.
- [Fake Agents](fake-agents.md) defines deterministic candidate behavior.
- [Testing](testing.md) defines fixture validation expectations.

## Related Topics

- [300 peval CLI](../300-peval-cli/spec.md)
- [350 Coding Evaluation](../350-coding-evaluation/spec.md)
- [340 Agent Evaluation](../340-agent-evaluation/spec.md)
- [330 Benchmark Integrations](../330-benchmark-integrations/spec.md)
