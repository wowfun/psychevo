---
name: 211. pevo TUI Rendering
psychevo_self_edit: deny
---

# 211. pevo TUI Rendering

Define the fullscreen ledger layout, evidence projection, rendering rules, and
visual diagnostic expectations. Shared surface language and component contracts
come from [080 Design System](../080-design-system/spec.md).

This topic is the source of truth for how the fullscreen TUI looks and how
runtime evidence becomes transcript rows. Input routing, slash commands, and
panels are defined by [212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md).
Ordinary Status transcript rows are quiet ledger notices: they use the dim
`·` marker, hide the generic `Status` title, and keep any detail under the same
tree indentation used by evidence bodies.
Tool evidence title text is tool-name first. Fullscreen rendering should show
actual tool invocation names and useful arguments, not category verbs such as
`Exploring`/`Explored`, `Running`/`Ran`, or `Updating`/`Updated`.
Transcript rendering consumes semantic display blocks and reusable renderable
components as defined by [213 pevo Display Model](../213-pevo-display-model/spec.md).
The TUI must not persist viewport-wrapped terminal lines as durable display
state.

## Scope

- alternate-screen lifecycle and fullscreen viewport layout
- transcript ledger blocks for prompts, Thinking, tool evidence, Agent rows,
  assistant answers, turn metadata, and local attachment metadata
- active and completed evidence-row presentation, expansion, folding,
  elapsed labels, and shared activity motion
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
- [Evidence Projection](evidence-projection.md) defines runtime-event to ledger
  evidence mapping, active tool rows, folding, and metadata projection.
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
  Codex-style `/diff` overlay.
