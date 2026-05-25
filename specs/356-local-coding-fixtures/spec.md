---
name: 356. Local Coding Fixtures
psychevo_self_edit: deny
---

# 356. Local Coding Fixtures

Define the local deterministic coding fixture suite used by `psychevo-eval`.

## Scope

- local `local-coding` fixture inventory
- fake pass/fail candidate behavior
- deterministic local scorers
- report diagnostics for coding observability

## Fixture Inventory

`local-coding` is the main local coding fixture project. It covers three task
families:

- `coding-loop`: read existing files, make a small deterministic edit, and pass
  a local scorer
- `prompt-ab`: compare prompt/policy variants while keeping the oracle stable
- `swe-style`: issue-to-patch workflow judged by local tests

Fixtures must be tiny, self-contained, deterministic, and runnable without
network access, package installation, credentials, global git config, or host
services.

## Reports

Reports should improve diagnosis without retaining code diffs or patch
artifacts by default. Report rows include task family, failure class, scorer
message/details, trajectory links, and artifact links. Case workspaces are not
retained by default.

## Related Topics

- [350 Coding Evaluation](../350-coding-evaluation/spec.md)
- [355 Coding Fixtures](../355-coding-fixtures/spec.md)
