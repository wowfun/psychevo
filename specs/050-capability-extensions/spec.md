---
name: 050. Capability Extensions
psychevo_self_edit: deny
---

Define the broad capability-extension boundary for sources that may affect
runtime assembly without receiving direct runtime authority.

## Scope

- capability-extension sources and declarations at the semantic level
- source identity for provenance, conflict handling, and observability
- discovery, activation, availability, and invocation-scoped acceptance
- conflict boundaries before declarations enter runtime assembly
- mapping from accepted declarations into typed contributors or owning runtime
  modules
- evidence relationship for extension registry facts that affect invocation
  assembly

Out of scope:
- plugin package formats, manifests, install, update, remove, marketplaces, hot
  reload, startup protocols, shutdown protocols, or healthcheck protocols
- concrete hook schemas, hook payloads, hook return fields, CLI commands, UI
  APIs, or SDK APIs
- concrete tool names, tool schemas, tool result formats, provider wire
  formats, memory provider APIs, context engine APIs, or skill package formats
- permission rules, approval UX, sandboxing, security policy, storage schemas,
  persistence formats, Rust APIs, or payload schemas

## Capability Extension Model

A capability extension is a source boundary through which built-in,
runtime-provided, package-provided, or interface-provided material may be
considered by Psychevo.

A source is an origin for declarations. Examples include built-in runtime
features, plugin packages, MCP server inputs, profile or project configuration,
selected agent definitions, managed policy, Gateway inputs, ACP session inputs,
and future peer-agent adapters.

A declaration is source-provided candidate material. Discovery only creates
candidates. A discovered declaration is not automatically active, available,
accepted, model-visible, executable, trusted, persisted, or permitted.

Source identity is the semantic provenance of a source or declaration. Each
accepted declaration must remain relatable to source identity for conflict
handling, invocation inspection, diagnostics, and evidence linkage. This spec
does not define identifier strings, path syntax, package names, namespaces, or
storage records.

Activation is a semantic gate that allows a source or declaration to
participate in acceptance. Activation does not define enable or disable
configuration, install UX, defaults, trust policy, or product behavior.

Availability is `available`, `unavailable`, or `degraded`. Availability may
apply to a source or to one declaration. Degraded means only part of the source
or declaration is usable. Availability is an acceptance signal, not a lifecycle
or healthcheck protocol.

An accepted declaration is activated, available or degraded, conflict-resolved,
and admitted by the owning runtime module for one invocation or session scope.
Only accepted declarations may become typed contributors, hook declarations,
tool bindings, MCP inputs, context candidates, or other runtime-owned effects.

If a selected capability source contains a recognized package manifest, that
manifest is the source boundary. A malformed recognized manifest must fail
closed for that source instead of falling back to treating the package directory
as an unrelated skill root or generic directory source.

## Registry Relationship

The runtime extension registry is the primary runtime interface for extension
effects. Accepted declarations are mapped by Psychevo host code into typed
contributors or into owning runtime modules that themselves feed the registry.

The registry interface is defined in the
[Runtime Extension Registry](runtime-extension-registry.md) attachment. This
spec owns the broader source and declaration vocabulary; the attachment owns
`ExtensionRegistry`, `ExtensionData`, typed contributor slots, frozen registry
views, and compact registry evidence.

Sources must not mutate the registry directly. A plugin package, MCP server
declaration, selected agent, skill root, profile setting, project setting,
managed policy, or interface input may cause contributors to be installed only
through Psychevo host code.

## Declaration Families

Declaration families are non-exhaustive. They may include:

- MCP server declarations
- tool declarations and execution bindings
- toolset declarations
- context and turn-input candidates
- resource candidates or resource gates
- memory candidates or memory providers
- skill roots or skill providers
- hook declarations
- agent and peer-agent descriptors
- provider-adjacent descriptors
- command descriptors
- interface or marketplace metadata

Each family keeps its owning semantics. Tool surface and toolset expansion stay
owned by [007 Tool Surface](../007-tool-surface/spec.md). AI protocol semantics
stay owned by [003 AI Protocol](../003-ai-protocol/spec.md). Context projection
stays owned by [006 Context Assembly](../006-context-assembly/spec.md).
Resource gates stay owned by [009 Resource Surface](../009-resource-surface/spec.md).
Memory stays owned by [010 Memory System](../010-memory-system/spec.md).
Skills stay owned by [055 Skills](../055-skills/spec.md). Hooks stay owned by
[053 Hooks](../053-hooks/spec.md) and [140 Hook Runtime](../140-hook-runtime/spec.md).
Plugin package mechanics stay owned by [054 Plugins](../054-plugins/spec.md),
[150 Plugin Runtime](../150-plugin-runtime/spec.md), and
[155 Plugin Manifest](../155-plugin-manifest/spec.md).

Capability extensions may add candidates or constraints, but they must not turn
candidates into model context, model-visible tools, executable operations,
retained memory, provider protocol semantics, persistent permission grants, or
caller-facing behavior without passing through the owning module.

## Conflicts

A conflict exists when multiple sources or declarations cannot all be accepted
while preserving the relevant owning semantics.

Conflicts must be resolved before a declaration affects an accepted invocation.
Unresolved conflicts must not enter the runtime extension registry, the tool
surface, context assembly, hook execution, provider resolution, or any other
owning module.

Conflict handling may use omission, precedence, namespacing, replacement,
rejection, or another policy defined by the owning module. This spec does not
define conflict priority, override rights, namespace syntax, or diagnostic
payloads.

Conflict facts that affect invocation assembly should remain observable.
Toolset conflicts, include conflicts, unknown includes, unavailable includes,
duplicate visible names, and cycles are source/declaration facts before they
become expanded tool surface facts.

## Evidence Relationship

Extension registry facts that affect invocation assembly should be observable.
These facts may include selected contributors, omitted unavailable contributors,
degraded contributors that changed assembly, conflicts that caused omission or
resolution, visibility decisions, and source identity for accepted effects.

Durable evidence and persistence remain owned by adjacent specs. [005 Durable
Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics
for inspectable invocation facts. [030 State and Data Model](../030-state-and-data-model/spec.md)
defines state relationships. [031 Storage and Persistence](../031-storage-and-persistence/spec.md)
defines persistence boundaries.

This spec does not require every discovered source, candidate declaration,
conflict, or availability signal to become durable.

## Current Implementation Slice

The current implementation slice should normalize existing runtime-owned
extension surfaces without changing user-visible behavior.

This slice covers current tools, toolsets, MCP inputs, skills, agents,
providers, hooks, plugin declarations, and context assembly facts only where
runtime already has a source or acceptance fact to record. Memory and resource
families may appear only when runtime has an existing source or acceptance fact;
the implementation must not invent memory or resource behavior to satisfy this
normalization layer.

Tool declarations enter a runtime-owned tool router. The router owns the
model-visible tool declaration snapshot, dispatch lookup, duplicate visible-name
rejection, and current invocation dispatch facts. It may represent direct,
deferred, and hidden exposure, but the current slice does not add
model-visible tool search or model-initiated dynamic activation.

Prompt-prefix records freeze the request reconstruction boundary. They retain
prompt slots, prompt hash, model-visible tool declaration hash, and minimal
runtime metadata needed to reconstruct or mark approximate a later provider
request. Ordinary selected/omitted extension lists are not a separate durable
truth source.

Conflicts, unavailable sources, rejected declarations, degraded sources, or
deferred activation outcomes may be surfaced as current-run warnings or future
explicit evidence records when another spec requires durable inspection. The
current slice does not persist a canonical registry snapshot, ordered
acceptance event list, or full selected contributor inventory.

Capability summaries must avoid storing payloads that already belong to
adjacent evidence surfaces. Skill bodies, agent instructions, provider secrets,
raw provider payloads, full context text, and full tool declaration payloads do
not belong in capability-extension state by default. Context content remains
governed by [006 Context Assembly](../006-context-assembly/spec.md) and current
context evidence.

## Attachments

- [Runtime Extension Registry](runtime-extension-registry.md) defines
  `ExtensionRegistry`, `ExtensionData`, typed contributor slots, frozen
  registry views, and compact registry evidence.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and runtime ownership.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral AI protocol semantics.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines invocation assembly and wiring responsibilities.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics for final invocation facts.
- [006 Context Assembly](../006-context-assembly/spec.md) defines context projection and model visibility boundaries.
- [007 Tool Surface](../007-tool-surface/spec.md) defines invocation-scoped tool surface semantics.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource surface and resource decision semantics.
- [010 Memory System](../010-memory-system/spec.md) defines optional memory boundaries.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing interface semantics.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines semantic state relationships.
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines persistence boundaries for durable semantic facts.
- [053 Hooks](../053-hooks/spec.md) defines hook authority.
- [054 Plugins](../054-plugins/spec.md) defines plugin package boundaries.
- [055 Skills](../055-skills/spec.md) defines skill packages, discovery, tools, CLI commands, scanning, and provenance.
- [056 MCP](../056-mcp/spec.md) defines MCP source, naming, dispatch, permission, and evidence boundaries.
- [027 ACP](../027-acp/spec.md) defines ACP-provided MCP source projection.
