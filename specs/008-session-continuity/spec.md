---
name: 008. Session Continuity
psychevo_self_edit: deny
---

Define the session continuity contract owned at the runtime boundary.

## Scope

- session continuity across runs
- session identity at the semantic level
- continuity points within a session
- evidence-backed continuity inputs for runtime run assembly
- relationship between session continuity and model context assembly
- minimal lineage through parent continuity points
- ephemeral runs that do not join session continuity

Out of scope:
- JSONL, SQLite, schemas, tables, files, serialization, migrations, indexes, search behavior, or storage layout
- transcript formats, session file formats, branch UI, fork behavior, tree navigation, merge behavior, retry behavior, undo behavior, or resume commands
- replay algorithms or deterministic replay guarantees
- compaction algorithms, summarization algorithms, summary schemas, token thresholds, or prompt templates
- memory retention behavior, skills, evaluation, self-evolution, or workflow search
- filesystem checkpoints, process checkpoints, rollback behavior, sandboxing, approval behavior, or resource permission rules
- CLI rendering, terminal behavior, SDK APIs, Rust APIs, payload schemas, or event names

## Session Continuity

A session is a continuity boundary across runs. It gives later runs a way to continue from earlier run facts without making the run itself larger than the execution unit defined by [002 Agent Execution](../002-agent-execution/spec.md).

A session identity identifies one continuity boundary. This spec does not define identity format, lookup rules, storage location, naming, or discovery.

Session continuity is optional. An ephemeral run does not join session continuity. Durable evidence for that run remains governed by [004 Runtime Contract](../004-runtime-contract/spec.md) and [005 Durable Evidence](../005-durable-evidence/spec.md).

Session continuity connects runs inside a continuity boundary. [010 Memory System](../010-memory-system/spec.md) defines optional reusable knowledge across runs or sessions.

This spec does not define session lifecycle events. Starting, ending, switching, resuming, or deleting sessions belongs outside this spec unless a later spec promotes those actions into stable semantics.

## Evidence-Backed Continuity

Session continuity must be backed by durable evidence. A session may select, reference, or organize durable run facts, but it must not create a second source of execution truth.

Durable evidence remains the source for finalized messages, generation outcomes, tool outcomes, terminal run outcomes, and causal relationships. Continuity inputs offered to a later run must come from those durable facts.

If continuity metadata is attached, it must not redefine execution, AI protocol, runtime, context assembly, or durable evidence semantics. Metadata shape, keys, serialization, persistence, and replay behavior belong outside this spec.

## Continuity Inputs

Runtime may receive continuity inputs from a session during run assembly. A continuity input is evidence-backed material supplied from session continuity into runtime run assembly. Continuity inputs are candidates for context projection and other runtime wiring; they are not automatically model-visible.

Continuity inputs may include:
- prior finalized loop-visible messages
- summary context
- attached references
- previous run metadata
- continuity metadata

Prior finalized loop-visible messages use the message semantics defined by [002 Agent Execution](../002-agent-execution/spec.md). Summary context and attached references use the model context categories defined by [006 Context Assembly](../006-context-assembly/spec.md).

[006 Context Assembly](../006-context-assembly/spec.md) decides which continuity inputs become model context for a generation request. This spec does not define prompt wording, context ordering, source discovery, summary format, or visibility policy.

## Lineage

A continuity point identifies where a later run may continue from within a session. A continuity point is semantic. It must not imply a storage cursor, file entry, database row, tree node, or message identifier.

Lineage is limited to parent continuity points. A later continuity point may identify a parent continuity point, but this spec does not define branch trees, fork behavior, merge behavior, tree navigation, retry, undo, or resume UI.

Lineage may help runtime choose continuity inputs, but it does not define replay behavior or deterministic reconstruction.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and dependency direction.
- [002 Agent Execution](../002-agent-execution/spec.md) defines run, turn, message, and outcome semantics.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime run assembly that may receive session continuity inputs.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines the durable evidence substrate for session continuity.
- [006 Context Assembly](../006-context-assembly/spec.md) defines how continuity inputs may become model context.
- [010 Memory System](../010-memory-system/spec.md) defines optional cross-session memory, separate from session continuity.
