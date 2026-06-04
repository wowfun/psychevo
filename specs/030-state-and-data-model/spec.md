---
name: 030. State and Data Model
psychevo_self_edit: deny
---

Define Psychevo's semantic state and data model across foundation specs.

## Scope

- semantic state families used by Psychevo sessions and agent invocations
- relationships between session, agent invocation, turn, message, generation, tool, runtime, evidence, resource, memory, and extension facts
- truth-source ownership for state families
- semantic identity and causal relationship requirements
- recoverability classes for durable, reconstructable, and transient facts
- first-slice session record model attachment
- transcript state ownership attachment

Out of scope:
- Rust structs, traits, APIs, modules, or type names
- JSON, HTTP, SDK, CLI, or event payload schemas
- ID formats, field names, tables, files, migrations, indexes, storage engines, or persistence layout
- replay algorithms, deterministic replay guarantees, trace formats, session file formats, or transcript formats
- concrete tool schemas, resource permission schemas, memory provider schemas, provider wire formats, or configuration schemas

## State Model

Psychevo's state model is session-centered. A session is the durable continuity boundary. An agent invocation is a live and semantic relationship for one accepted caller prompt or continuation assembled inside a session boundary.

The core relationship graph is:

```text
Gateway Source -> Gateway Thread -> Session
Session -> Agent Invocation -> Turn -> Message
Session -> AI Generation
Assistant tool request -> Tool execution -> Tool-result message/material
Session -> runtime assembly facts -> context, tool, resource, memory, extension, and evidence relationships
```

Agent Invocation in this graph is a semantic relationship, not a required persistent root, table, public identifier, or storage object.

The first implementation slice centers persistence on sessions and messages. Turn index may be live-visible for observation, but this spec does not require a turn field in message records.

This graph defines semantic relationships only. It does not define storage layout, object identifiers, foreign keys, in-memory structs, wire payloads, or event payloads.

State facts may be live, durable, reconstructable, or derived. A state fact should have one best-fit truth source. Other representations are projections, observations, cached views, or implementation details.
Transcript state follows the same rule: ordinary transcript facts belong to
runtime messages, while product transcript views are projections owned by their
interface specs.

## State Families

Execution state covers agent invocations, turns, loop-visible messages, tool executions, and outcomes. [002 Agent Execution](../002-agent-execution/spec.md) owns these concepts and their lifecycle semantics.

AI generation state covers model targets, generation requests, normalized stream categories, terminal generation outcomes, and optional metadata. [003 AI Protocol](../003-ai-protocol/spec.md) owns these semantics.

Runtime assembly state covers the facts runtime resolves for an agent invocation: session boundary, model and generation selections, provider configuration selection, context projection, resource surface, tool surface, capability extension selections, session continuity inputs, memory hints, control-signal wiring, and evidence sink wiring. [004 Runtime Contract](../004-runtime-contract/spec.md) owns runtime assembly.

Context state covers instruction context, loop-visible context, attached context, summary context, and context projection. [006 Context Assembly](../006-context-assembly/spec.md) owns model visibility and transformation boundaries.

Tool state covers the agent-invocation scoped tool surface, refreshable tool declaration snapshots, execution bindings, assistant tool requests, tool execution outcomes, and tool-result artifacts. [007 Tool Surface](../007-tool-surface/spec.md) owns declaration snapshot and binding semantics. [002 Agent Execution](../002-agent-execution/spec.md) owns tool execution lifecycle semantics.

Live observation state covers active agent-invocation observation, partial assistant output, pending tool executions, and active control signals. [020 Interfaces](../020-interfaces/spec.md) owns caller-facing observation semantics. [002 Agent Execution](../002-agent-execution/spec.md) and [003 AI Protocol](../003-ai-protocol/spec.md) own the underlying execution and AI output categories.

Durable evidence facts cover final session and agent-invocation facts and causal relationships required for inspection and future replay work. [005 Durable Evidence](../005-durable-evidence/spec.md) owns durable evidence requirements.

Gateway state covers source identity, source-to-thread binding, active-turn control handles, in-memory queues, and caller-facing item/event projections. [021 Gateway](../021-gateway/spec.md) owns gateway state semantics. Persistent source-to-thread bindings may be durable; invocation and process source bindings, active queues, and live control handles are transient.

Session continuity state covers session identity, session lifecycle semantics, parent session lineage, and continuity inputs. [008 Session Continuity](../008-session-continuity/spec.md) owns session continuity semantics.

Resource state covers resource facts, resource operations, access gates, and resource decisions. [009 Resource Surface](../009-resource-surface/spec.md) owns resource surface and decision semantics.

Memory state covers memory candidates, retained memory, memory recall, and memory mutation boundaries. [010 Memory System](../010-memory-system/spec.md) owns memory semantics.

Extension state covers capability source identity, capability contributions, activation, availability, agent-invocation scoped selection, and conflicts. [050 Capability Extensions](../050-capability-extensions/spec.md) owns capability extension semantics.

## Identity and Relationships

State facts must be relatable across their source boundaries. A consumer should be able to connect a session to its agent invocations, turns, messages, AI generations, assistant tool requests, tool executions, tool-result artifacts, runtime assembly facts, resource decisions, durable evidence, continuity inputs, memory-related facts, and capability extension facts when those facts exist.

Semantic identity means a fact can be recognized and related within its owning semantics. This spec does not define identifier names, identifier formats, database keys, storage cursors, field names, or payload shapes.

Causal relationships matter more than one physical log. A model tool request causes or contributes to a tool execution; a tool execution produces a tool-result artifact; a context projection contributes to a generation request; a resource decision may affect model visibility or tool execution; session continuity inputs, memory recall, and capability extension selections may contribute to runtime assembly.

Derived views must not become new truth sources. CLI rendering, SDK responses, live observation views, test harness output, caches, and future transport payloads may expose or summarize state facts, but they must not redefine the underlying semantics.

## Truth Source and Recoverability

Each state family keeps its source-of-truth ownership in its owning spec. This spec maps relationships between those families; it does not move ownership into one crate or storage layer.

Durable facts must be representable through durable evidence or another durable system owned by a later spec. Final loop-visible messages, terminal outcomes, tool request and result relationships, resource decisions that affect execution, and evidence-linked memory facts fall into this class when the owning specs require them.

[040 Storage and Persistence](../040-storage-and-persistence/spec.md) defines the persistence boundary for durable facts. This spec defines recoverability classes, not persistence substrate behavior.

Reconstructable facts may be rebuilt from durable facts, configuration, source material, or provider/runtime capability discovery. Context projections, summary context, selected tool surfaces, tool declaration snapshots, resource facts, capability extension candidates, capability assembly selections, and memory recall candidates may be reconstructable when their source material remains available. This spec does not guarantee deterministic reconstruction.

Request reconstruction should prefer durable prompt-prefix evidence, message
metadata, context evidence, and current runtime/provider registries over a
separate durable capability sidecar. If a reconstructed tool declaration
snapshot cannot be verified against the recorded declaration hash, the consumer
must mark the reconstruction approximate instead of treating current registry
state as the original request.

Transient facts exist while an agent invocation or runtime operation is active. Partial assistant output, pending tool executions, live observation buffers, in-flight control signals, temporary resource operations, active runtime handles, and raw runtime diagnostic observations may disappear after settlement, failure, abort, or process loss unless another spec requires durable evidence for their final effect.

Generic runtime debug observations are not ordinary durable facts, request
reconstruction facts, or transcript facts. A future diagnostic store must be
defined by a domain-specific spec with explicit payload and retention policy,
not inferred from the state model.

Recoverability class is semantic. It does not define persistence format, retention policy, retry behavior, cleanup behavior, or replay behavior.

## Attachments

- [Session Record Model](session-record-model.md) defines the first implementation slice contract for session and message records.
- [Transcript State](transcript-state.md) defines ordinary transcript fact
  ownership and recoverability boundaries.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and dependency direction.
- [002 Agent Execution](../002-agent-execution/spec.md) defines execution state concepts and outcomes.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines AI generation state and metadata boundaries.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime assembly state.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence requirements for final facts.
- [006 Context Assembly](../006-context-assembly/spec.md) defines context state and projection boundaries.
- [007 Tool Surface](../007-tool-surface/spec.md) defines tool surface state.
- [008 Session Continuity](../008-session-continuity/spec.md) defines session continuity state.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource state and decision semantics.
- [010 Memory System](../010-memory-system/spec.md) defines memory state boundaries.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing interface projection for session and agent-invocation state.
- [021 Gateway](../021-gateway/spec.md) defines source-to-thread and live gateway state semantics.
- [040 Storage and Persistence](../040-storage-and-persistence/spec.md) defines persistence boundaries for durable semantic facts.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines capability extension state boundaries.
