---
name: 290. History Editing and Thread Fork
psychevo_self_edit: deny
---

# History Editing and Thread Fork

Define conversation history editing and user-owned Thread fork behavior across
Gateway, Workbench, and TUI.

## Scope

- editable user-input recovery for persisted Native messages
- same-Thread conversation revert and restore
- Native full-session fork and fork before a selected user message
- explicit fork provenance
- Workbench and TUI edit/fork interactions
- compatibility with existing workspace `/undo` and `/redo`

Out of scope:

- restoring, copying, or materializing a historical workspace for Edit or Fork
- branch merge, branch trees, or fork comparison
- point fork or conversation revert over generic ACP
- editing subagent, side, channel, automation, ephemeral, or non-root Threads
- Resource and ResourceLink restoration into an editable draft
- persistence of an unsent point-fork draft after the child is opened

## Vocabulary

An editable draft is the user-authored Text and Image input recovered from one
persisted user message. Synthetic Context, Resource, and ResourceLink input are
not editable draft parts.

A conversation revert stages a visible-history boundary at one persisted user
message. It hides that message and the later suffix without changing workspace
files. Restoring the conversation clears the boundary and returns the staged
replacement draft to the active composer.

A workspace undo is the existing `/undo` state that stages a history boundary
and restores a tracked Git snapshot. Conversation revert and workspace undo are
distinct state kinds and are mutually exclusive.

A point fork creates a new root Thread whose copied conversation ends before a
selected user message. A full fork copies all effective source history.

## Eligibility

History Edit and Native point fork are available only when all of the following
are true:

- the caller is Workbench or TUI;
- the Thread has a durable root session and a resolved Native binding;
- the session is not a subagent, side, channel, or automation session;
- no turn is active or queued;
- no workspace undo or conversation revert is already staged;
- the selected boundary is a visible, finalized, persisted user message.

Unsupported surfaces receive a bounded unavailable reason. Standard ACP
`session/fork` continues to advertise full fork only when the Agent negotiated
it; that capability never implies point fork or same-Thread revert.

## Editable Input

New Native user messages persist a versioned editable-input envelope in message
metadata. The envelope preserves the order of user Text and Image parts. Text
is stored directly. Image parts reference the matching durable user-message
image block rather than duplicating its bytes or data URL.

Gateway Context, Resource, ResourceLink, and runtime-generated contextual input
must not enter the envelope. A message whose envelope contains no editable Text
or Image has no Edit or point-fork action.

Messages created before the envelope exists are reconstructed from the durable
`Message::User` Text and Image blocks with `bestEffort` fidelity. That draft may
contain context previously flattened into user text. Workbench and TUI must
show a persistent, non-blocking warning while such a draft is edited.

An editable draft is non-empty when it contains an Image or non-whitespace Text.
Exact structural equality includes part order, Text bytes, Image kind, and Image
source. `Update & run` with an unchanged draft is a no-op.

## Conversation Edit

Selecting `Update & run` stages a conversation revert at the selected message
and persists the edited draft before starting the replacement turn. The selected
message is part of the hidden suffix.

Conversation edit never reads or restores a workspace snapshot. If `turn/start`
is rejected before admission, the staged boundary and replacement draft remain
available after refresh or restart. The caller can retry the turn or choose
`Restore history`.

An admitted Native turn uses the existing cleanup-before-run boundary to delete
the hidden suffix before appending the replacement message. A later provider
failure does not resurrect the deleted suffix. The replacement user message
keeps the same editable-input contract as an ordinary newly sent message. Once
the replacement turn settles and the Thread is eligible again, the user can
select that message and perform another `Update & run`; successful history
editing is not a one-shot capability.

`Restore history` clears a conversation-revert boundary without touching the
workspace and returns the edited draft to the composer. `/redo` remains the
recovery operation for workspace undo and does not consume conversation-edit
state.

## Native Fork

Native full and point fork are one local transaction. The transaction must:

1. revalidate source eligibility and resolve an optional `message:<session_seq>`
   boundary;
2. create a new root session in the same cwd with the same source, model,
   provider, and title;
3. copy messages before the boundary, including accounting and message metadata;
4. copy context evidence, referenced prompt-prefix records, valid compactions,
   and terminal evidence whose referenced messages are inside the copied prefix;
5. create a fresh resolved Native binding with the source profile, Agent, and
   Thread preferences but no native session handle or parent Thread;
6. recompute child message and tool-call counts;
7. persist only `forkedFromThreadId` as child session lineage metadata.

The transaction must not copy source bindings, activities, live state, turn
deliveries, outbox rows, revert state, agent edges, side/subagent lineage,
automation ownership, or ACP-native handles. Child-owned identifiers and
references are remapped while relative message order is preserved.

Point fork excludes the selected message. Forking before the first user message
creates an empty child. Source messages, metadata, revert state, and workspace
are unchanged by either fork form. A failed local fork leaves no child session.

The child copies the source title exactly. `forkedFromThreadId` is identity, not
display text and not `parent_session_id`; the child remains visible in root
history. Deleting the source does not delete the scalar provenance value.

## Thread Interface

The Framework Thread Application Interface is the single semantic owner of
Native history-editing eligibility, durable state, and mutations. Gateway
authorizes Workbench and TUI callers, guards Gateway-local Shell activity, and
projects typed Framework results onto the shared wire contract. A surface must
not read or mutate `StateRuntime` revert, message-count, binding, lineage, or
activity records to reconstruct those decisions. `Thread::history_editing_state`
and `editable_draft` return typed eligibility, staged state, draft parts,
fidelity, and bounded unavailable reasons. `stage_conversation_edit` and
`restore_conversation_edit` return typed idempotence, conflict, unavailable,
staged, unchanged, and restored outcomes without exposing persistence records,
metadata keys, wire strings, or a workspace snapshot hash.

Conversation-edit and Native-fork mutations first record an idle-only
reservation in the same `ApplicationRuntime` mutex that registers a Turn. A
pending-admission, active, or queued Turn, or another idle-only history
mutation, makes that reservation fail with structured `thread_busy` evidence
before it queues or touches persistence; if the reservation wins first, later
Turn registration fails before durable acceptance. The reservation remains
held for the complete Store future. This two-way exclusion is not implemented
as an activity snapshot followed by an ordinary FIFO mutation, and ordinary
Thread mutations retain their existing FIFO semantics. Reservation rejection
is returned as a structured Framework Error with `kind = thread_busy`; it is
not a Store mutation outcome. An active idle-only reservation is likewise
projected as busy by `Thread::history_editing_state`. Under the reservation,
the durable eligibility and boundary lookup plus either the fork copy or the
current staged-state check, draft revalidation, and revert write or restore
occur in one SQL write transaction. The edit write compares the session
metadata read by that transaction, so it cannot overwrite a concurrent
workspace undo, conversation edit, or unrelated metadata update.

Gateway conversation-edit and restore calls additionally reserve the Thread's
primary Shell queue key in the same mutex used by Shell admission. A running or
queued Shell rejects the history mutation before the Framework call; a history
reservation that wins first makes `send_shell` reject before queueing or durable
activity creation. The Gateway guard spans the complete Framework mutation
future and removes its flag and otherwise-empty active state on drop. Neither
direction is implemented as an activity snapshot followed by a mutation.
When a source-keyed Shell first reveals its durable Thread id, Gateway publishes
the `thread:*` alias in that same queue-state critical section before exposing
the identifying event to local consumers. Alias resolution and history-mutation
admission are one operation under that owner: a consumer cannot observe the id
and reserve a second `thread:*` lane while the original `source:*` Shell is
active.

`thread/history/draft/read` accepts `scope`, `threadId`, and `messageId`. It
returns the resolved `messageSeq`, ordered editable draft, fidelity, and an
optional warning or unavailable reason.

Thread actions distinguish:

- `fork` for full fork;
- `forkBefore { messageId }` for point fork;
- `revertConversation { messageId, draft }` for same-Thread staging;
- `unrevertConversation` for restoring the source history.

`ThreadSnapshot.historyEditing` reports the staged state kind, boundary message,
hidden-message count, replacement draft for conversation edit, and valid
recovery action. Snapshot projection never exposes a workspace snapshot hash.

`GatewayThread` and session-list projections expose optional
`forkedFromThreadId`, distinct from subagent parent identity.

## Surface Behavior

Workbench exposes one Edit icon in the existing user-message hover/focus action
row. Activating it replaces the bubble with an inline editor using existing
transcript and composer tokens. The editor offers `Cancel`, `Update & run`, and
`Fork`. Point fork waits for the authoritative child snapshot, opens the child,
and preloads the edited draft into the current client composer without sending
or persisting it.

Workbench renders staged conversation state as one compact strip above the
composer with the hidden-message count and `Restore history`. Fork provenance is
secondary text in the child header/history row; an unavailable source is shown
as an identifier without an active link.

TUI mouse click on an eligible persisted user row opens a Message Actions panel.
The keyboard equivalent is `Ctrl+T`, select the user row, then `Enter`. Edit and
Fork use one image-capable bottom editor. The sessions action mode uses `F` for
full fork. Existing `/fork` remains a child-agent command.

## Failure And Concurrency Rules

- running, queued, or stale-boundary operations fail before mutation;
- a staged revert disables additional Edit and Fork operations;
- a draft-read race is revalidated by the mutating action;
- a failed point fork leaves the source editor and draft intact;
- a successful fork navigates only after the authoritative child snapshot is
  available;
- source deletion after fork degrades provenance navigation without changing
  child history.

## Acceptance Criteria

- exact drafts preserve ordered Text/Image and omit synthetic Context;
- legacy drafts are marked `bestEffort` and display the warning;
- conversation edit never changes workspace files;
- workspace `/undo` and `/redo` keep their existing snapshot behavior;
- failed turn admission preserves staged conversation state;
- full and point forks preserve the required durable evidence and never mutate
  source history or workspace;
- ordinary Native fork children remain visible in root history with explicit
  scalar provenance;
- generic ACP exposes no point-fork or revert capability;
- Workbench and TUI exercise the same Gateway interface and recovery states.

## Related Topics

- [008 Session Continuity](../008-session-continuity/spec.md) defines durable
  session continuity.
- [030 Thread Lineage](../030-state-and-data-model/thread-lineage.md) defines
  shared Thread relationship vocabulary.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines the terminal surface.
- [230 pevo ACP](../230-pevo-acp/spec.md) defines generic ACP capability limits.
- [240 pevo Web](../240-pevo-web/spec.md) defines Workbench behavior.
- [Testing](testing.md) defines deterministic coverage.
