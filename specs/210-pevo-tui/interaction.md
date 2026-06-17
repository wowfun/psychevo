---
name: 210. pevo TUI Interaction
psychevo_self_edit: deny
---

# 210. pevo TUI Interaction

Define terminal-specific input handling, keymaps, slash panes, completion
popups, mouse routing, and local selection behavior for fullscreen `pevo tui`.

Shared composer, command routing, permission/clarify, steer/queue, shell mode,
undo/redo, and interrupt semantics are defined by
[270 UI Interaction](../270-ui-interaction/spec.md). This topic owns only the
terminal projection of those shared contracts.

## Scope

- composer key handling, prompt submission, history recall, shell mode entry,
  local text selection, and terminal clipboard behavior
- terminal slash menu behavior, user-configured slash aliases/keybindings,
  bottom panes, and fullscreen command feedback projection
- file, image, agent, and skill popup interactions
- fullscreen mouse routing, wheel routing, transcript focus, row toggles, and
  local copy selection
- TUI-specific permission/clarify panel layout and shortcut handling
- TUI-specific child-agent navigation controls and Agent row open affordances

Out of scope:

- shared composer, command, permission/clarify, interrupt, and display-only
  feedback semantics; these belong to
  [270 UI Interaction](../270-ui-interaction/spec.md)
- transcript row layout, evidence projection, sidebar rendering, status-line
  visual composition, and visual diagnostics; these belong to
  [Rendering](rendering.md) and [260 UI Rendering](../260-ui-rendering/spec.md)
- durable session and model metadata contracts; these belong to
  [Spec](spec.md)

## Keymap And Composer

The fullscreen keymap is composer-first. Core editing, quit, pane, popup,
selection, and interruption controls remain fixed so the terminal surface stays
recoverable. Users may configure slash command shortcuts only through the
effective `config.toml` `tui.slash_keybinds` map.

Default key behavior:

- `Enter` submits the composer or executes the selected slash completion.
- `Shift+Enter`, `Ctrl+Enter`, `Alt+Enter`, and `Ctrl+J` insert a newline.
- `Shift+1` from an empty composer enters shell mode; empty shell mode exits
  with `Esc` or `Backspace`.
- `Tab` completes slash commands in slash input and inserts the selected file
  path in `@` completion popups.
- `Shift+Tab` cycles `default -> acceptEdits -> plan -> default`; dangerous
  bypass modes are not part of the normal cycle.
- `Esc` clears active UI state before interrupting work: local selection,
  popups, slash menus, bottom panes, history search, and empty shell mode all
  take priority.
- `Ctrl+T` focuses the transcript; in transcript focus, `Up`/`Down` move the
  focused row, `PageUp`/`PageDown` scroll, `Enter`/`Space` toggles folded
  evidence or opens clickable Agent rows, and `Esc` returns to composer focus.
- `Ctrl+C` copies and clears active TUI selection, otherwise requests quit.
- `Ctrl+D` quits.
- `Ctrl+O` copies the latest visible assistant answer as raw Markdown source.
- `Ctrl+B` toggles the local context sidebar.
- `Ctrl+R` enters composer history search.
- `?` opens contextual shortcut help when the current surface supports it.

Composer `Ctrl+A` and mouse drag create input-local textarea selection.
Selection is edit-only: release does not copy to the clipboard, and later
`Backspace`, `Delete`, typing, or bracketed paste replace selected text.

Plain `Up` and `Down` in composer focus are input/history boundary keys, not
transcript scrolling keys. They recall submitted composer history only when the
cursor is on the relevant first or last logical line.

## Slash Menu And Bottom Panes

Slash command discovery is backed by the shared runtime command catalog defined
by [026 Commands](../026-commands/spec.md). The TUI supplies terminal
capabilities such as pickers, clipboard, renderer toggles, process exit, Side
chat, and image attachment, then projects shared command effects into terminal
panes, command feedback rows, composer state, queues, and approvals.

The slash menu stays a flat list with at most eight rows and does not show
group headers. Built-in compatibility aliases may match the canonical command
row but do not appear as independent rows. User-configured aliases appear as
alias rows when matched. Configured shortcut dispatch works only from an empty
composer while shell mode, selection, popup, bottom pane, and history search
are inactive.

Fullscreen `/help` opens a bottom help pane with `Help`, `General`,
`Commands`, and `Custom commands` sections. `Esc` closes the pane, and
tab/arrow navigation may switch help sections. Scripted `/help` prints the
same deterministic help text without opening a pane.

Slash commands that open bottom panes, including `/help`, do not append a
command transcript row. Terminal command feedback that remains in the
transcript echoes the submitted command as `> <command>` and renders the local
result below it as display-only material.

## Completion, Selection, And Mouse Routing

Typing an `@` token in the fullscreen composer opens a file path completion
popup for the selected workdir. Valid tokens start at the beginning of the
current line or after whitespace. The popup is hidden while a bottom selection
pane is open. Shell mode reuses the same `@` file path completion popup; image
paths selected this way remain plain shell text and do not create attachments.

Mouse wheel events route by the pointer row: transcript hover scrolls the
transcript, bottom-pane hover scrolls the pane, and composer/status/other
non-scrollable hover does not trigger composer history recall. Mouse clicks on
expandable rows toggle details unless the click is part of text selection.

## Permission, Clarify, And Agent Controls

When a running foreground or background turn reaches a permission prompt, TUI
shows a bottom approval panel. The panel owns a FIFO queue of approval requests
and displays the source session/agent when the request did not originate from
the active main thread.

Approval panels use terminal list-selection behavior with Up/Down or `j`/`k`,
`Enter` to accept the highlighted option, `Esc` to cancel/deny, and direct
action shortcuts where the current approval type exposes them. Mouse clicks on
approval option rows select and immediately resolve the clicked option through
the same decision path as keyboard confirmation.

The `/approve` and `/deny` slash commands resolve the current pending approval
or the most recent smart-review denial override. `/approve` accepts `once`,
`session`, or `always`; omitted scope defaults to `once`. These commands must
not substitute for the model-visible clarify tool.

`/agents`, `@agent-name`, and `/fork` are TUI projections for agent definition
discovery and first-class child-agent runs. Opening an Agent row follows
[250 Thread Navigation](../250-ui-display-model/thread-navigation.md). In TUI,
opening enters that child thread in the foreground and the active composer
follows the displayed session. Parent navigation is available through
`Alt+Left` and the mnemonic `Alt+P`.

## Gateway Control

The fullscreen TUI owns slash parsing and local pending-input UI, but active
turn control is delegated to Gateway. Foreground turns are addressed by the
current `GatewayThreadSelector` and active turn id. Interrupt keys and exit
cleanup call Gateway interrupt for the active selector, then clear local
approval, clarify, and pending steer UI.

Gateway rejects stale steer attempts whose expected turn id no longer matches
the active turn. The TUI presents that rejection as bounded feedback and leaves
the current transcript state consistent with the latest Gateway event.

## Related Topics

- [Spec](spec.md) is the parent topic for command, state, sessions, and
  cross-cutting behavior.
- [Rendering](rendering.md) defines transcript, status-line, sidebar, and
  visual projection behavior.
- [270 UI Interaction](../270-ui-interaction/spec.md) defines shared
  interaction semantics.
- [041 Permissions](../041-permissions/spec.md) defines permission modes and
  approval semantics projected through interactive commands.
