---
name: 030. Transcript State Attachment
psychevo_self_edit: deny
---

# 030. Transcript State Attachment

Define transcript state ownership for Psychevo's session-centered data model.
This attachment is part of [030 State and Data Model](spec.md). It defines
truth-source and recoverability semantics only; product projection contracts,
wire payloads, JSON schema, terminal rows, and UI layout belong to product and
interface specs.

## Scope

- ordinary transcript fact ownership
- durable, reconstructable, transient, debug, and display-only recoverability
  classes for transcript-related material
- semantic relationships between messages, reasoning, assistant text, tool
  requests, tool results, usage/provider metadata, and selected skill metadata
- boundaries that prevent duplicate durable transcript sources

Out of scope:

- transport or protocol payload shapes
- Rust structs, TypeScript types, JSON schema, table names, columns, indexes,
  migrations, or storage engines
- TUI, Web, ACP, IM, or CLI rendering details
- deterministic replay, transcript search, branch/fork UI, or export formats
- domain artifact sidecars such as diffs, reports, or files

## Transcript Ownership

An ordinary transcript is the model-facing session history plus the semantic
material needed to inspect that history after a turn settles. It is not a
database table, timeline log, TUI ledger row, terminal-rendered string, viewport
cache, or product-specific display artifact.

Transcript ownership is explicit per thread. Native Psychevo threads use
runtime `messages` as their durable ordinary transcript fact source. An ACP
Agent that advertises load/resume owns its native transcript; Gateway retains a
bounded product projection/checkpoint but does not synthesize a replacement
Agent session by replaying copied text. An ACP Agent without load/resume has a
process-ephemeral native transcript and Gateway marks it non-resumable after
process loss.

Psychevo durably retains the public/native binding, captured Agent Definition
and Runtime Profile, accepted user intent, classified terminal outcome, content
hashes, context statistics, delivery state, and product-safe projection. These
facts never justify fabricating missing Agent-authoritative content.

Durable Native ordinary transcript material includes user message content,
assistant visible text, local folded reasoning blocks when retained by runtime,
assistant tool requests, tool-result messages or material, terminal assistant
outcomes, and metadata that belongs to those messages.

Derived Native transcript views are projections over messages. ACP transcript
views are projections over Agent replay plus Gateway's product checkpoint. A
derived view may group, sort, attach, summarize, or render content for a caller,
but it must not become an additional durable ordinary transcript source. The
owning content facts must be enough to rebuild ordinary history after reload,
session switch, export, or reconnect. Psychevo never combines local and native
content owners for one thread.

## Recoverability Classes

Durable facts are retained as message/session facts or metadata. User text,
assistant text, retained reasoning, tool-call identity, tool-call arguments,
tool-result content, tool-result status, terminal outcomes, provider/model
metadata, usage metrics, accounting metadata, and selected skill activation
metadata fall into this class when runtime records them.

Reconstructable views are rebuilt from durable messages and metadata. Ordinary
transcript entries, surface snapshots, visible tool rows, assistant preamble
views, attached tool-result views, selected skill notices, and history reload
rows are reconstructable. They may have stable derived identity, but that
identity does not make the view a second source of truth.

Transient observations exist only while a turn or process is active. Streaming
reasoning deltas, provisional assistant text, pending tool-call input,
tool-execution progress, live elapsed timers, active control handles, queued
turn state, and optimistic local prompt echoes are transient unless their final
effect is recorded by the thread's owning content source. Accepted prompt text
is scrubbed from bounded delivery ledgers as soon as delivery is confirmed;
only its hash and delivery state remain. Live snapshots are erased after
terminal completion and reconnect uses the declared history owner.

Raw or unclassified runtime/provider observations are transient diagnostics.
They are not ordinary transcript facts, are not request-reconstruction facts,
and must not become a generic durable debug store or ordinary transcript
projection.

Display-only command and UI state is local presentation material. Command
feedback, bottom panes, overlays, structured diff panels, copy/export dialogs,
completion popovers, and renderer toggles do not become durable messages,
model context, usage/accounting material, or ordinary transcript history.

## Semantic Relationships

Assistant tool requests and later tool results must remain relatable by their
tool-call identity. A tool-result message belongs to model-visible history, but
ordinary display projections may attach it to the assistant tool request that
caused it.

Reasoning and assistant text are message material, not tool evidence by
themselves. A reasoning or assistant text block that says the model intends to
use a tool does not create a durable tool execution fact. Tool evidence requires
a structured tool request, a tool execution observation, or a durable
tool-result relationship.

Usage, accounting, provider, model, finish reason, elapsed, and outcome
metadata describe message or turn facts. They are not assistant text blocks and
must not be serialized as sanitized transcript content unless a downstream
projection explicitly renders them as metadata.

Selected skill activation is prompt/message metadata. It may be rendered as a
quiet notice by product surfaces, but it is not a separate ordinary transcript
entry or a durable display row.

## Sidecar Boundary

Psychevo must not introduce a generic durable ordinary transcript sidecar. A
future durable artifact or display history must be justified by a concrete
domain spec, own its recoverability rules, and define how it relates back to
messages without replacing them as the ordinary transcript source.

A Channel delivery outbox for an Agent final payload is not transcript
ownership. It retains the payload only until the platform acknowledges delivery,
then erases it and keeps delivery status plus content hash.

## Related Topics

- [030 State and Data Model](spec.md)
- [030 Session Record Model](session-record-model.md)
- [250 UI Display Model](../250-ui-display-model/spec.md)
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md)
