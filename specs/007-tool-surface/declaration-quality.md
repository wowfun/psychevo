---
name: 007. Tool Declaration Quality
psychevo_self_edit: deny
---

Define first-slice quality expectations for model-visible tool declarations.
This attachment belongs to [007 Tool Surface](spec.md); it does not freeze a
stable public schema or provider-specific wire format.

## Scope

- model-visible descriptions attached to tool declarations
- descriptions for JSON-schema properties that a model must choose or fill
- recursive description coverage for nested object and array item properties
- description guidance for ambiguous names, enum values, defaults, bounds, and
  compatibility aliases

Out of scope:

- concrete tool behavior and result contracts
- provider-specific schema translation
- public Rust APIs or stable JSON schema snapshots
- prompt wording outside tool declarations

## Description Contract

Every model-visible tool declaration should include a concise tool-level
description that says when to use the tool and names the most important
behavioral constraint.

Every input property that a model can supply should include a `description`
unless the property is an intentionally empty arbitrary JSON value whose parent
description fully explains its meaning. Descriptions should be present for
nested object properties and for properties inside array item schemas, not only
top-level parameters.

Descriptions must disambiguate vague or overloaded names. In particular:

- target identifiers must say whether they accept an agent id, task label,
  skill name, path, source identifier, or another handle type.
- boolean flags must say what `true` changes from the default behavior.
- numeric bounds and defaults should be described when they affect waiting,
  truncation, pagination, spawn depth, or execution limits.
- enum-valued actions should summarize when to choose each action at the field
  or tool level.
- compatibility aliases must identify the canonical field and any conflict
  behavior.
- freeform content fields must say whether they replace a whole file/body,
  patch existing material, send a message, or supply a reason.

Tool declarations should avoid placeholders such as empty descriptions or
generic wording that only repeats the field name. A missing description is a
schema quality defect even when the runtime can still execute the tool call.

## Validation

Narrow tests should cover declaration descriptions for tool families whose
schemas are manually authored. Tests should prefer structural assertions over
snapshotting the entire tool inventory so expected changes remain reviewable.

## Related Topics

- [007 Tool Surface](spec.md) defines declaration visibility and binding
  semantics.
- [051 Subagents](../051-agents/subagents.md) defines agent control tool
  semantics.
- [055 Skills](../055-skills/spec.md) defines skill tool semantics.
- [115 Interactive Clarify Tool](../115-interactive-clarify/spec.md) defines
  clarify tool semantics.
