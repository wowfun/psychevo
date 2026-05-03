---
name: 008. Session Continuity
psychevo_self_edit: deny
---

Define session as the continuity and persistence boundary owned at the runtime boundary.

## Scope

- session identity at the semantic level
- session as the continuity and persistence boundary
- required session boundary for started agent invocations
- persistent and ephemeral session semantics
- evidence-backed continuity inputs for agent-invocation assembly
- relationship between session continuity and model context assembly
- minimal lineage through parent session relationships

Out of scope:
- JSONL, SQLite, schemas, tables, files, serialization, migrations, indexes, search behavior, or storage layout
- transcript formats, session file formats, branch UI, fork behavior, tree navigation, merge behavior, retry behavior, undo behavior, or resume commands
- replay algorithms or deterministic replay guarantees
- compaction algorithms, summarization algorithms, summary schemas, token thresholds, or prompt templates
- memory retention behavior, skills, evaluation, self-evolution, or workflow search
- filesystem checkpoints, process checkpoints, rollback behavior, sandboxing, approval behavior, or resource permission rules
- CLI rendering, terminal behavior, SDK APIs, Rust APIs, payload schemas, or event names

## Session Boundary

A session is the continuity and persistence boundary for Psychevo work. It gives later agent invocations a way to continue from earlier evidence-backed facts without making an agent invocation itself the durable continuity root.

A session identity identifies one continuity boundary. This spec does not define identity format, lookup rules, storage location, naming, or discovery.

Every started agent invocation has a session boundary. The session may be persistent or ephemeral/in-memory.

An ephemeral session may exist only for one accepted agent invocation or process lifetime. Its durable behavior remains governed by [004 Runtime Contract](../004-runtime-contract/spec.md), [005 Durable Evidence](../005-durable-evidence/spec.md), and [040 Storage and Persistence](../040-storage-and-persistence/spec.md).

Persistent sessions do not automatically end at `agent_end`. A persistent session ends at explicit lifecycle boundaries such as close, reset, switch to another session, resume of a different session, branch, expiry, or compression when an implementation defines those actions.

Resume of the same persistent session reopens that session, clears or updates ended state as needed, and continues appending to the same linear message history for the first implementation slice.

[010 Memory System](../010-memory-system/spec.md) defines optional reusable knowledge across sessions or future agent invocations. Memory is separate from session continuity.

This spec does not define stable session lifecycle events. Starting, ending, switching, resuming, or deleting sessions belongs outside this spec unless a later spec promotes those actions into stable semantics.

Any future lifecycle API must distinguish session lifecycle from agent-invocation lifecycle. `agent_end` is not a session close, reset, expiry, or lifecycle-end event.

## Evidence-Backed Continuity

Session continuity must be backed by durable evidence. A session may select, retain, or organize durable agent-invocation facts, but it must not create a second source of execution truth.

Durable evidence remains the source for finalized messages, generation outcomes, tool outcomes, terminal agent-invocation outcomes, and causal relationships. Continuity inputs offered to a later agent invocation must come from those durable facts or retained session messages.

Session persistence boundaries are defined by [040 Storage and Persistence](../040-storage-and-persistence/spec.md). This spec defines continuity semantics, not storage behavior.

If continuity metadata is attached, it must not redefine execution, AI protocol, runtime, context assembly, or durable evidence semantics. Metadata shape, keys, serialization, persistence, and replay behavior belong outside this spec.

## Continuity Inputs

Runtime may receive continuity inputs from a session during agent-invocation assembly. A continuity input is evidence-backed material supplied from session continuity into runtime assembly. Continuity inputs are candidates for context projection and other runtime wiring; they are not automatically model-visible.

Continuity inputs may include:
- prior finalized loop-visible messages
- summary context
- attached source material
- previous agent-invocation metadata
- continuity metadata

Prior finalized loop-visible messages use the message semantics defined by [002 Agent Execution](../002-agent-execution/spec.md). Summary context and attached source material use the model context categories defined by [006 Context Assembly](../006-context-assembly/spec.md).

[006 Context Assembly](../006-context-assembly/spec.md) decides which continuity inputs become model context for a generation request. This spec does not define prompt wording, context ordering, source discovery, summary format, or visibility policy.

## Lineage

A lineage relationship identifies how one persistent session relates to another persistent session. The first implementation slice may keep only a parent session relationship.

Lineage is semantic. It must not imply a storage cursor, file entry, database row, tree node, or message identifier.

Lineage may help runtime choose continuity inputs, but it does not define replay behavior, deterministic reconstruction, branch UI, fork behavior, merge behavior, tree navigation, retry, or undo.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and dependency direction.
- [002 Agent Execution](../002-agent-execution/spec.md) defines agent invocation, turn, message, and outcome semantics.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime assembly that may receive session continuity inputs.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines the durable evidence substrate for session continuity.
- [006 Context Assembly](../006-context-assembly/spec.md) defines how continuity inputs may become model context.
- [010 Memory System](../010-memory-system/spec.md) defines optional cross-session memory, separate from session continuity.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines how session continuity facts relate to other state families.
- [040 Storage and Persistence](../040-storage-and-persistence/spec.md) defines optional persistence boundaries for session continuity facts.
