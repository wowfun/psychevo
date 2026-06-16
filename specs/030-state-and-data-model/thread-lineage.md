---
name: 030. Thread Lineage Attachment
psychevo_self_edit: deny
---

# Thread Lineage

Define the shared state vocabulary for caller-facing threads and their
relationships to durable sessions.

This attachment is part of [030 State and Data Model](spec.md). It is not an
independently numbered spec and does not introduce a new public interface.

## Scope

- caller-facing thread and durable session vocabulary
- parent, child, forked, side, and subagent thread relationships
- model-visible inherited context boundaries for forked child threads
- visibility and recoverability classes for temporary side threads

Out of scope:

- concrete API payload fields, Rust or TypeScript type names, SQL schemas, and
  storage migrations
- child-agent execution semantics, which belong to
  [051 Subagents](../051-agents/subagents.md)
- surface-specific tab, row, keybinding, or layout behavior, which belongs to
  [213 Thread Navigation](../213-pevo-display-model/thread-navigation.md) and
  surface specs

## Thread And Session Vocabulary

A thread is the caller-facing conversation and routing identity exposed by
interactive interfaces and Gateway thread operations. A session is the durable
continuity and storage boundary. For local Psychevo execution, a thread is
backed by a session record when durable state exists, but thread navigation and
session storage are separate concerns.

Thread terminology should be used for user-facing conversation selection,
navigation, opening, focusing, and live event routing. Session terminology
should be used for durable records, message ownership, retained evidence, and
storage relationships.

Runtime, Gateway, and surface implementation names should use this vocabulary
for newly introduced caller-facing concepts. Specs should still avoid
mechanical renames of `session_id`, `parent_session_id`, or `child_session_id`
when those names refer to durable session records rather than thread
navigation.

A child thread view is a child thread opened inside another thread's
interaction context. Side conversations and subagent threads share this
navigation model, but they keep their domain names because they differ in
creation, lifetime, and ownership. The view describes parent-scoped inspection
and navigation, not a separate durable storage object. When a child thread view
has a backing session record, that session remains the durable body of work;
parent rows, tabs, badges, and buttons are projections over lineage metadata.

## Lineage

A root thread has no parent thread. A child thread is created from or owned by a
parent thread. A forked thread is a child thread that starts with a captured
parent context snapshot. A parent can have multiple child threads.

Lineage facts must preserve enough identity for a surface to return to the
parent, route live events to the correct visible thread, and explain child work
from the parent transcript. Lineage does not require parent transcript rows to be
copied into child transcript views.

When a thread has a durable backing session, the corresponding session lineage
must remain relatable to the thread lineage. Storage-specific edge tables may
record coordination state, but they do not replace the child thread's own
session as the durable body of work.

Lineage metadata is the shared fact source for child thread identity. Surfaces
must not infer a child thread from display labels alone. For subagent threads,
the durable parent-to-child agent edge and child session metadata provide the
open target. For side threads, the side-thread session metadata records the
parent and hidden inherited-context boundary. These facts may enrich display
rows, but display rows do not become lineage ownership.

## Side Threads

A side chat is a temporary side task opened from an existing parent
thread. Its implementation thread is a side thread. A side thread is an
ephemeral fork: it receives a parent context snapshot and hidden boundary
instructions, but it is not an ordinary history thread from the user's
perspective.

Inherited parent context in a side thread is model-visible reference material
only. It must be marked so ordinary transcript projection hides it. Messages
submitted after the side-chat boundary are the active user instructions
for the side thread.

Later parent output is not merged into the side thread. Later side-local control
changes do not mutate the parent thread's controls. Workspace changes explicitly
requested inside the side chat are real workspace changes; closing or
deleting the side thread does not revert them.

Side threads are hidden from ordinary history, list, resume, and default latest
thread selection. An implementation may use temporary durable records as an
implementation detail while the side chat is open, but ordinary product
surfaces must treat those records as ephemeral and clean them up when the side
chat is closed or stale.

## Subagent Threads

A subagent thread is a runtime-owned child agent thread created for a subagent
run. It is not a temporary side thread. The child thread is the durable agent
body when the run has durable local state.

[051 Subagents](../051-agents/subagents.md) owns subagent execution, lifecycle,
mailbox, control, and agent-edge semantics. This attachment only defines how the
subagent child thread relates to its parent thread and durable backing session.

Interactive surfaces may inspect a subagent thread while it is running or after
it completes. Opening that thread preserves the child identity and routes future
submissions and live events to the child, not to the parent Agent row.

Failed subagent spawn attempts that never created a child session have no
subagent thread to open. If a later retry succeeds, that retry creates a
separate subagent thread and must not be rebound to earlier failed rows merely
because the agent definition or task label looks similar.

## Transcript Visibility

Thread lineage does not create ordinary transcript entries. Opening, focusing,
closing, or returning from a child thread is navigation state. It must not create
model-visible messages, usage records, session-export content, or ordinary
transcript facts unless the user submits a prompt inside that thread.

Inherited side-thread context must not appear in ordinary transcript projection.
Subagent thread transcript projection uses the child thread's own messages and
live events. Parent Agent rows may summarize child status, but those summaries
do not replace the child thread's transcript.

## Related Topics

- [030 State and Data Model](spec.md) defines the semantic state model.
- [030 Session Record Model](session-record-model.md) defines first-slice durable
  session and message records.
- [030 Transcript State](transcript-state.md) defines ordinary transcript fact
  ownership.
- [051 Subagents](../051-agents/subagents.md) defines child-agent execution.
- [213 Thread Navigation](../213-pevo-display-model/thread-navigation.md) defines
  shared display and navigation behavior for side and subagent threads.
