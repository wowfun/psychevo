---
name: 090. Evaluation Adapters Attachment
psychevo_self_edit: deny
---

Define generic adapter responsibilities for agents, benchmarks, collectors, and
official harness bridges.

This attachment is part of [090 Evaluation](spec.md).

## Scope

- candidate adapter boundaries
- benchmark adapter boundaries
- trajectory collector responsibilities
- normalization into foundation result and event concepts

Out of scope:

- concrete agent presets
- exact process protocols
- benchmark-specific data loaders

## Agent Adapters

An agent adapter converts one candidate configuration into an executable
attempt. It may invoke a native library API, spawn an external process, or wrap
an existing CLI. Regardless of mechanism, it must report:

- accepted candidate identity
- command or native execution metadata safe for local diagnostics
- normalized lifecycle events when available
- final status and final answer or equivalent terminal material

Agent adapters must not own scoring. They may collect output needed by
evaluators, but the evaluator or benchmark adapter owns pass/fail and numeric
score decisions.

## Benchmark Adapters

A benchmark adapter converts an external task set, dataset, or task directory into
the common task model. It owns benchmark-specific metadata, setup requirements,
and bridge rules to external harnesses.

Benchmark adapters may delegate environment construction or scoring to an
official harness when a lower spec allows it. Delegated results must still be
imported into the common run and task result model.

## Collectors

A collector observes candidate execution and normalizes events into the
trajectory model. Event streams are the preferred source of truth. Local
databases, session exports, benchmark logs, process stdout/stderr, model
proxies, and tool proxies may supplement the event stream when the adapter
declares the source and normalization rules.

Collectors must preserve enough source metadata to diagnose lossy
normalization. They must redact or classify sensitive material according to
[Artifacts](artifacts.md).

## Adapter Compatibility

Adapters should fail readiness checks before a run when their required command,
runtime feature, dataset source, credentials, or sidecar is unavailable. A run
may still record the failed case and continue with other matrix entries when
the concrete runner layer allows failure continuation.

## Related Topics

- [095 Execution](../095-evaluation-framework/execution.md)
- [340 Agent Evaluation](../340-agent-evaluation/spec.md)
- [330 Benchmark Integrations](../330-benchmark-integrations/spec.md)
