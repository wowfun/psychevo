---
name: 260. UI Rendering Agent Rows
psychevo_self_edit: deny
---

# 260. UI Rendering Agent Rows

Define shared rendering behavior for subagent work in parent and child
transcript views.

## Agent Rows

Surfaces render the shared display model's `AgentInvocationBlock` as one
compact subagent row in the parent transcript. The collapsed row uses a display
label derived from the resolved agent definition, canonical task name, or task
message summary. Its identity and open target come only from block fields,
never from display label text.

Foreground and background `spawn_agent` invocations share the same parent-row
identity rule: one model tool invocation and child thread may produce at most
one parent Agent row. A background `spawn_agent` result with
`background=true` and `status=running` is a child-thread handoff, not a
terminal parent-tool completion. It must keep the row live/openable and remain
bound to the same `tool_call_id` so later child-session starts, child previews,
and late streaming fragments update that row instead of creating another row.

When such a handoff arrives through the display stream, the transcript block
may still have `status=running` because the child session is running. Surfaces
must not treat that block, or a later child-session update for the same
openable row, as an active parent tool row. Parent turn cleanup must not
temporarily mark the row interrupted only because the child is still running.

## Identity And Reconciliation

Partial streaming `spawn_agent` rows created before the provider emits a stable
tool call id must reconcile into the later child-session row when execution or
child-session metadata arrives. The parent transcript must not retain a
separate placeholder row such as only the agent name.

For parallel calls to the same agent definition, reconciliation uses the shared
projector's stable block id derived from `tool_call_id`. A surface must not own
Agent-specific placeholder, title, prompt, or result-text matching as a source
of truth. A completed invocation, or a background invocation handed off to a
running child thread, leaves exactly one parent Agent row.

Late streaming pending-tool or assistant message-end tool blocks that arrive
after the corresponding `spawn_agent` invocation has already completed or
handed off to a child thread are stale. They may refresh aliases at most and
must not create or reactivate another Agent row.

## Parent And Child Views

Parent rows may show a bounded live tail preview of child Thinking, tool, and
message activity, but never duplicate the full child transcript. Streaming
child Thinking deltas are coalesced into one preview segment per contiguous
Thinking block so provider chunking does not create one preview line per token
or fragment.

Expanded rows reveal the original prompt and response summary for quick
inspection. For a running Agent row, the expanded prompt is the
model-supplied `spawn_agent.message` captured at tool start. If completion
metadata omits the original message, the row keeps the message captured from
the running tool call so expansion remains stable before and after completion.

Entering a running child agent session shows that child session's live
Thinking, tool, and message stream using the normal session transcript
renderer. While the parent remains active, scoped child events are buffered for
the child session and summarized in the parent Agent row, so opening the child
during a live run immediately replays current work before future scoped events
continue streaming.

Long Thinking, tool, and Agent preview bodies share the same evidence-row
folding, active elapsed timing, and live middle-folded preview behavior defined
by [Evidence](evidence.md). Agent-specific affordances, such as `Open`, are
title actions on top of that shared row behavior rather than a separate row
interaction model.

## Related Topics

- [Spec](spec.md) defines the parent rendering contract.
- [250 Thread Navigation](../250-ui-display-model/thread-navigation.md) defines
  shared child-thread display and navigation behavior.
- [051 Subagents](../051-agents/subagents.md) defines child-agent execution.
