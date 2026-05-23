---
name: 212. pevo TUI Interaction
psychevo_self_edit: deny
---

# 212. pevo TUI Interaction

Define fullscreen input handling, keymaps, slash commands, file completion,
shell escapes, and local selection behavior. Shared interaction principles and
visual treatment come from [080 Design System](../080-design-system/spec.md).

This topic is the source of truth for how users control the fullscreen TUI.
Rendering of the resulting transcript rows and panels is defined by
[211 pevo TUI Rendering](../211-pevo-tui-rendering/spec.md).

## Scope

- composer key handling, prompt submission, history recall, shell mode, and
  queued input behavior
- fixed pending input preview behavior for unsent steer and next-turn queued
  prompt inputs
- slash command registry behavior, parsing, menus, completion, and command
  feedback
- user-visible interaction copy that describes pevo behavior directly without
  naming external reference implementations
- file, image, agent, and skill popup interactions
- bottom panes for help, sessions, models, variants, usage, and agent controls
- fullscreen mouse routing, wheel routing, app-native selection, and clipboard
  behavior
- local user shell escapes and their interaction with active foreground turns
- `/agents`, `@agent`, `/fork`, selected-main-agent, and child-session
  navigation controls from the TUI
- undo/redo command interactions and interruption priority

Out of scope:

- transcript row layout, evidence projection, sidebar rendering, status-line
  visual composition, and visual diagnostics; these belong to
  [211 pevo TUI Rendering](../211-pevo-tui-rendering/spec.md)
- durable session and model metadata contracts; these belong to
  [210 pevo TUI](../210-pevo-tui/spec.md)

## Topic Attachments

- [Keymap and Composer](keymap-and-composer.md) defines fullscreen key handling,
  composer state, paste handling, mouse routing, and local selection basics.
- [Slash Commands](slash-commands.md) defines slash command inventory, parsing,
  command feedback, bottom panes, model/session commands, file completion, and
  local command behavior.
- [Agent Controls](agent-interaction.md) defines `/agents`, `@agent`, `/fork`,
  selected-main-agent, child-session navigation, and Agent row controls.
- [Testing](testing.md) defines interaction acceptance coverage.

## Related Topics

- [210 pevo TUI](../210-pevo-tui/spec.md) is the parent topic for command,
  state, sessions, and cross-cutting behavior.
- [211 pevo TUI Rendering](../211-pevo-tui-rendering/spec.md) defines
  transcript, status-line, sidebar, and visual projection behavior.
- [035 Permissions](../035-permissions/spec.md) defines permission modes and
  approval semantics projected through interactive commands.
