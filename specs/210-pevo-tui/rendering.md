---
name: 210. pevo TUI Rendering
psychevo_self_edit: deny
---

# 210. pevo TUI Rendering

Define terminal-specific fullscreen rendering rules for `pevo tui`.

Shared transcript projection is defined by
[250 UI Display Model](../250-ui-display-model/spec.md). Shared evidence,
Agent row, folding, activity, and observability rendering rules are defined by
[260 UI Rendering](../260-ui-rendering/spec.md). This topic owns only the
terminal projection of those shared contracts.

## Scope

- alternate-screen lifecycle and fullscreen viewport layout
- terminal ledger layout for transcript, composer, status line, sidebar, bottom
  panes, overlays, and selection affordances
- terminal-adaptive palette, Markdown/plain/raw rendering, and reduced-motion
  behavior
- fixed composer/status-line rendering, including terminal cursor anchoring for
  IME candidate windows
- passive redraw cadence for running-state motion, while preserving immediate
  redraws for input and runtime events
- deterministic terminal visual diagnostics and VHS capture expectations

Out of scope:

- shared transcript/evidence semantics, `exec_command` / `write_stdin`
  ownership, Agent row identity, folding, and display-only boundaries; these
  belong to [260 UI Rendering](../260-ui-rendering/spec.md)
- key bindings, slash command parsing, composer editing, popup routing, and
  bottom-panel behavior; these belong to [Interaction](interaction.md) and
  [270 UI Interaction](../270-ui-interaction/spec.md)
- session storage, model selection, variant persistence, and history ownership;
  these belong to [Spec](spec.md)

## Fullscreen Layout

Interactive terminals use raw mode and the alternate screen. Fullscreen startup
enters a clean alternate screen, enables alternate-scroll mode, clears the
alternate buffer, and homes the cursor before the first draw so host scrollback
cannot bleed into the fullscreen view. Fullscreen shutdown restores cursor
visibility, raw mode, mouse capture, alternate-scroll mode, and the alternate
screen on normal exit, errors, and unwinds.

The fullscreen layout is an evidence ledger, not a row-level event log. The
main transcript area is scrollable. The composer and status line are fixed at
the bottom. Optional sidebars and bottom panes are terminal utility surfaces;
they must not be required for the core prompt-to-answer flow.

Prompt blocks and the composer share the same full-width terminal surface
within a render profile. Wrapped prompt rows, including CJK/wide-character
continuation rows, keep that surface so the prompt block does not visually
break at wrap boundaries. Assistant answer material remains unlabeled body
text in the transcript ledger.

The fullscreen composer height is derived from rendered editable input rows.
Empty input uses one visible row. Non-empty input grows from explicit logical
lines and terminal soft-wrapped rows, and remains capped at six visible rows
before the textarea scrolls internally.

The bottom status line is compact and must degrade without overlap at narrow
widths. While foreground work is running, it appends the shared activity marker
and elapsed/interrupt hint to the stable status line instead of replacing
model/mode/context information with generic `Working` text.

## Terminal Rendering

TUI uses a compact terminal-adaptive palette:

- default foreground for primary text
- dim secondary text
- cyan hints and status markers
- green success markers
- red failures
- magenta `pevo` identity

Fullscreen rendering may query terminal default foreground/background under a
bounded startup deadline. When the terminal reports a usable background, TUI
surfaces derive subtle prompt/composer/menu/selection backgrounds from that
default. When probing is unsupported, times out, or runs outside an interactive
terminal, the renderer falls back to the existing dark surface palette.
Terminal probing is best-effort and must never block startup beyond the bounded
deadline or fail TUI startup.

Assistant answer text uses lightweight Markdown rendering for local display
only. Supported styling includes headings, lists, emphasis, inline code, fenced
code blocks, tables, links, and local file links. Raw mode renders assistant
answer bodies and visible Thinking bodies as raw Markdown source, but remains
display-only and must not change persisted message content, provider replay,
`/copy`, non-terminal JSON output, row identity, or tool evidence rendering.

Timeout-only fullscreen redraws used only to advance running-state motion are
coalesced to a bounded passive cadence. Key input, paste, resize, mouse
actions, and drained runtime events still request immediate redraws. This keeps
activity motion responsive without disturbing terminal IME candidate windows at
frame-rate cadence.

## Sidebar And Overlays

The right sidebar is a plain utility appendix. It is optional, low contrast,
local-only, and never required for the core prompt-to-answer flow. Sidebar
content follows the same selected-row, wrapping, and terminal palette rules as
other TUI utility panes.

Read-only overlays such as `/diff` are static display artifacts. They use the
terminal pager shape owned by TUI, but their display-only boundary and diff
semantics are owned by [214 pevo Diff Command](../214-pevo-diff-command/spec.md)
and [270 UI Interaction](../270-ui-interaction/spec.md).

## Visual Diagnostics

Deterministic TUI/VHS visual fixtures use fake providers and keep running
agent, clarification, permission, and tool states observable long enough for
terminal capture and screenshot I/O. Capture timing must not depend on real
provider latency.

## Related Topics

- [Spec](spec.md) is the parent topic for command, state, sessions, and
  cross-cutting behavior.
- [Interaction](interaction.md) defines terminal key handling, slash panes,
  popups, and local selection behavior.
- [250 UI Display Model](../250-ui-display-model/spec.md) defines shared
  transcript projection.
- [260 UI Rendering](../260-ui-rendering/spec.md) defines shared rendering
  invariants.
- [214 pevo Diff Command](../214-pevo-diff-command/spec.md) defines the
  fullscreen `/diff` overlay semantics.
