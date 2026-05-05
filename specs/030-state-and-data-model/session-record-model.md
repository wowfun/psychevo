---
name: 030. Session Record Model Attachment
psychevo_self_edit: deny
---

Define the first implementation slice contract for session-centered records.

This attachment is part of [030 State and Data Model](spec.md). It is not an independently numbered spec and does not introduce a new public interface.

## Scope

- logical session records
- logical message records
- first-slice message field support
- first-slice linear message history
- minimal parent session lineage
- model and working-context summaries kept as implementation metadata

Out of scope:
- concrete Rust structs, SQL column names, JSON shapes, or public APIs
- event payload schemas, stream payloads, or transport payloads
- branch trees, fork UI, retry UI, undo UI, merge behavior, or transcript search
- activity tables, fact tables, artifact tables, or storage-level reference tables
- deterministic replay, full provenance graphs, or model-provider payload archival

## Record Model

The first implementation slice uses a linear session transcript with session records and message records.

The model does not introduce a persistent activity root. Agent invocation remains a live and semantic relationship assembled by runtime and represented through session, message, evidence, and outcome relationships.

Turn index is live-visible for observation when needed. It is not required as a field on message records.

## Session Record

A session record represents one continuity and persistence boundary.

Logical session record material includes:
- session identity
- source or entrypoint category when known
- working context summary when relevant
- model summary when relevant
- optional parent session identity
- `started_at`
- optional `ended_at`
- optional `end_reason`
- `message_count`
- `tool_call_count`
- optional title
- optional metadata

The working context summary and model summary are descriptive metadata. They must not become the source of truth for resource boundaries, model selection, or provider configuration.

`ended_at` and `end_reason` describe session lifecycle state. Reopening the same persistent session may clear or update those fields before appending more messages.

## Message Record

A message record represents retained loop-visible material in a session.

Core logical message record fields are:
- `session_id`
- `role`
- `content` or material
- `timestamp`

The first implementation slice must also be able to store these fields when they are present:
- `tool_call_id`
- `tool_calls`
- `tool_name`
- `token_count`
- `finish_reason` or outcome metadata
- local folded `reasoning` content blocks
- optional reasoning provider evidence that cannot be derived from the
  reasoning text
- model metadata
- provider metadata
- normalized usage metrics

These names define logical message field support for the first implementation slice. They do not require matching SQL column names, transport fields, Rust field names, or caller-facing APIs.

Message role and loop-visible semantics remain owned by [002 Agent Execution](../002-agent-execution/spec.md). Tool-call identity, tool calls, and tool name are relationship aids, not caller-facing API requirements.

Provider-specific replay payload may be preserved as optional metadata when it
cannot be derived from local folded reasoning blocks. Provider wire fields such
as `reasoning_content` must not become stored logical message fields, public
APIs, or provider-neutral message semantics.

Usage metrics and provider metadata are evidence facts associated with message
records. They remain outside retained transcript content: they are not
assistant text, not assistant content blocks, and not serialized into sanitized
transcript projections. Interfaces may join them with sanitized messages for
local summaries or debug views.

Large tool material may be embedded in message material or metadata for the first implementation slice. External artifact storage belongs to a later attachment or spec.

## Relationships

Session records relate to message records through session identity.

Assistant tool request material, tool execution outcome, and tool-result message or material must remain relatable through evidence and message relationships.

Final result material must be reachable through retained session, evidence, message, and material relationships. This attachment does not define public locators or storage handles for that retrieval.

## Non-Goals

The first implementation slice does not introduce:
- entry trees
- activity records
- separate fact records
- storage-level reference records
- artifact records
- branch or fork navigation structures
- durable turn records

## Related Topics

- [030 State and Data Model](spec.md) defines the semantic state model.
- [008 Session Continuity](../008-session-continuity/spec.md) defines session continuity semantics.
- [040 Storage and Persistence](../040-storage-and-persistence/spec.md) defines storage boundaries.
- [040 SQLite Persistence](../040-storage-and-persistence/sqlite-persistence.md) defines the default SQLite implementation contract.
