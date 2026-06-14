---
name: 005. Durable Evidence
psychevo_self_edit: deny
---

Define the durable evidence contract for Psychevo sessions and agent invocations.

## Scope

- durable session and agent-invocation evidence semantics
- agent-invocation assembly facts at the semantic level
- finalized loop-visible message artifacts
- AI generation terminal outcomes
- assistant tool request, tool execution outcome, and tool-result linkage
- resource decisions that affect model visibility or tool execution
- capability extension facts that affect agent-invocation assembly
- terminal agent-invocation outcome
- causal relationships between session, agent invocation, turn, message, generation, and tool facts
- optional metadata preservation boundary

Out of scope:
- JSONL, SQLite, schemas, tables, files, serialization, migrations, indexes, or search behavior
- transcript formats, branch, fork, compaction, retry, undo, resume, or storage layouts
- replay algorithms or deterministic replay guarantees
- event stream transport, UI rendering, CLI behavior, or process behavior
- concrete tool declarations, tool bindings, tool schemas, permission schemas, provider wire formats, or provider payload fields
- memory storage, memory retention behavior, memory provider state, or memory retrieval behavior
- secret handling, redaction, retention, privacy policy, or data governance policy

## Evidence Model

Durable evidence is final session and agent-invocation facts connected to an evidence sink by runtime.

Durable evidence records final facts. Streaming progress events may be observed, but this spec does not require every update to become durable evidence.

Durable evidence preserves causal relationships. A consumer should be able to relate session facts, agent-invocation final facts, turns, messages, AI generations, tool requests, tool executions, resource decisions, and terminal outcomes without requiring one global append-only log.

Durable evidence prepares for replay. Future replay work should be able to use durable evidence as an input, but this spec does not define replay behavior or deterministic replay guarantees.

Evidence sink failures must be observable. This spec does not require runtime to fail closed, retry, block, or continue in a specific way when evidence wiring fails.

Durable evidence persistence is governed by [031 Storage and Persistence](../031-storage-and-persistence/spec.md). This spec defines evidence semantics, not persistence substrate behavior.

Durable evidence is the source substrate for session continuity. [008 Session Continuity](../008-session-continuity/spec.md) defines how sessions may select, retain, or organize evidence-backed facts.

Before-agent-start rejection may be exposed as an observable rejection. This spec does not require such a rejection to become a transcript message.

## Required Evidence

Durable evidence must be able to represent agent-invocation assembly facts at the semantic level. Those facts may include the capability target, model target, generation controls, context assembly facts, runtime-injected context material that was model-visible, resource surface facts, toolset selection and expansion facts, tool declaration snapshot facts, tool refresh facts, capability extension selection facts, unavailable or degraded assembly facts, cancellation wiring facts, and evidence sink wiring facts. [006 Context Assembly](../006-context-assembly/spec.md) defines context assembly semantics. [050 Capability Extensions](../050-capability-extensions/spec.md) defines capability extension facts that may affect agent-invocation assembly. This spec does not require full prompt snapshots, context schemas, policy schemas, extension manifests, or concrete configuration fields.

Durable evidence must be able to represent finalized loop-visible message artifacts from agent execution. Message semantics are defined by [002 Agent Execution](../002-agent-execution/spec.md).

Durable evidence must be able to represent AI generation terminal outcomes. Generation semantics and metadata boundaries are defined by [003 AI Protocol](../003-ai-protocol/spec.md).

Durable evidence must be able to connect assistant-requested tool calls to tool execution outcomes and tool-result artifacts. It must preserve the relationship between the model request, the execution outcome, the raw tool-result material, and the loop-visible tool-result message. Evidence may preserve a tool outcome summary such as success, failure, stopped, or aborted, but capability-specific tool result fields remain owned by their capability or tool specs. [007 Tool Surface](../007-tool-surface/spec.md) defines tool surface semantics. This spec does not define concrete tool names, tool declarations, tool bindings, tool schemas, tool permissions, tool result formats, or handler identities.

Durable evidence must be able to represent resource decisions that affect model visibility or tool execution. [009 Resource Surface](../009-resource-surface/spec.md) defines resource decision semantics. This spec does not define resource policy records, permission schemas, approval records, or enforcement records.

Memory candidates and memory mutations that arise from an agent invocation should be linkable to durable evidence. [010 Memory System](../010-memory-system/spec.md) defines memory boundaries. This spec does not define memory storage, retention behavior, retrieval behavior, or provider state.

Durable evidence must be able to represent the terminal agent-invocation outcome. Outcome semantics are defined by [002 Agent Execution](../002-agent-execution/spec.md). When a terminal outcome is projected from lower-level execution, provider, runtime, or interface facts, durable evidence must be able to preserve enough relationship information to inspect that projection without freezing a record schema.

Durable evidence must be able to preserve optional metadata when an implementation attaches it. Provider, continuity, diagnostic, or future-compatibility metadata may be preserved, but metadata must not redefine core execution, AI protocol, runtime, or evidence semantics. This spec does not define metadata schemas, metadata keys, serialization, or replay rules.

## Event Stream Relationship

Agent execution emits lifecycle events for observation. Durable evidence is not the same as the event stream.

Agent invocation, turn, message, tool execution, and generation lifecycle events may contribute to durable evidence, but this spec requires durable final facts rather than a durable copy of every start, update, and end event.

Streaming message updates, AI stream progress, tool execution progress, and UI-facing updates may be captured by an implementation. This spec does not require them for the durable evidence contract.

This spec does not require dedicated activity, fact, or reference tables. [030 State and Data Model](../030-state-and-data-model/spec.md) and [031 Storage and Persistence](../031-storage-and-persistence/spec.md) define first-slice state and storage boundaries.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and the principle that execution leaves evidence.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and dependency direction.
- [002 Agent Execution](../002-agent-execution/spec.md) defines execution concepts, event families, message semantics, and outcome semantics.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral generation semantics and metadata boundaries.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines agent-invocation assembly and evidence sink wiring.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly facts related to durable evidence.
- [007 Tool Surface](../007-tool-surface/spec.md) defines tool surface facts related to durable evidence.
- [008 Session Continuity](../008-session-continuity/spec.md) defines evidence-backed session continuity.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource decisions related to durable evidence.
- [010 Memory System](../010-memory-system/spec.md) defines memory candidates and mutations that may link to durable evidence.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines semantic state relationships that durable evidence may represent.
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines the persistence substrate boundary for durable evidence facts.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines extension facts that may affect runtime assembly and evidence relationships.
