---
name: 005. Durable Evidence
psychevo_self_edit: deny
---

Define the durable evidence contract for Psychevo runs.

## Scope

- durable run evidence semantics
- run assembly facts at the semantic level
- finalized loop-visible message artifacts
- AI generation terminal outcomes
- assistant tool request, tool execution outcome, and tool-result linkage
- terminal run outcome
- causal relationships between run, turn, message, generation, and tool facts
- optional metadata preservation boundary

Out of scope:
- JSONL, SQLite, schemas, tables, files, serialization, migrations, indexes, or search behavior
- session storage, branch, fork, compaction, retry, undo, resume, or transcript formats
- replay algorithms or deterministic replay guarantees
- event stream transport, UI rendering, CLI behavior, or process behavior
- concrete tool declarations, tool bindings, tool schemas, permission schemas, provider wire formats, or provider payload fields
- memory storage, memory retention behavior, memory provider state, or memory retrieval behavior
- secret handling, redaction, retention, privacy policy, or data governance policy

## Evidence Model

Durable evidence is final run facts connected to an evidence sink by runtime.

Durable evidence records final facts. Streaming progress events may be observed, but this spec does not require every update to become durable evidence.

Durable evidence preserves causal relationships. A consumer should be able to relate run facts to turns, messages, AI generations, tool requests, tool executions, and terminal outcomes without requiring one global append-only log.

Durable evidence prepares for replay. Future replay work should be able to use durable evidence as an input, but this spec does not define replay behavior or deterministic replay guarantees.

Evidence sink failures must be observable. This spec does not require runtime to fail closed, retry, block, or continue in a specific way when evidence wiring fails.

Durable evidence is the source substrate for session continuity. [008 Session Continuity](../008-session-continuity/spec.md) defines how sessions may select, reference, or organize evidence-backed facts across runs.

## Required Evidence

Durable evidence must be able to represent run assembly facts at the semantic level. Those facts may include the model target, generation controls, context assembly facts, resource surface facts, tool surface selection facts, cancellation wiring facts, and evidence sink wiring facts. [006 Context Assembly](../006-context-assembly/spec.md) defines context assembly semantics. This spec does not require full prompt snapshots, context schemas, policy schemas, or concrete configuration fields.

Durable evidence must be able to represent finalized loop-visible message artifacts from agent execution. Message semantics are defined by [002 Agent Execution](../002-agent-execution/spec.md).

Durable evidence must be able to represent AI generation terminal outcomes. Generation semantics and metadata boundaries are defined by [003 AI Protocol](../003-ai-protocol/spec.md).

Durable evidence must be able to connect assistant-requested tool calls to tool execution outcomes and tool-result artifacts. It must preserve the relationship between the model request, the execution outcome, and the loop-visible tool-result message. [007 Tool Surface](../007-tool-surface/spec.md) defines tool surface semantics. This spec does not define concrete tool names, tool declarations, tool bindings, tool schemas, tool permissions, tool result formats, or handler identities.

Durable evidence must be able to represent resource decisions that affect model visibility or tool execution. [009 Resource Surface](../009-resource-surface/spec.md) defines resource decision semantics. This spec does not define resource policy records, permission schemas, approval records, or enforcement records.

Memory candidates and memory mutations that arise from a run should be linkable to durable evidence. [010 Memory System](../010-memory-system/spec.md) defines memory boundaries. This spec does not define memory storage, retention behavior, retrieval behavior, or provider state.

Durable evidence must be able to represent the terminal run outcome. Outcome semantics are defined by [002 Agent Execution](../002-agent-execution/spec.md).

Durable evidence must be able to preserve optional metadata when an implementation attaches it. Provider, continuity, diagnostic, or future-compatibility metadata may be preserved, but metadata must not redefine core execution, AI protocol, runtime, or evidence semantics. This spec does not define metadata schemas, metadata keys, serialization, or replay rules.

## Event Stream Relationship

Agent execution emits lifecycle events for observation. Durable evidence is not the same as the event stream.

Run, turn, message, tool execution, and generation lifecycle events may contribute to durable evidence, but this spec requires durable final facts rather than a durable copy of every start, update, and end event.

Streaming message updates, AI stream progress, tool execution progress, and UI-facing updates may be captured by an implementation. This spec does not require them for the durable evidence contract.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and the principle that execution leaves evidence.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and dependency direction.
- [002 Agent Execution](../002-agent-execution/spec.md) defines execution concepts, event families, message semantics, and outcome semantics.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral generation semantics and metadata boundaries.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime run assembly and evidence sink wiring.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly facts related to durable evidence.
- [007 Tool Surface](../007-tool-surface/spec.md) defines tool surface facts related to durable evidence.
- [008 Session Continuity](../008-session-continuity/spec.md) defines evidence-backed continuity across runs.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource decisions related to durable evidence.
- [010 Memory System](../010-memory-system/spec.md) defines memory candidates and mutations that may link to durable evidence.
