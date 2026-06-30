---
name: 010. Memory System
psychevo_self_edit: deny
---

Define the optional memory system boundary for reusable knowledge across sessions or future agent invocations.

## Scope

- optional memory system boundary
- memory candidates
- retained memory
- memory recall as context candidates
- memory mutation boundary
- projection boundary between memory recall and model context
- evidence relationship for memory candidates and mutations that arise from an agent invocation

Out of scope:
- storage formats, files, databases, migrations, indexes, vector stores, full-text search, or provider state
- retrieval algorithms, ranking, embedding, summarization, compaction, deduplication, consolidation, expiration, or conflict resolution
- prompt templates, memory block formatting, token budgets, context ordering, or model-visible wording
- concrete memory tools, tool schemas, provider plugin APIs, provider selection, setup UX, authentication, billing, or network transport
- privacy policy, retention policy, redaction policy, injection scanning, secret handling, or trust scoring
- session replay, transcript search, branch or fork behavior, workflow search, skills, evaluation, or self-evolution
- CLI rendering, terminal behavior, SDK APIs, Rust APIs, payload schemas, or event names

## Memory System

A memory system is an optional runtime-adjacent mechanism for retaining reusable knowledge across sessions or future agent invocations.

Execution, durable evidence, and session continuity must work without memory. Memory may improve later agent invocations, but it must not become required execution substrate.

Memory is for stable reusable knowledge. It is not task progress, transcript replay, session continuation, or workflow state.

Memory may come from user intent, execution observations, durable evidence, or external memory providers. Regardless of source, memory must not become a second source of execution truth.

Capability extensions may declare memory candidates or memory providers. [050 Capability Extensions](../050-capability-extensions/spec.md) defines source, declaration, activation, availability, and conflict boundaries for those candidates. This spec owns memory semantics after memory candidates or providers reach the memory boundary.

## Memory Candidates

A memory candidate is information that may be retained or recalled as memory.

Memory candidates may include user preferences, durable corrections, stable environment facts, project conventions, or reusable operational knowledge.

Memory candidates should remain compact enough to be useful across future agent invocations. This spec does not define selection criteria, quality scoring, freshness, priority, or trust policy.

Memory candidates that arise from an agent invocation should be evidence-linked. Evidence links preserve where the candidate came from; they do not make memory part of the execution record.

## Retention and Recall

Retained memory is memory material preserved for possible later recall.

Memory recall selects retained memory as context candidates. Recall does not make memory model-visible by itself.

Memory persistence boundaries are defined by [031 Storage and Persistence](../031-storage-and-persistence/spec.md). This spec does not define storage, retrieval, ranking, provider routing, background refresh, synchronization, or capacity management.

## Projection Boundary

Memory recall produces context candidates for [006 Context Assembly](../006-context-assembly/spec.md).

Context assembly owns model visibility, ordering, transformation, and prompt projection. A recalled memory item may be omitted, transformed, summarized, or attached according to context projection rules.

Memory must not bypass context assembly by defining prompt sections, provider roles, message kinds, or model-visible wording.

## Mutation Boundary

A memory mutation creates, replaces, removes, or corrects retained memory.

Memory mutation is a boundary, not a tool contract. A later spec may define concrete memory tools, schemas, approval rules, or provider behavior.

Memory mutations that arise during an agent invocation should be observable. This spec does not define mutation events, records, storage commits, conflict handling, undo, approval behavior, or user-facing output.

## Evidence Relationship

Memory candidates and memory mutations that arise from an agent invocation should be linkable to durable evidence.

Durable evidence remains the source for execution facts. Memory may reference evidence, derive from evidence, or influence later context candidates, but memory does not replace durable evidence.

[005 Durable Evidence](../005-durable-evidence/spec.md) owns durable evidence semantics. This spec does not define evidence records, storage, replay, or retention.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream execution-substrate principle.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines agent-invocation assembly without requiring memory.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence that memory candidates may link to.
- [006 Context Assembly](../006-context-assembly/spec.md) defines how memory recall candidates may become model context.
- [008 Session Continuity](../008-session-continuity/spec.md) defines session continuity, which stays separate from cross-session memory.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines how memory facts relate to other state families.
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines optional persistence boundaries for retained memory facts.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  how extension declarations may provide memory candidates or providers.
