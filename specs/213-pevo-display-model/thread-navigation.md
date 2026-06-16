---
name: 213. pevo Display Model Thread Navigation
psychevo_self_edit: deny
---

# 213. pevo Display Model Thread Navigation

Define shared display and navigation behavior for threads that are inspected
from another active thread, including Side chat and subagent
child threads.

## Scope

- shared parent/child thread display vocabulary
- Side chat visibility, display, and cleanup expectations
- child-agent thread opening and live-event routing expectations
- display-only boundaries for parent rows, child tabs, and local navigation

Out of scope:

- child-agent execution semantics, which belong to
  [051 Subagents](../051-agents/subagents.md)
- concrete keybindings, row hit areas, tab layout, or product-specific chrome
- durable thread lineage, transcript fact ownership, storage schemas, and
  provider semantics, which belong to [030 Thread Lineage](../030-state-and-data-model/thread-lineage.md)

## Shared Model

A child thread view is a thread displayed from another thread's context. Child
thread views use ordinary thread identity and ordinary transcript projection;
they are not copied transcript fragments. A surface that opens a child thread
must render and submit against that child thread while keeping the parent thread
identity available for return, status, and notifications.

Subsession is the shared display term for parent-scoped child thread views.
Side chat and subagent inspection use the same navigation model: the visible
tab or split view points at a concrete child thread identity, while parent
transcript rows and right-column buttons are only affordances for opening that
identity. Display surfaces must consume the lineage/display metadata projected
from the shared state model instead of maintaining a second source of truth for
side chat or subagent identity.

Child thread navigation is display state. Opening, focusing, closing, or
returning from a child thread must not create ordinary transcript entries,
model-visible messages, usage records, or session-export content unless the user
submits actual prompt input inside that thread.

Child thread affordances are visible only when there is a concrete current
thread. Draft/no-thread states may still parse an explicitly typed command and
return bounded guidance, but discovery, completion, and GUI navigation should
hide Side chat creation controls until the parent thread exists.
Child thread tabs or split views are scoped to their parent thread. Switching
to a draft/no-thread state or another parent thread hides scoped child thread
views instead of rendering them under the wrong parent. Returning to the parent
may reveal retained child thread views that were not explicitly closed.

## Side Chat

`Side chat` is the user-facing name for temporary side-thread views across
interactive surfaces. `/btw [prompt]` creates a temporary side chat. The backing side thread
is an ephemeral fork defined by [030 Thread Lineage](../030-state-and-data-model/thread-lineage.md).
It starts from a snapshot of the parent conversation plus hidden boundary
instructions marking inherited history as reference-only. Later parent output is
not merged into the side context.

The side chat inherits the parent surface's current controls at creation
time, including model, reasoning effort, mode, permission mode, selected agent,
skills, and tool surface where the surface exposes those controls. Later
side-local control changes do not mutate the parent controls. Workspace changes
requested inside the side chat are real workspace changes; closing or
deleting the side thread does not revert them.

Surfaces may open side chats as an entered view, a split/tab, or another
native child-thread container. The side transcript and composer submit to the
side thread. The visible side transcript starts at the side-chat
boundary: inherited parent messages and hidden boundary instructions remain
model-visible reference context but must not render as ordinary transcript
entries. Closing a temporary side chat deletes only the temporary side
thread transcript and messages, clears any retained live-event backlog for that
side thread, and returns/focuses the parent. If side work is running,
interrupting the side work takes precedence over deleting the side thread.

When `/btw` includes an inline prompt, the surface must open the side thread
view and submit the prompt to the side thread, not the parent. The first side
prompt must be visible through the same optimistic/live reconciliation path as
ordinary thread submission, even if live events arrive before the side thread
snapshot has loaded.

If a surface retains live events for a side thread before the side view has an
initial snapshot, opening the side view replays that backlog through the same
thread-scoped reconciliation rules as normal live transcript events. Failed and
interrupted terminal status uses the shared turn lifecycle projection, so side
threads do not invent a separate error display source.

Side chats support a restricted command set. Nested side chats,
session navigation/new-session commands, undo/redo, agent-management commands,
compaction, refresh/reload-context, and dynamic skill invocation are rejected
with bounded feedback from inside a side chat.

## Child-Agent Threads

Runtime-owned subagents remain defined by [051 Subagents](../051-agents/subagents.md).
For display, the parent Agent block is the parent-session affordance for the
child run. When the block or its metadata identifies a `child_session_id`, an
interactive surface should provide a way to open that original subagent thread.
Pointer-based surfaces expose this as an explicit `Open` action on the Agent
row, separate from expanding or collapsing the row's inline detail.
Opening the subagent thread preserves the child thread identity, selected-agent
identity, policy, transcript, live state, and future prompt submissions.

`agent_session_start` is an identity and status enrichment for the parent
`Agent` tool block. It must not project as a second ordinary parent transcript
entry for the same child invocation. Live and committed projection must use the
same `Agent` block identity across pending, running, completed, failed,
interrupted, and reloaded states. The block keeps the resolved agent name, task
summary, original task prompt when available, parent thread/session identity,
and child thread/session identity in structured metadata. Completion updates
the same block instead of appending a trailing openable Agent row below the
assistant answer.

While the parent remains visible, scoped child events may be summarized inside
the parent Agent block, but their transcript entries belong to the child
thread. Runtime or Gateway projection must preserve the scoped child thread id
on any live transcript entry emitted for child work. A parent transcript must
not render child `Thinking`, tool, text, diagnostic, or terminal entries as
ordinary parent entries. When the subagent thread is opened, the same scoped
events are routed to the child transcript using ordinary transcript projection.
If a surface retained child live-event backlog before the child was opened,
opening the child replays that backlog through the same reconciliation rules as
normal live transcript events.
Failed and interrupted child-thread turns use the shared turn lifecycle
projection; parent Agent blocks may summarize child status, but the child
thread transcript remains the full child-session inspection surface.
Live transcript entry state is turn-scoped. When a parent or child turn reaches
a terminal state, the corresponding projector clears turn-local live blocks,
tool aliases, cached tool arguments, exec-session bindings, and segment counters
before accepting events for a later turn. Later turns must not inherit Agent,
tool, Thinking, text, diagnostic, or terminal blocks from an earlier turn's
live overlay.

The parent Agent block is openable only when it carries a real child thread
identity, such as `child_session_id` or `session_id`, from runtime or Gateway
metadata. Running and completed Agent blocks use the same identity extraction
contract; completion must not drop the child thread id or change the block into
a generic tool row that cannot open the original child thread.
Authoritative snapshot replacement must preserve a child thread target learned
from an earlier live identity event when the matching committed block still
represents the same invocation. If a committed block lacks child identity,
Gateway should first enrich it from durable lineage metadata before clients
replace live state. Failed Agent blocks remain non-openable unless they
actually created a child thread.
Parallel child invocations with the same agent definition must remain distinct
by tool-call or child-thread identity while showing enough task prompt or task
summary text to distinguish their rows.

Completed, failed, interrupted, and closed child agents need not remain in live
agent pickers, but they remain reachable from their parent Agent blocks while
the parent transcript is available. A surface may also keep opened subagent
threads in tabs or history views as ordinary threads.

## Related Topics

- [213 pevo Display Model](spec.md) defines ordinary transcript projection.
- [030 Thread Lineage](../030-state-and-data-model/thread-lineage.md) defines
  shared parent, child, side, and subagent thread vocabulary.
- [051 Subagents](../051-agents/subagents.md) defines child-agent execution.
- [212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md) defines TUI
  projection-specific controls.
- [220 pevo Gateway](../220-pevo-gateway/spec.md) defines Workbench/Web shell
  projection-specific layout.
