---
name: 040. Storage and Persistence
psychevo_self_edit: deny
---

Define Psychevo's storage and persistence boundary for durable semantic facts.

## Scope

- persistence boundary for durable semantic facts
- persistence substrate for durable evidence
- optional persistence boundary for session continuity
- optional persistence boundary for memory
- optional persistence boundary for capability extension facts that affect agent-invocation assembly
- optional persistence boundary for gateway source-to-thread bindings
- persistence attempt and outcome observability
- retrieval by semantic relationship
- representation evolution boundary
- first-slice SQLite persistence attachment

Out of scope:
- JSONL, SQLite, tables, files, indexes, storage engines, migrations, FTS, search, query languages, pagination, or sorting except where an attachment explicitly defines an implementation slice
- Rust APIs, SDK APIs, CLI behavior, HTTP or JSON payloads, event payloads, schemas, fields, ID formats, cursors, paths, or handles
- replay algorithms, deterministic replay guarantees, trace formats, transcript formats, session-selection behavior, branch UI, or resume commands
- memory retrieval, ranking, vector stores, provider state, authentication storage, secret storage, configuration storage, or provider credential storage
- retention policy, deletion policy, garbage collection, tombstones, redaction, privacy policy, security policy, or data governance

## Storage Boundary

A persistence substrate is a runtime-wired durable substrate for persisted semantic facts.

A persisted fact is a durable representation of a semantic fact owned by another spec. Storage is not a semantic truth source. Persisted representations must preserve the owning spec's semantics instead of redefining them.

Durable evidence persistence is the baseline persistence requirement. Session continuity and memory may use persistence, but they are optional consumers and must not become required execution substrate.

Runtime wires persistence boundaries into session coordination, agent-invocation assembly, and evidence sinks. This spec does not create a new crate boundary, storage service boundary, or ownership layer for semantic state.

## Persistable Facts

Persistable facts are durable or evidence-linked facts identified by the owning semantic specs.

Durable evidence facts from [005 Durable Evidence](../005-durable-evidence/spec.md) are the baseline persistable facts. They include final session and agent-invocation facts and causal relationships needed for inspection and future replay work.

Session continuity facts from [008 Session Continuity](../008-session-continuity/spec.md) may be persisted when a persistent session is active. Session persistence must remain backed by durable evidence and must not create a second source of execution truth.

Memory facts from [010 Memory System](../010-memory-system/spec.md) may be persisted when optional memory is enabled. Memory persistence must not replace durable evidence or session continuity as execution truth.

Capability extension facts from [050 Capability Extensions](../050-capability-extensions/spec.md) may be persisted when they affect agent-invocation assembly or evidence inspection. Persistence must not turn storage into the source of extension semantics.

Gateway source-to-thread bindings from [021 Gateway](../021-gateway/spec.md) may be persisted when a caller-facing source uses `Persistent` lifetime and needs continuity across process restarts or transport reconnects. Persistence stores routing and lineage facts only; invocation-scoped and process-scoped source bindings are not persisted, and runtime sessions and durable evidence remain the execution truth.

State relationships from [030 State and Data Model](../030-state-and-data-model/spec.md) describe how persisted facts remain relatable. This spec does not define identifiers, fields, tables, storage cursors, or object graphs outside explicit attachments.

## Persistence Outcomes

A persistence attempt is an attempt to make a semantic fact durable through the persistence substrate.

A persistence outcome is the observable success or failure of a persistence attempt. Persistence failures must be observable to the runtime or caller-facing layers that depend on the persisted fact.

This spec does not require runtime to fail closed, retry, block, abort an agent invocation, mark an agent invocation as failed, or use ACID transactions when persistence fails. Outcome presentation and execution impact belong to runtime and interface behavior outside this spec unless another spec defines a stricter rule.

The baseline is final-fact persistence. Implementations may persist intermediate updates, streaming progress, or implementation records, but this spec does not require event-by-event persistence.

Per-message metadata may carry durable metric facts for the message they
annotate. For tool-result messages, implementations may persist tool execution
duration such as `elapsed_ms` in the message metadata so interfaces can restore
completed tool timing without replaying live execution events. Protocol bridges
may also preserve runtime tool timing in protocol extension metadata, such as
ACP `_meta.psychevo.toolTiming`, so downstream report projections can recover
actual tool execution duration without treating an entire agent step as tool
execution time.

## Retrieval Boundary

Persisted facts and retained material must be retrievable by the semantic relationships required by their consumers.

Evidence inspection may need to relate a session to final messages, agent-invocation final facts, generation outcomes, tool execution outcomes, resource decisions, metadata, and terminal outcomes.

Session continuity may need to recover continuity inputs from evidence-backed facts. Memory recall may need to recover retained memory and evidence-linked origins. Interface completion may need a way to reach final result material, tool result material, artifact material, or other evidence-backed result material through retained session, evidence, message, and material relationships.

This spec does not define public boundary identifiers, raw record access, query languages, search behavior, index behavior, ordering, pagination, projection fields, or transport shape.

## Representation Evolution

Persistent representations may evolve as implementations change.

Evolution failures should be observable when they prevent persisted facts or retained material from being used by their semantic consumers.

This spec does not define version fields, migration algorithms, compatibility matrices, backfill behavior, schema reconciliation, storage-engine strategy, or cleanup behavior outside explicit attachments.

## Attachments

- [SQLite Persistence](sqlite-persistence.md) defines the default first implementation slice contract for SQLite-backed session and message persistence.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream principle that execution leaves evidence.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines session coordination, agent-invocation assembly, and evidence sink wiring.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence facts that form the baseline persistence requirement.
- [008 Session Continuity](../008-session-continuity/spec.md) defines optional evidence-backed session continuity.
- [010 Memory System](../010-memory-system/spec.md) defines optional memory facts that may use persistence.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing result access through session and evidence relationships.
- [021 Gateway](../021-gateway/spec.md) defines gateway source mapping persistence needs.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines semantic state relationships and recoverability classes.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines capability extension facts that may affect runtime assembly and evidence inspection.
