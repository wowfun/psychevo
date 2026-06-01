---
name: 211. pevo TUI Rendering
psychevo_self_edit: deny
---

# 211. pevo TUI Rendering

Define the fullscreen ledger layout, evidence projection, rendering rules, and
visual diagnostic expectations. Shared surface language and component contracts
come from [080 Design System](../080-design-system/spec.md).

This topic is the source of truth for how the fullscreen TUI looks and how
Gateway timeline events become transcript rows. Input routing, slash commands,
and panels are defined by
[212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md).
Ordinary Status transcript rows are quiet ledger notices: they use the dim
`·` marker, hide the generic `Status` title, and keep any detail under the same
tree indentation used by evidence bodies.
Tool evidence title text is tool-name first. Fullscreen rendering should show
actual tool invocation names and useful arguments, not category verbs such as
`Exploring`/`Explored`, `Running`/`Ran`, or `Updating`/`Updated`.
Tool rows are rendered only for typed tool calls, execution events, or durable
tool timeline items. The TUI must not create an active `write`, `read`,
`exec_command`, or similar row from reasoning/assistant prose alone.
Yielded `exec_command` rows keep the original command invocation as their
title across output chunks, polls, and completion; the numeric session id used
to poll `write_stdin` is not a display title.
`write_stdin` output for a yielded session is appended under the owning
`exec_command` row and must not appear as a separate primary transcript row,
including while it is pending or running. If the terminal `write_stdin` result
has a null `session_id`, the TUI uses the call arguments to finish the owning
`exec_command` row.
Assistant-message transcript order follows the runtime timeline's semantic
message order, including reasoning before later assistant text. Skill activation
status such as `skill loaded: ...` is a typed status row emitted by Gateway and
runtime timeline snapshots, not a raw-stream-only side effect.
Completed assistant answers keep their turn metadata row. In the typed Gateway
path, the TUI reads usage, provider/model, elapsed metadata, accounting, and
terminal-answer eligibility from the assistant timeline item, defers the row
while tools or the foreground turn are still active, and appends it after the
turn is fully complete.
An empty reasoning-completed event only closes an existing Thinking row. It must
not create a title-only Thinking row, and completed history Thinking rows must
not keep live elapsed timers.
Transcript rendering consumes semantic timeline items and reusable renderable
components as defined by [213 pevo Display Model](../213-pevo-display-model/spec.md).
The TUI must not persist viewport-wrapped terminal lines as durable display
state.
Raw runtime/debug observations are not ordinary transcript rows. They may be
shown only when a debug/raw surface explicitly requests bounded debug records.

Live fullscreen turns consume in-process Gateway typed events, not raw runtime
stream events. `GatewayEvent` item lifecycle events are the live rendering
source of truth while a turn is running. TUI may keep small local presentation
state for folding, elapsed timers, selection, and pending panels, but that state
must be derived from Gateway events or runtime-owned timeline snapshots.

History reload and session switching prefer Gateway/thread snapshots backed by
runtime `timeline_items`. The TUI may keep a temporary message/tool-result
replay fallback only for sessions that do not yet have timeline rows; fallback
rows must preserve the same user-visible ledger language and must not become a
second durable display model.

## Scope

- alternate-screen lifecycle and fullscreen viewport layout
- transcript ledger blocks for prompts, Thinking, tool evidence, Agent rows,
  assistant answers, turn metadata, and local attachment metadata
- active and completed evidence-row presentation, expansion, folding,
  elapsed labels, and shared activity motion
- typed Gateway event projection into live transcript rows and runtime-owned
  timeline snapshot projection into history rows
- fixed composer/status-line rendering, including context usage, path/branch
  display, running elapsed projection, and child-session status-line behavior
- active editable-surface terminal cursor anchoring for IME candidate windows
  in fullscreen terminals
- bounded passive redraw cadence for running-state motion, while preserving
  immediate redraws for input and runtime events
- sidebar content and clearing behavior
- lightweight terminal Markdown projection, raw transcript display, and
  copy-visible rendered text boundaries
- non-terminal plain semantic rendering for `pevo tui`
- visual regression and diagnostic artifact expectations, including the
  organization of deterministic VHS fixture assets
- read-only overlay surfaces for display artifacts such as `/diff`

Out of scope:

- key bindings, slash command parsing, composer editing, popup routing, and
  bottom-panel behavior; these belong to
  [212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md)
- session storage, model selection, variant persistence, and history ownership;
  these belong to [210 pevo TUI](../210-pevo-tui/spec.md)

## Attachments

- [Layout](layout.md) defines the fullscreen layout, transcript/composer/status
  line, and sidebar rendering rules.
- [Agent Rows](agent-rows.md) defines foreground subagent row rendering and
  child-session transcript projection.
- [Evidence Projection](evidence-projection.md) defines timeline/event to
  ledger evidence mapping, active tool rows, folding, and metadata projection.
- [Terminal Rendering](terminal-rendering.md) defines terminal-adaptive palette,
  Markdown/raw/plain rendering, transcript scrolling, and runtime-drain display
  behavior.
- [Testing](testing.md) defines rendering acceptance coverage and visual
  regression expectations.

## Related Topics

- [210 pevo TUI](../210-pevo-tui/spec.md) is the parent topic for command,
  state, sessions, and cross-cutting behavior.
- [212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md) defines
  input routing, slash commands, panels, popups, and selection behavior.
- [214 pevo Diff Command](../214-pevo-diff-command/spec.md) defines the
  fullscreen `/diff` overlay.
