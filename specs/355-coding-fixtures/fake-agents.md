---
name: 355. Coding Fixtures Fake Agents Attachment
psychevo_self_edit: deny
---

Define deterministic fake candidates used by coding-evaluation tests.

This attachment is part of [355 Coding Fixtures](spec.md).

## Scope

- fake candidate roles
- deterministic trajectory behavior
- failure-path coverage
- adapter independence

## Fake Candidate Behavior

Fake candidates should model observable agent behavior without calling real
providers. They can edit fixture files, emit trajectory events, simulate tool
calls, return final answers, time out, or fail intentionally.

The fake behavior must be deterministic from manifest input. It should not read
user config, require API keys, use current time for assertions, or depend on
host-specific environment variables beyond explicit test inputs.

## Coverage Roles

The fixture set should include fake candidates for:

- successful coding change
- incorrect final workspace
- runtime failure
- timeout
- lossy trajectory source

These fakes support `peval run`, report rendering, adapter normalization, and
failure-matrix validation without involving Psychevo, OpenCode, or Hermes live
execution.

## Related Topics

- [340 Testing](../340-agent-evaluation/testing.md)
- [300 Testing](../300-peval-cli/testing.md)
