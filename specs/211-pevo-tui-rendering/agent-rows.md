---
name: 211. pevo TUI Rendering Agent Rows
psychevo_self_edit: deny
---

# 211. pevo TUI Rendering Agent Rows

Define how subagent work is rendered in parent and child transcript views.

## Agent Rows

`Agent` tool calls render as a single compact subagent block in the
parent transcript. The collapsed row uses the callable definition name plus the
task summary, keeps one explicit right-side `Open` title action for entering
the child session as soon as runtime creates that child session, and shows
status/elapsed time, child tool-use count, and the child session's latest
assistant token usage when available.

Foreground and background `Agent` invocations share the same parent-row
identity rule: one model tool invocation and child session may produce at most
one parent `Agent` row. A background `Agent` tool result with
`background=true` and `status=running` is a child-session handoff, not a
terminal tool completion. It must keep the row live/openable and remain bound
to the same `tool_call_id` so later `agent_session_start`, child previews, and
late streaming tool-call fragments update that row instead of creating another
row.
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
For a running Agent row, the expanded prompt is the model-supplied `Agent` tool
task prompt, normally `args.prompt` or an equivalent task field captured at
tool start. It is not the agent definition id, prompt profile id, or task-name
handle. If completion metadata omits the original task prompt, the row keeps
the prompt captured from the running tool call so expanding the row before and
after completion shows the same task context.

Partial streaming Agent tool rows that are created before the provider has
emitted a stable tool call id must be reconciled into the later child-session
row when `tool_execution_start` or `agent_session_start` arrives; the parent
transcript must not retain a separate no-`Open` placeholder row such as only
the agent name. When returning to or reloading a parent session while a
foreground child is still running, the TUI must enrich persisted active
`Agent` tool-call rows from durable parent-to-child agent edges so the row
keeps its full agent title and `Open` affordance even though the local
`agent_session_start` event is not replayed as a history message.
For parallel calls to the same agent definition, pending rows must not be
matched only by agent name. Reconciliation uses the stable `tool_call_id` when
available, otherwise a strong task identity such as `task_name` plus the task
prompt. Explicit task labels from `task_name` or `task` are the preferred row
detail; runtime-generated labels such as `agent-<id-prefix>` are not user
labels and must not override a useful agent definition description. If a
provider changes the provisional stream position or provisional tool id before
execution starts, the TUI migrates the old pending row to the resolved
invocation and removes stale position/id aliases. A completed invocation, or a
background invocation handed off to a running child session, must leave exactly
one parent Agent row; any unmatched provisional row for that same invocation is
removed before turn cleanup can mark it `interrupted`. Late streaming
`tool_call_pending` or assistant message-end tool blocks that arrive after the
corresponding `Agent` invocation has already completed or handed off to a child
session are stale. If they match an existing row by stable id or by strong task
identity, they must refresh aliases at most and must not create or reactivate
another Agent row.
Streaming providers may also emit partial Agent arguments that contain only the
agent definition, such as `agent_type`, before `prompt` or `task_name` is
parseable. Such rows are weak placeholders, not durable invocations. When the
next resolved `Agent` start for that agent arrives, the TUI may adopt the weak
placeholder only if it is the unique unresolved weak placeholder for that agent;
otherwise it must wait for stable id or strong task identity.

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
