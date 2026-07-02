---
name: 250. UI Display Model Thread Navigation
psychevo_self_edit: deny
---

# 250. UI Display Model Thread Navigation

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

Side chat and subagent inspection use the same child thread view navigation
model: the visible tab or split view points at a concrete child thread
identity, while parent transcript rows and right-column buttons are only
affordances for opening that identity. Display surfaces must consume the
lineage/display metadata projected from the shared state model instead of
maintaining a second source of truth for side chat or subagent identity.

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
side thread. Turns submitted to a side-conversation thread are thread-scoped and
must not bind or rebind the caller-facing Gateway source. The visible side
transcript starts at the side-chat boundary: inherited parent messages and
hidden boundary instructions remain model-visible reference context but must not
render as ordinary transcript entries. Closing a temporary side chat deletes
only the temporary side thread transcript and messages, clears any retained
live-event backlog for that side thread, and returns/focuses the parent. If side
work is running, interrupting the side work takes precedence over deleting the
side thread.

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
For display, the parent `AgentInvocationBlock` is the parent-thread affordance
for the child run. When the block carries a `child_thread_id`, an interactive
surface should provide a way to open that original subagent thread.
Pointer-based surfaces expose this as an explicit `Open` action on the Agent
row/block, separate from expanding or collapsing the inline detail.
Opening the subagent thread preserves the child thread identity, selected-agent
identity, policy, transcript, live state, and future prompt submissions.

`AgentInvocationBlock` is the shared display object consumed by TUI and GUI for
parent transcript Agent blocks. Its stable block id is derived from
`tool_call_id`. Its open target is `child_thread_id`. Its parent block status
is separate from the child thread's internal turn state, so a successful
background handoff remains an openable running Agent block instead of becoming
a completed parent tool row or an interrupted placeholder. The block contains
structured metadata for `tool_call_id`, `parent_thread_id`, `child_thread_id`,
`task_name`, `agent_path`, `agent_type`, original `message`, status, summaries,
token usage, and error state when known. Display labels may be derived from
those fields, but labels are never identity.

`agent_session_start`, `spawn_agent` begin/end events, child activity, and
committed history are all inputs to the same shared display projector. They
must upsert one `AgentInvocationBlock` by `tool_call_id` and never project as a
second ordinary parent transcript entry for the same child invocation. Live and
committed projection must use the same block identity across pending, running,
completed, failed, interrupted, and reloaded states. The block keeps the
resolved agent name, canonical task name, original task message when available,
parent thread/session identity, and child thread/session identity in structured
metadata. Completion updates the same block instead of appending a trailing
openable Agent row below the assistant answer.
Partial live tool-call frames are display previews only. They may introduce a
provisional Agent block and update incomplete argument text, but they do not
define a new identity once a block is bound to a concrete `tool_call_id` or
`child_thread_id`. The projector must merge Agent metadata by typed field
ownership: durable identity fields such as `tool_call_id`, `parent_thread_id`,
and `child_thread_id` are preserved for that invocation, while volatile fields
such as `args`, `arguments`, `result`, status, and summaries are replaced only
by events for the same invocation. This prevents one parallel Agent's partial
arguments from changing another Agent block's title, status, or open target.

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

The parent Agent block is openable only when it carries a real `child_thread_id`
from runtime or Gateway metadata. Storage-specific `child_session_id` values may
be used internally to enrich that public thread id, but UI code must not infer
an open target from title text, task labels, prompts, summaries, result text, or
agent definition name. Running and completed Agent blocks use the same identity
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

- [Spec](spec.md) defines ordinary transcript projection.
- [030 Thread Lineage](../030-state-and-data-model/thread-lineage.md) defines
  shared parent, child, side, and subagent thread vocabulary.
- [051 Subagents](../051-agents/subagents.md) defines child-agent execution.
- [210 pevo TUI Interaction](../210-pevo-tui/interaction.md) defines TUI
  projection-specific controls.
- [240 pevo Web](../240-pevo-web/spec.md) defines Workbench/Web shell
  projection-specific layout.
