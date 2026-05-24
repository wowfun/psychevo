---
name: 355. Coding Fixtures Testing
psychevo_self_edit: deny
---

Define validation expectations for coding fixtures.

## Scope

- fixture loading
- fake candidate execution
- deterministic scorer outputs
- report and artifact sanity checks

Out of scope:

- third-party agent execution
- provider API calls
- official benchmark harnesses

## Deterministic Coverage

Fixture tests should run under temporary directories and isolated environment
variables. They should verify that all required fixture tasks load, fake
candidates run, scorer outputs import, failure classes are recorded, and reports
can be generated from artifacts.

At least one fixture test should cover a full local path from manifest loading
through fake candidate execution, scoring, artifact writing, report rendering,
and replay input loading.

Fixture tests must remain cheap enough for default validation.

## Related Topics

- [355 Fixtures](fixtures.md)
- [355 Fake Agents](fake-agents.md)
- [300 Testing](../300-peval-cli/testing.md)
