---
name: 051. Subagents
psychevo_self_edit: deny
---

Define child and forked agent execution semantics for Psychevo.

## Scope

- child-agent and forked-agent invocation semantics
- foreground and background child-run behavior
- generic agent control operations: list, wait, send, close, resume
- parent/child session lineage, durable agent edges, and parent result observation
- child-run tool-policy constraints
- child-agent hook points at the runtime boundary

Out of scope:

- agent definition discovery and selected-agent behavior, defined by
  [051 Agents](spec.md)
- concrete CLI command spelling, terminal rendering, process behavior, or exit
  codes
- stable storage schemas, wire payloads, or TUI layout details
- byte-for-byte compatibility with any external product
- remote provider, MCP, or API-key validation without explicit opt-in

## First-Class Child Agent Runs

A subagent is a runtime-owned child agent invocation related to a parent
session. It reuses the agent definition model from [051 Agents](spec.md), but a
specific run records whether it is a named child agent or a fork. Child agents
are first-class agent invocations; "subagent" describes their relationship to a
parent session.

Named subagents default to fresh context. Forked subagents receive a
runtime-captured parent context snapshot. `/fork` and `fork_context=true` run
in the background by default. Combining `agent_type` with `fork_context=true`
means "use this named agent definition with forked parent context."

Child agents cannot spawn further agents by default. Nested subagents are a
Psychevo extension controlled by an effective `max_spawn_depth` value. The
value counts additional spawn levels after the direct child: unset, `null`, or
`0` makes the direct child a leaf; `1` lets that child create grandchildren
whose remaining depth is `0`. Runtime clamps effective values to a safe
implementation cap. When an agent definition is used as the main session
identity, it may use agent tools only if its effective tool policy includes
them.

## Model-Visible Entry Point

The `Agent` tool is the primary model-visible spawn entrypoint. Its only
required field is `prompt`. `agent_type` is the canonical agent definition name.
`name` is accepted only as a compatibility alias for `agent_type`; if both
fields are supplied with different non-empty values, the tool call fails.
When no agent name is supplied, runtime defaults to `general`. When an explicit
agent name is supplied and no matching definition exists, runtime fails the
tool call and does not create a child session. `task_name` is a separate
automation/debug handle and never changes definition selection. The tool also
accepts a background flag, optional model override, optional fork behavior,
optional max-turn override, and optional `max_spawn_depth` override.

An explicit `@agent-name` mention in the parent prompt must resolve to the same
definition name in the Agent invocation. Runtime may inject the single required
agent name when the model omits `agent_type`; it must not silently fall back to
`general` for that required delegation.

`Agent` is a tool declaration and does not authorize execution by itself.
Runtime still applies the active mode ceiling, selected-agent policy, parent
invocation safety policy, scoped MCP availability, and resource boundaries.

## Run Lifecycle And Control

Foreground subagents block the tool call until completion and return the final
summary as the tool result.

Background subagents return a handle immediately. Completion records final
summary and status, and may inject or project a subagent-result observation to
the parent session.

Interactive clients may also start a background subagent directly from a
selected definition. That run uses fresh child context by default, records the
same durable parent/child edge as model-triggered `Agent` tool calls, and writes
a short parent-session observation row so the child transcript remains
discoverable after it leaves the live running view.

Control tools use first-class agent naming rather than subagent-specific names:
`list_agents`, `wait_agent`, `send_message`, `close_agent`, and
`resume_agent`. `wait_agent` may wait on multiple targets and returns both
statuses and timeout information. `close_agent` closes the target's control
edge, requests shutdown for running work, recursively closes open descendants,
and returns the previous status. `send_message` can automatically resume a
closed or completed agent in the background and continue it as a new turn.
Runtime also exposes a pause-new-spawns state for interactive control surfaces.
Pausing blocks future `Agent` spawn requests while leaving already running
children alone. Resuming allows new spawn requests again. Stop subtree uses the
same cooperative-then-force semantics as stopping a single child, applied to
the target and all live descendants.

Agent status follows a fixed status lattice: `pending_init`, `running`,
`completed(summary)`, `errored`, `interrupted`, `shutdown`, and `not_found`.
Timeout is reported separately and is not itself an agent status.

## Lineage

Child agent runs use session lineage to relate a child session to its parent.
The child session is the durable agent body. Runtime projects `AgentRun` state
from session metadata, live registry state, and a durable parent-to-child agent
edge. The edge records coordination state as `open` or `closed`; completion
does not automatically close the edge.

Parent result observations must not redefine core execution semantics. The
child invocation still emits its own agent lifecycle under
[002 Agent Execution](../002-agent-execution/spec.md). Foreground `Agent` tool
calls return the child handle and concise result through the normal tool result,
and runtime also emits a local `agent_session_start` stream event once the
child session exists so interactive clients can open the child while it is
running. While the child run is active, child session stream events are emitted
with an explicit child-session scope so interactive clients can route them to
the child transcript when it is active, or summarize them inside the parent
Agent row when the parent transcript remains active. Clients may retain a
bounded live-event backlog per child session so opening a running child can
immediately show work that started before inspection. The foreground tool row
is still the only parent-transcript inspection affordance for that invocation.
The canonical `Agent` argument for selecting a definition is `agent_type`, but
runtime also accepts `name` as a compatibility alias and treats it identically
for execution and required-agent mention accounting.
Completion may still write a hidden contextual user notification to the parent
session so later parent turns can see child-agent status without treating the
notification as new human intent. TUI must not render hidden notifications as
separate rows. Background/manual starts and completions may write visible local
status rows because they do not otherwise have a foreground tool-result row.
Start observations are UI-facing local records and are not human prompts.
Child metadata records the resolved definition name, generated or provided
`task_name`, parent session id, source/path, role, background/fork settings, and
effective remaining spawn depth.

No daemon or supervisor is required in the first implementation slice. If the
process exits while work is active, the active provider call is interrupted,
but the session and open edge remain durable so attach, resume, or
`send_message` can continue later as a new turn.

## Hooks

Runtime owns `SubagentStart` and `SubagentStop` hook execution. These hook
points observe child-run lifecycle boundaries at the runtime boundary and do
not enter `psychevo-agent-core`.

## Related Topics

- [051 Agents](spec.md) defines reusable agent definitions and selected-agent
  policy.
- [002 Agent Execution](../002-agent-execution/spec.md) defines core
  invocation, turn, message, tool execution, and outcome semantics.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime-owned
  agent-invocation assembly and control wiring.
- [007 Tool Surface](../007-tool-surface/spec.md) defines tool declaration and
  execution binding semantics.
- [008 Session Continuity](../008-session-continuity/spec.md) defines session
  lineage and continuity inputs.
- [200 pevo CLI](../200-pevo-cli/spec.md) owns command spelling.
- [212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md) owns
  interactive projection.
