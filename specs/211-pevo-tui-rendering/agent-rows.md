---
name: 211. pevo TUI Rendering Agent Rows
psychevo_self_edit: deny
---

# 211. pevo TUI Rendering Agent Rows

Define how foreground subagent work is rendered in parent and child transcript views.

## Agent Rows

Foreground `Agent` tool calls render as a single compact subagent block in the
parent transcript. The collapsed row uses the callable definition name plus the
task summary, keeps one explicit right-side `Open` title action for entering
the child session as soon as runtime creates that child session, and shows
status/elapsed time, child tool-use count, and the child session's latest
assistant token usage when available.

Parent rows may show a bounded live tail preview of child Thinking, tool, and
message activity, but never duplicate the full child transcript. Streaming
child Thinking deltas are coalesced into one preview segment per contiguous
Thinking block, so provider chunking does not create one `Thinking:` line per
token or fragment. Expanded rows reveal the original prompt and response
summary for quick inspection.

Partial streaming Agent tool rows that are created before the provider has
emitted a stable tool call id must be reconciled into the later child-session
row when `tool_execution_start` or `agent_session_start` arrives; the parent
transcript must not retain a separate no-`Open` placeholder row such as only
the agent name. When returning to or reloading a parent session while a
foreground child is still running, the TUI must enrich persisted active
`Agent` tool-call rows from durable parent-to-child agent edges so the row
keeps its full agent title and `Open` affordance even though the local
`agent_session_start` event is not replayed as a history message.

Hidden contextual completion notifications are not rendered as separate TUI
rows, so a foreground subagent never creates two clickable `Agent` entries for
the same child session.

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
