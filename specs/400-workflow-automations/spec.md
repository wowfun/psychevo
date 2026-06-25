---
name: 400. Workflow Automations
psychevo_self_edit: deny
---

Define Psychevo's product workflow automations: user-managed scheduled turns
that can be created from natural language in a thread, edited in Workbench, and
executed by the local Gateway scheduler. This spec is the concrete source of
truth for workflow automation behavior. [060 Automation](../060-automation/spec.md)
keeps the cross-cutting automation principles for evidence, isolation, local
scheduling, and run policy.

## Scope

This topic owns:

- project automations bound to a workspace/workdir
- thread-bound automations appended to an existing thread
- natural-language CRUD through the model-facing `automation` tool
- Workbench automation configuration UX
- scheduler persistence, run records, last-run visibility, and run-now behavior
- compatibility for existing GUI automation RPCs

Out of scope:

- hosted or cloud schedulers
- OS cron, systemd, launch agents, or other machine-level background installers
- provider catalog automation
- cross-machine execution guarantees or guaranteed execution while Gateway is
  closed
- recursive automation management from automation-triggered turns

## Product Model

A workflow automation is a persisted local definition plus append-only run
records. Definitions include title, target, prompt, schedule, enabled state,
optional model/reasoning selection, and execution policy. Run records include
timing, status, target thread/source information when known, and bounded error
text.

Project automations are scoped to a workspace/workdir and use a stable Gateway
source key shaped like `automation:<task-id>`. They must not rebind the ordinary
Workbench workdir source.

Thread-bound automations target an explicit existing thread id. They are used
when scheduled work should continue an established conversation. A thread target
must belong to the selected workspace.

Automation prompts are submitted as ordinary user content. Slash-looking text in
an automation prompt remains prompt text and does not pass through the
Workbench slash-command parser or a separate command scheduler.

## Targets and Defaults

When a user creates or manages automations from inside a thread and does not
specify a target, the current thread is the default target.

Workbench's first accepted prompt for a detached draft is already inside the
current conversation. Before that prompt is sent to a model, Gateway must
materialize a current thread id for the turn so the model-facing `automation`
tool can bind omitted targets or `currentThread` targets to that thread. The
tool must not manufacture a current thread id itself; it only consumes the
thread identity supplied by Gateway.

Outside a thread, creation defaults to the selected/current workspace as a
project automation. Workbench may only offer workspace and thread targets that
are already known to the current local Gateway session.

The Workbench Automations page itself is not bound to the current workdir. It
must not show the current workdir path as page-level context. Workspace context
belongs to draft controls, automation binding controls, and automation rows.

## Schedules

The schedule grammar supports:

- `interval`: a positive `everyMinutes` value
- `daily`: a local `HH:mm` time
- `weekly`: one or more local weekdays plus a local `HH:mm` time
- `delay`: a positive `afterMinutes` value for one-shot relative scheduling
- `once`: a concrete local or RFC3339 timestamp for one-shot scheduling

Schedule calculation must be deterministic under a supplied clock in tests.
Calendar schedules should use a real local-time library rather than ad hoc date
math. Daylight-saving and clock-shift behavior should fail gently by advancing
to the next valid local occurrence instead of creating repeated immediate runs.

`delay` and `once` schedules run at most once. After a successful scheduled run
has been recorded, they have no next scheduled run. Their definitions and run
history remain inspectable, and manual run-now remains available.

## Execution and Persistence

The scheduler runs inside the local Gateway/Web process. It checks due tasks
while the process is alive. If Gateway is closed, missed work is not executed
until Gateway starts again. On restart, a task may run at most once for the
latest missed occurrence; the scheduler must not replay every missed interval as
a backlog.

The scheduler must avoid overlapping runs for the same task. A manual run-now
request and a timed tick compete for the same task claim. If a task is already
running, the later request should report or record a skipped/busy run instead of
starting a second turn.

Pausing and resuming only toggle the automation's enabled state. They must not
delete the definition, target binding, prompt, schedule, or run history. Resuming
a recurring automation recomputes its next run from the current clock and prior
run history.

The transcript remains the durable evidence for model-visible messages and tool
results. Automation run records are coordination and inspection facts, not a
second transcript.

The default execution policy is `Auto in sandbox`: automation turns may use
auto-allow prompt approval while still respecting hard permission denies and
sandbox enforcement. Ask-first policy may run with ordinary permission prompts.
If a scheduled run reaches a user approval or clarify prompt, it becomes an
ordinary pending interaction in the owning thread/source; the scheduler must not
invent a second approval channel.

## Gateway Interfaces

Existing GUI RPCs remain supported:

- `automation/list`
- `automation/draft`
- `automation/write`
- `automation/pause`
- `automation/resume`
- `automation/delete`
- `automation/run`

These RPCs continue to validate targets and schedules through the same
normalization rules used by natural-language creation. `automation/write`
creates and edits automation definitions; it does not change lifecycle state.
Pause and resume are explicit lifecycle mutations through `automation/pause`
and `automation/resume`.

Natural-language actions map to Gateway RPCs as follows:

| Action | Gateway RPC |
| --- | --- |
| `list` | `automation/list` |
| `create` | `automation/write` without `automationId` |
| `update` | `automation/write` with `automationId` |
| `pause` | `automation/pause` |
| `resume` | `automation/resume` |
| `run` | `automation/run` |
| `remove` | `automation/delete` |

## Natural-Language Management Tool

Gateway-backed user turns expose a single model-facing `automation` tool. The
tool uses action-style input and supports:

- `list`
- `create`
- `update`
- `pause`
- `resume`
- `run`
- `remove`

`create` may omit a target only when the current execution context supplies a
clear default: current thread first, otherwise selected/current workspace. If a
model explicitly requests `currentThread`, that is a hard thread-bound target
and must fail when Gateway has not supplied a current thread id; normal
Workbench first-prompt turns must not enter that failure path because their
thread is materialized before model execution.
Mutating actions that operate on an existing automation require an automation id.
`list` defaults to the current workspace rather than only the current thread, so
users can discover project and thread-bound automations together.

Automation draft turns and automation-triggered turns must not expose the
`automation` tool. Scheduled work cannot recursively create, update, or remove
more scheduled work unless a future spec explicitly changes that boundary.

## Workbench UX

Workbench presents automations as an app-level operational surface, not as
Settings and not as a landing page. The first screen should support scanning,
editing, and creating real automations without marketing copy.

The draft/editor area should be centered in the available space and avoid large
unused right-side whitespace. The page title must not display the current
workdir path. Workspace and thread selection belong inside the draft and binding
controls.

When no automations exist and no draft is open, the page should show one
focused empty-state creation area. Project and thread template actions must not
be duplicated, and an empty draft placeholder card must not appear beside the
empty list. The draft editor appears only after the user starts a draft from the
template actions, the New button, an existing automation edit action, or the
natural-language draft flow.

The draft flow supports selecting the workspace and choosing whether the draft
binds to a project automation or an existing thread. Thread selection is shown
when thread binding is selected and there are known threads for the selected
workspace.

Automation rows show title, target, schedule, enabled/paused state, next run
when known, and last run time when known. Rows for tasks that have never run
should say so. Existing automations expose pause/resume, run-now, edit, and
delete without hiding the target or schedule context.

## Related Topics

- [060 Automation](../060-automation/spec.md) defines cross-cutting automation
  principles and local-first validation boundaries.
- [007 Tool Surface](../007-tool-surface/spec.md) defines the model-facing tool
  contract style used by the `automation` tool.
- [021 Gateway](../021-gateway/spec.md) defines Gateway responsibilities for
  local user surfaces.
- [041 Permissions](../041-permissions/spec.md) and
  [045 Sandbox](../045-sandbox/spec.md) define permission and sandbox behavior
  for automated turns.
- [240 Pevo Web](../240-pevo-web/spec.md) defines Workbench app-surface
  expectations.
- [280 Channel UX](../280-channel-ux/spec.md) defines adjacent thread-like user
  surfaces that should share automation management semantics.
