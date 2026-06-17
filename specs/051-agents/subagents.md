---
name: 051. Subagents
psychevo_self_edit: deny
---

Define child and forked agent execution semantics for Psychevo.

## Scope

- child-agent and forked-agent invocation semantics
- foreground and background child-run behavior
- generic agent control operations: list, wait, send, close, resume
- parent/child thread lineage, durable agent edges, and parent result observation
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

`spawn_agent` is the only model-visible spawn entrypoint. It creates an Agent
invocation and, on successful spawn, a child agent thread. Its required fields
are `task_name`, the canonical machine key for this invocation, and `message`,
the initial task prompt/message for the child agent.

`agent_type` is optional and selects the agent definition/role. When omitted,
runtime defaults to the configured default agent definition, normally
`general`, unless an explicit `@agent-name` mention requires a specific
definition. `task_name` never selects the agent definition.

`task_name` must contain only lowercase ASCII letters, digits, and underscores.
It must not be empty and must not be `root`, `.`, or `..`. Values containing
non-ASCII characters, spaces, hyphens, slashes, or other punctuation are
invalid. Runtime rejects invalid names with a clear error such as
`task_name must use lowercase letters, digits, and underscores`. Runtime must
not silently sanitize, lowercase, transliterate, or otherwise rewrite
model-supplied `task_name` values.

The model-visible schema does not expose `name` and unknown fields fail
argument validation. A caller that sends `name`, `prompt`, or any other
non-schema field receives a tool-argument error and no child thread is created.

The tool also accepts a background flag, optional model override, optional fork
behavior, optional max-turn override, and optional `max_spawn_depth` override.
`fork_turns` accepts `none`, `all`, or a positive integer string and defaults
to `all` when fork context is enabled.

An explicit `@agent-name` mention in the parent prompt must resolve to the same
definition name in the Agent invocation. Runtime may inject the single required
agent name when the model omits `agent_type`; it must not silently fall back to
`general` for that required delegation.

`spawn_agent` is a tool declaration and does not authorize execution by itself.
Runtime still applies the active mode ceiling, selected-agent policy, parent
invocation safety policy, scoped MCP availability, and resource boundaries.

## Run Lifecycle And Control

Foreground subagents block the tool call until completion and return the final
summary as the model-visible tool result. The foreground final summary must not
also be projected through the parent mailbox.

Model-visible subagent results are compact summary JSON objects. Runtime may
emit and persist richer metadata for system surfaces, but future model context
receives only the compact projection unless a control handle is explicitly
needed. The common summary projection contains:

- `agent_name`
- `task_name`
- `status`
- `exit_reason`
- `summary`
- `duration_ms`
- `tool_call_count`
- `model`
- `tokens`

`tokens` contains `input`, `output`, `reasoning`, and `total` when those values
are available for the direct child thread. Unavailable fields are omitted
rather than serialized as `null`. Failed runs use `status`, `exit_reason`, and
an `error` field when an error string is available; they do not expose a
separate `error_kind`. `task_name` is the canonical key supplied to
`spawn_agent`; human-readable task or result text is carried only in prompt,
summary, or display metadata. Summary text is not hard-truncated.

The full structured tool output remains available to runtime-owned system
surfaces such as `ToolExecutionEnd` stream events, TUI rows, durable child
session metadata, agent edges, mailbox metadata, debug output, and export
metadata. Tool result messages persisted for future model context may store a
smaller model-visible content string than the full structured event payload.

Background subagents return a handle immediately, but only after runtime has
created or reserved the child thread identity, created the durable backing
session when local storage is available, written the parent/child edge, and
attached the child identity to runtime-owned tool-result/display metadata. The
model-visible summary remains compact; interactive surfaces use richer
metadata to open the child thread. Completion records final summary and status,
then writes one parent mailbox event. Runtime must not persist that completion
as a normal parent `user` message. The mailbox payload uses structured
inter-agent communication content containing a `subagent_notification` whose
content is the compact summary projection. The notification does not include
`agent_id`; the full mailbox record and metadata retain identity,
child-thread, outcome, and operational details for system inspection.

Runtime/display metadata for every successful Agent invocation contains one
canonical `AgentInvocation` record:

- `tool_call_id`: parent tool invocation identity.
- `parent_thread_id`: parent thread identity.
- `child_thread_id`: child agent thread identity.
- `task_name`: canonical machine key.
- `agent_path`: canonical path such as `/root/translate_zh_to_en`.
- `agent_type`: resolved agent definition/role.
- `message`: original task message.
- `status`: `pending`, `running`, `completed`, `failed`, `interrupted`, or
  `closed`.
- `result_summary`, `error`, `tokens`, and `child_tool_call_count` when known.

Storage implementations may keep `parent_session_id` and `child_session_id` as
backing-record fields. Display and navigation APIs use `parent_thread_id` and
`child_thread_id`.

Interactive clients may also start a background subagent directly from a
selected definition. That run uses fresh child context by default, records the
same durable parent/child edge as model-triggered `spawn_agent` tool calls, and
writes a short parent-session observation row so the child transcript remains
discoverable after it leaves the live running view.

Control tools use first-class agent naming rather than subagent-specific names:
`list_agents`, `wait_agent`, `send_message`, `close_agent`, and
`resume_agent`. `wait_agent` accepts only an optional timeout, waits for any
pending or newly arriving parent mailbox event, and returns a small status
object with `message` and `timed_out`. It never returns child `final_answer`
content. When multiple background children complete before a wait boundary, one
wait delivery drains the current pending batch; the parent model may call
`wait_agent` again to wait for later completions. If the model never calls
`wait_agent`, pending mailbox events remain buffered until the next parent
model-input boundary, where they are delivered once and retained as structured
mailbox history for subsequent requests.

`list_agents`, `send_message`, `close_agent`, and `resume_agent` expose compact
model-visible status objects. These control-related outputs include `agent_id`
because the model may need a durable target handle. Control targets resolve by
`agent_id` or by canonical `task_name`. If a task name matches multiple agents,
runtime returns an ambiguity error asking the caller to use `agent_id`.

`close_agent` closes the target's control edge, requests shutdown for running
work, recursively closes open descendants, and returns the previous status.
`send_message` can automatically resume a closed or completed agent in the
background and continue it as a new turn. Runtime also exposes a
pause-new-spawns state for interactive control surfaces.
Pausing blocks future `spawn_agent` requests while leaving already running
children alone. Resuming allows new spawn requests again. Stop subtree uses the
same cooperative-then-force semantics as stopping a single child, applied to
the target and all live descendants.

Agent status follows a fixed status lattice: `pending_init`, `running`,
`completed(summary)`, `errored`, `interrupted`, `shutdown`, and `not_found`.
Timeout is reported separately and is not itself an agent status.

## Thread Lineage

Child agent runs use thread lineage to relate a child thread to its parent. When
the child has durable local state, the child thread's backing session is the
durable agent body. Runtime projects `AgentRun` state from session metadata,
live registry state, and a durable parent-to-child agent edge. The edge records
coordination state as `open` or `closed`; completion does not automatically
close the edge.

Parent result observations must not redefine core execution semantics. The
child invocation still emits its own agent lifecycle under
[002 Agent Execution](../002-agent-execution/spec.md). Foreground
`spawn_agent` calls return the child handle and concise result through the
normal tool result, and runtime also emits a local `agent_session_start` stream
event once the child thread exists so interactive clients can open the child
while it is running.
That start event enriches the already-visible `spawn_agent` invocation with
child identity; it is not a separate parent transcript fact and must not cause a
second parent Agent block for the same child run.
While the child run is active, child-thread stream events are emitted with an
explicit child-thread scope so interactive clients can route them to the child
transcript when it is active, or summarize them inside the parent Agent row when
the parent transcript remains active. Clients may retain a bounded live-event
backlog per child thread so opening a running child can immediately show work
that started before inspection. The foreground tool row is still the only
parent-transcript inspection affordance for that invocation.
The canonical `spawn_agent` argument for selecting a definition is
`agent_type`. Required-agent mention accounting must use `agent_type`; it must
not treat `name` as an alias or infer child identity from a task label.
Completion observations are mailbox events, not human prompts. TUI may render
agent start, wait, close, and completion status as tool/event rows, but those
rows must not create a second model-visible copy of the child final answer.
Legacy hidden contextual user notifications may still appear in old sessions;
TUI must not render hidden notifications as separate rows. Start observations
are UI-facing local records and are not human prompts.
Child metadata records the resolved definition name, generated or provided
`task_name`, parent thread/session id, source/path, role, background/fork
settings, and effective remaining spawn depth.

Existing session records are not migrated. Old verbose tool results and
mailbox records remain historical development data. New records are compact by
construction, and old `last-provider-request` reconstruction remains unchanged.

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
- [210 pevo TUI Interaction](../210-pevo-tui/interaction.md) owns
  interactive projection.
