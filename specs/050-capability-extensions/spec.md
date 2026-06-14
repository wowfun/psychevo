---
name: 050. Capability Extensions
psychevo_self_edit: deny
---

Define the boundary for capability contributions from built-in, runtime-provided, or external sources before they become part of an agent invocation.

## Scope

- capability sources and capability contributions at the semantic level
- source identity required for provenance, conflict handling, and observability
- discovery, activation, availability, and agent-invocation scoped selection boundaries
- conflict boundaries before contributions enter an agent invocation
- extension point categories that may contribute capabilities to adjacent specs
- evidence relationship for extension facts that affect agent-invocation assembly

Out of scope:
- plugin manifests, install, update, remove, discovery paths, package formats, marketplaces, hot reload, startup protocols, shutdown protocols, or healthcheck protocols
- concrete hook names, event APIs, return payloads, interception mechanics, CLI commands, UI APIs, or SDK APIs
- concrete tool names, tool schemas, tool result formats, provider wire formats,
  memory provider APIs, context engine APIs, or skill package formats
- permission rules, approval UX, sandboxing, security policy, storage schemas, persistence formats, Rust APIs, or payload schemas

## Capability Flow

A capability extension is a boundary through which a source may contribute capabilities to Psychevo without changing core execution, AI protocol, runtime, interface, or storage semantics.

A capability source is a built-in, runtime-provided, or external origin for capability contributions. A plugin is one possible source form, but this spec does not define plugin mechanics.

A capability contribution is a candidate capability provided by a source. Discovery only creates candidates. A discovered contribution is not automatically active, available, selected for an agent invocation, model-visible, executable, or persisted.

Source identity is the semantic provenance of a source or contribution. Each registered contribution must be relatable to source identity for conflict handling, agent-invocation inspection, and evidence linkage. This spec does not define identifier names, identifier formats, paths, manifests, package names, namespaces, or source records.

Activation is a semantic gate that allows a source or contribution to participate in selection. Activation does not define enable or disable configuration, install UX, defaults, trust policy, or product behavior.

An availability signal is available, unavailable, or degraded. Availability may apply to a source or to an individual contribution. Degraded means only part of the source or contribution is usable. Degraded is a selection signal, not a lifecycle or healthcheck protocol.

An eligible contribution is activated, available or degraded, and conflict-resolved. Only eligible contributions may enter agent-invocation scoped selection.

Agent-invocation scoped selection is runtime resolution of eligible contributions into the capabilities used by one accepted agent invocation. `psychevo-runtime` owns extension resolution into agent-invocation scoped selections. An implementation may use a runtime-adjacent manager or registry, but extensions must not bypass runtime assembly, refreshable tool declaration snapshots, tool binding checks, or lower-layer semantics.

## Contributions

Capability contribution categories are non-exhaustive. They may include:
- tool declarations and execution bindings
- toolsets or toolset includes
- AI provider adapters
- context candidates or context adjuncts
- resource gates or resource adjuncts
- memory candidates or memory providers
- interface adjuncts
- skill discovery, index, view, and management contributions
- MCP server sources that contribute tool candidates, including ACP-supplied
  session-scoped sources

Each contribution category keeps its own source-of-truth semantics. Tool surface and toolset expansion semantics remain owned by [007 Tool Surface](../007-tool-surface/spec.md). AI protocol semantics remain owned by [003 AI Protocol](../003-ai-protocol/spec.md). Context projection remains owned by [006 Context Assembly](../006-context-assembly/spec.md). Resource gate semantics remain owned by [009 Resource Surface](../009-resource-surface/spec.md). Memory boundaries remain owned by [010 Memory System](../010-memory-system/spec.md). Skills remain owned by [055 Skills](../055-skills/spec.md). Caller-facing interface semantics remain owned by [020 Interfaces](../020-interfaces/spec.md).

Capability extensions may add candidates or constraints, but they must not turn candidates into model context, model-visible tools, executable operations, retained memory, provider protocol semantics, or caller-facing behavior without passing through the owning boundary.

## Conflicts

A conflict exists when multiple sources or contributions cannot all be selected while preserving the relevant owning semantics.

Conflicts must be resolved before agent-invocation scoped selection. Unresolved conflicts must not enter agent-invocation scoped selection.

Conflict handling may use omission, precedence, namespacing, replacement, rejection, or other policy defined by a later spec or implementation. This spec does not define conflict priority, override rights, namespace syntax, or diagnostic payloads.

Conflict facts that affect agent-invocation assembly should remain observable. Toolset conflicts, include conflicts, unknown includes, unavailable includes, and cycles are contribution or selection facts before they become expanded tool surface facts.

## Extension Points

Extension points are boundaries where capability contributions may be considered by runtime or adjacent specs.

This spec permits extension points but does not define hook names, lifecycle events, callback signatures, event ordering, return semantics, interception behavior, command APIs, UI APIs, or SDK APIs.

Extension points must preserve source-of-truth ownership. A contribution may provide a tool candidate, but [007 Tool Surface](../007-tool-surface/spec.md) owns whether it becomes part of the agent-invocation scoped tool surface. A contribution may provide context candidates, but [006 Context Assembly](../006-context-assembly/spec.md) owns model visibility. A contribution may provide resource gates, but [009 Resource Surface](../009-resource-surface/spec.md) owns resource decision semantics.

Future lifecycle or tool-refresh APIs must route through runtime-owned selection and the owning source-of-truth specs. They must distinguish session lifecycle from agent-invocation lifecycle and must not treat `agent_end` as a session close, reset, expiry, or lifecycle-end event.

## Evidence Relationship

Extension facts that affect agent-invocation assembly should be observable. These facts may include selected contributions, omitted unavailable contributions, degraded contributions that affected selection, conflicts that caused omission or resolution, tool-refresh contribution facts, and source identity for selected contributions.

Durable evidence and persistence remain owned by adjacent specs. [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics for inspectable agent-invocation facts. [030 State and Data Model](../030-state-and-data-model/spec.md) defines state relationships. [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines persistence boundaries.

This spec does not require every discovered source, every candidate contribution, every conflict, or every availability signal to become durable.

ACP-provided MCP sources are session-scoped capability sources. They must enter
runtime contribution normalization before any MCP tool becomes model-visible or
executable. ACP source presence does not imply trust, activation, selection, or
permission approval.

## First Implementation Slice

The first implementation slice normalizes current runtime-owned capabilities
without changing user-visible behavior. Runtime records the source,
contribution, selection, and evidence vocabulary for the capability surfaces it
already assembles while treating ordinary capability assembly as reconstructable
runtime state.

This slice covers current tools, toolsets, MCP tool inputs, skills, agents,
providers, and context assembly facts. Memory and resource categories may appear
as capability categories only when the runtime has an existing source or
selection fact to record; the implementation must not invent memory or resource
behavior to satisfy this normalization layer.

Tool contributions enter a runtime-owned tool router. The router is responsible
for the model-visible tool declaration snapshot, dispatch lookup, duplicate
visible-name rejection, and current invocation dispatch facts. It may represent
direct, deferred, and hidden exposure, but the first slice does not add
model-visible tool search or model-initiated dynamic activation. Deferred
activation is an internal runtime API until a later spec defines caller-facing
or model-facing activation behavior.

Prompt-prefix records freeze the request reconstruction boundary. They retain
the prompt slots, prompt hash, model-visible tool declaration hash, and minimal
runtime metadata needed to reconstruct or mark approximate a later provider
request. Ordinary selected/omitted capability lists are not a separate durable
truth source.

Capability conflicts, unavailable sources, rejected contributions, degraded
sources, or deferred activation outcomes may be surfaced as current-run
warnings or future explicit evidence records when another spec requires durable
inspection. The first slice does not persist a canonical capability snapshot,
ordered selection event list, or full selected contribution inventory.

Because Psychevo is still pre-release, this slice may advance the local state
schema directly instead of carrying migration code for earlier internal schema
versions. Old development databases may require reset or replacement.

Capability summaries must avoid storing payloads that already belong to
adjacent evidence surfaces. Skill bodies, agent instructions, provider secrets,
raw provider payloads, full context text, and full tool declaration payloads do
not belong in capability extension state by default. Context content remains
governed by [006 Context Assembly](../006-context-assembly/spec.md) and current
context evidence.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and runtime ownership.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral AI protocol semantics.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines agent-invocation assembly and wiring responsibilities.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics for final agent-invocation facts.
- [006 Context Assembly](../006-context-assembly/spec.md) defines context projection and model visibility boundaries.
- [007 Tool Surface](../007-tool-surface/spec.md) defines agent-invocation scoped tool surface semantics.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource surface and resource decision semantics.
- [010 Memory System](../010-memory-system/spec.md) defines optional memory boundaries.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing interface semantics.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines semantic state relationships.
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines persistence boundaries for durable semantic facts.
- [055 Skills](../055-skills/spec.md) defines skill packages, discovery, tools, CLI commands, scanning, and provenance.
- [056 MCP](../056-mcp/spec.md) defines MCP source, naming, dispatch, permission, and evidence boundaries.
- [027 ACP](../027-acp/spec.md) defines ACP-provided MCP source projection.
