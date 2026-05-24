---
name: 095. Evaluation Framework Manifest Attachment
psychevo_self_edit: deny
---

Define framework manifest contracts for suites, agents, runs, and factor
expansion.

This attachment is part of [095 Evaluation Framework](spec.md).

## Scope

- human-authored manifest format
- schema-version requirements
- suite, agent, benchmark, model, and factor declarations
- environment and credential allowlist declarations

Out of scope:

- exact CLI flag names
- external benchmark-native schemas
- domain-specific task-family fields

## Format

Framework-owned manifests use TOML. External benchmark adapters may read their
native source formats, but must translate those sources into the framework's
canonical suite and task model before execution.

Every manifest must identify its `schema_version`. Readers reject unsupported
versions before matrix expansion.

## Project Layout

The first implementation uses a Cargo-like directory project rooted at
`eval.toml`. The root manifest contains project defaults, the schema version,
the default output root, and the live-execution gate. `allow_live` defaults to
`false` when omitted.

Project-relative manifests are organized as:

- `agents/*.toml` for fake and Psychevo agent declarations
- `suites/*.toml` for suite identity, matrix entries, and task sources
- `tasks/**/task.toml` for local coding tasks, workspace sources, prompts, and
  scorer commands

All paths inside these manifests resolve relative to the manifest that owns the
field unless a field explicitly says it is project-relative. The first
implementation does not perform schema migration; an unsupported
`schema_version` is a hard validation error with a clear diagnostic.

## Manifest Concepts

A suite manifest may declare:

- suite identity and description
- benchmark source or local task source
- agents or agent presets
- canonical models and per-agent model mappings
- factor matrix entries
- environment provider selection
- network and credential policy
- task limits or sample selection
- output and retention defaults

An agent manifest may declare:

- preset name or custom adapter kind
- command, arguments, working directory, and environment overrides for wrapper
  adapters
- native adapter options for in-process adapters
- collector source selection
- model mapping and provider credential allowlist
- readiness requirements

The first fake adapter is available in default builds and default validation.
The Psychevo adapter lives behind an explicit adapter boundary. Any run or
check path that would execute a real Psychevo/provider route must be rejected
unless the project manifest explicitly sets `allow_live = true`.

## Factor Expansion

Factors are first-class configuration. Agent comparison, prompt A/B, model
comparison, permission comparison, skill/toolset comparison, and benchmark split
selection all use the same expansion mechanism.

Expansion must be deterministic. Expanded case metadata must be recorded in
artifacts exactly enough for reports to reconstruct the comparison matrix.

## Credentials and Environment

Manifests must use allowlists for credentials and host environment variables.
Implicit inheritance of user config, shell environment, or agent home state is
not the default framework behavior.

Concrete adapters may define named convenience presets, but presets still
resolve to explicit manifest-equivalent settings before execution.

## Related Topics

- [095 Execution](execution.md)
- [090 Schema](../090-evaluation/schema.md)
- [300 Commands](../300-peval-cli/commands.md)
