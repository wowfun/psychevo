---
name: 300. peval CLI Testing
psychevo_self_edit: deny
---

Define deterministic validation for the `peval` command-line surface.

## Scope

- CLI parsing and help coverage
- offline check behavior
- artifact path and report rendering behavior
- fake adapter command integration

Out of scope:

- real provider calls
- OpenCode, Hermes, or Psychevo live agent execution
- official benchmark downloads or harness runs

## Deterministic Coverage

CLI tests should use temporary homes, temporary output roots, fake manifests,
fake agents, fake scorers, and local fixture suites. They should verify that
`doctor`, `list`, `check`, `run`, `report`, `compare`, and `replay` can be
exercised without user credentials.

`peval check` coverage must prove that live provider work is not triggered.
`peval run` coverage may execute fake agents and local scorers only.

Report tests should assert structured report data, redaction behavior, and
presence of local artifact links. They should avoid brittle snapshots of full
HTML when structured comparison can cover the same behavior.

## Validation

The default project validation path must not require Python sidecar
dependencies, Docker, provider API keys, official datasets, or installed third
party agents. Those checks belong behind explicit live or integration
validation commands.

## Related Topics

- [300 Commands](commands.md)
- [300 Reporting](reporting.md)
- [355 Coding Fixtures](../355-coding-fixtures/spec.md)
