---
name: 211. pevo TUI Rendering Agent Rows
psychevo_self_edit: deny
---

# 211. pevo TUI Rendering Agent Rows

Define how subagent work is rendered in parent and child transcript views.

## Agent Rows

The TUI renders the shared display model's `AgentInvocationBlock` as a single
compact subagent row in the parent transcript. The collapsed row uses a display
label derived from the resolved agent definition, canonical task name, or task
message summary, keeps one explicit right-side `Open` title action for entering
the child thread as soon as runtime creates that child thread, and shows
status/elapsed time, child tool-use count, and the child thread's latest
assistant token usage when available. The row's identity and open target come
only from `AgentInvocationBlock` fields, never from the display label.

Foreground and background `spawn_agent` invocations share the same parent-row
identity rule: one model tool invocation and child thread may produce at most
one parent Agent row. A background `spawn_agent` tool result with
`background=true` and `status=running` is a child-thread handoff, not a
terminal parent-tool completion. It must keep the row live/openable and remain
bound to the same `tool_call_id` so later `agent_session_start`, child previews,
and late streaming tool-call fragments update that row instead of creating
another row.
When this handoff arrives through the Gateway display stream, the transcript
block may still have `status=running` because the child session is running.
The TUI must not treat that block, or a subsequent `agent_session_start` update
for the same openable row, as an active parent tool row. It keeps the parent
row non-interrupted and openable, with no parent-tool spinner, so normal parent
turn cleanup cannot temporarily mark the row `interrupted`.

Parent rows may show a bounded live tail preview of child Thinking, tool, and
message activity, but never duplicate the full child transcript. Streaming
child Thinking deltas are coalesced into one preview segment per contiguous
Thinking block, so provider chunking does not create one `Thinking:` line per
token or fragment. Expanded rows reveal the original prompt and response
summary for quick inspection.
For a running Agent row, the expanded prompt is the model-supplied
`spawn_agent.message` captured at tool start. It is not the agent definition id,
prompt profile id, or `task_name` handle. If completion metadata omits the
original message, the row keeps the message captured from the running tool call
so expanding the row before and after completion shows the same task context.

Partial streaming `spawn_agent` rows that are created before the provider has
emitted a stable tool call id must be reconciled into the later child-session
row when `tool_execution_start` or `agent_session_start` arrives; the parent
transcript must not retain a separate no-`Open` placeholder row such as only
the agent name. When returning to or reloading a parent session while a
foreground child is still running, the TUI must enrich persisted active
Agent invocation rows from durable parent-to-child agent edges so the row
keeps its full agent title and `Open` affordance even though the local
`agent_session_start` event is not replayed as a history message.
For parallel calls to the same agent definition, reconciliation must use the
shared projector's stable block id derived from `tool_call_id`. The TUI must not
own Agent-specific placeholder/title/prompt matching as a source of truth. If a
provider changes the provisional stream position before execution starts, the
shared projector migrates the old pending block to the resolved invocation and
removes stale aliases. A completed invocation, or a background invocation handed
off to a running child thread, must leave exactly one parent Agent row; any
unmatched provisional row for that same invocation is removed by the projector
before turn cleanup can mark it `interrupted`. Late streaming
`tool_call_pending` or assistant message-end tool blocks that arrive after the
corresponding `spawn_agent` invocation has already completed or handed off to a
child thread are stale. They may refresh aliases at most and must not create or
reactivate another Agent row.
When the TUI consumes raw runtime events directly, it must obey the same
identity contract as the shared projector: exact `tool_call_id` wins, same
stream position may upgrade an unresolved provisional row, and no Agent row may
be adopted by matching only the tool name, agent name, task name, prompt, title,
or result text. A raw execution or session-start event with an unknown
`tool_call_id` creates its own provisional row. Turn cleanup must not label such
rows `interrupted` unless the runtime event explicitly reports an interrupted or
cancelled outcome.

Hidden contextual completion notifications are not rendered as separate TUI
rows, so a subagent never creates two clickable `Agent` entries for the same
child session.

Entering a running child agent session shows that child session's live
Thinking, tool, and message stream using the normal session transcript renderer.
While the parent remains active, scoped child events are buffered for that child
session as well as summarized in the parent Agent row, so opening the child
during a live run immediately replays the current work surface before future
scoped events continue streaming. Child-session views reuse the regular
composer and transcript layout; only parent/sibling navigation hints are added
by the interaction layer.

Long Thinking, tool, and Agent preview bodies use a middle-folding preview: the
first 2 lines/tokens plus the last 4 lines/tokens stay visible and the omitted
middle is represented by a compact marker. Streaming rows recompute the preview
from full text so the trailing window updates live. Thinking, tool, and Agent
rows share the same evidence-row behavior for active elapsed timing,
row-level expand/collapse, and live middle-folded previews. Agent-specific
affordances, such as `Open`, are title actions on top of that shared row
behavior rather than a separate row interaction model.
