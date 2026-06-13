---
name: 211. pevo TUI Rendering
psychevo_self_edit: deny
---

# 211. pevo TUI Rendering

Define the fullscreen ledger layout, evidence projection, rendering rules, and
visual diagnostic expectations. Shared surface language and component contracts
come from [080 Design System](../080-design-system/spec.md).

This topic is the source of truth for how the fullscreen TUI looks and how
Gateway transcript entries become ledger rows. Transcript state ownership is
defined by
[030 Transcript State](../030-state-and-data-model/transcript-state.md), and
the shared transcript projection contract is defined by
[213 pevo Display Model](../213-pevo-display-model/spec.md). Input routing,
slash commands, and panels are defined by
[212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md).
Ordinary Status transcript rows are quiet ledger notices: they use the dim
`·` marker, hide the generic `Status` title, and keep any detail under the same
tree indentation used by evidence bodies.
Tool evidence title text is tool-name first. Fullscreen rendering should show
actual tool invocation names and useful arguments, not category verbs such as
`Exploring`/`Explored`, `Running`/`Ran`, or `Updating`/`Updated`.
Tool rows are rendered only for typed tool calls, execution events, or shared
transcript tool blocks. The TUI must not create an active `write`, `read`,
`exec_command`, or similar row from reasoning/assistant prose alone.
Yielded `exec_command` rows keep the original command invocation as their
title across output chunks, polls, and completion; the numeric session id used
to poll `write_stdin` is not a display title.
Gateway live transcript entries for yielded `exec_command` must therefore carry
the original `args.cmd` in tool metadata even when the runtime result event only
contains `session_id` and output.
`write_stdin` output for a yielded session is appended under the owning
`exec_command` row and must not appear as a separate primary transcript row,
including while it is pending or running. If the terminal `write_stdin` result
has a null `session_id`, the TUI uses the call arguments to finish the owning
`exec_command` row.
Assistant-message transcript order follows message content order, including
reasoning before later assistant text. Skill activation status such as
`skill loaded: ...` is a typed display-only status row emitted by Gateway when
specified; it is not a raw-stream-only side effect and not ordinary message
history.
Completed assistant answers keep their turn metadata row. In the typed Gateway
path, the TUI reads usage, provider/model, elapsed metadata, accounting, and
terminal-answer eligibility from the assistant transcript block, defers the row
while tools or the foreground turn are still active, and appends it after the
turn is fully complete.
Typed Gateway transcript rows are reconciled by transcript entry/block id while
a turn is live. If Gateway completes a provisional assistant text block as a
Thinking block with `metadata.projection = "assistant_preamble"` and the same
id, the TUI converts the existing row in place; it must not keep both an
Answer row and a Thinking row for the same block. `assistant_preamble` is an
internal projection marker, so the TUI renders it as ordinary Thinking content
and must not expose a `Preamble` label. Non-tool final assistant answers render
only as Answer rows and must not be copied into Thinking rows.
Long Thinking and tool evidence bodies share the same logical-line middle
folding and row interaction: head/tail preview, full body, and title-only
states cycle consistently across live streaming and history reload.
When a turn completes with committed transcript entries as defined by
[213 pevo Display Model](../213-pevo-display-model/spec.md), the TUI treats all
same-turn `runtime.stream` rows and the locally echoed prompt as live overlay:
it removes them before applying the committed entries. Live turn meta/footer
rows belong to that overlay as display-only answer/thinking footers; they are
not durable transcript facts and must not remain as unowned `Meta` rows after
committed replacement. If the committed slice includes message sequences
already present from history reload, the TUI skips those entries instead of
duplicating old transcript rows.
Completed message-derived assistant entries may render a quiet committed footer
under the owning answer row, but that footer must carry the same entry identity
and source lineage as the committed assistant block so it is not removed or
reused by the next turn's live-overlay reconciliation.
After a committed footer is appended, the transient live turn metadata state is
consumed; later foreground task or turn completion cleanup must not use stale
failure, usage, timing, provider, or accounting state to synthesize a second
`Meta` row for the same answer.
History reload must preserve assistant message content order when rebuilding
transcripts from message-derived entries: reasoning, assistant pre-tool text,
tool rows, later reasoning, and final answers appear in their original
`session_seq` plus content-index order.
An empty reasoning-completed event only closes an existing Thinking row. It must
not create a title-only Thinking row, and completed history Thinking rows must
not keep live elapsed timers.
Transcript rendering consumes semantic transcript entries/blocks and reusable
renderable components as defined by
[213 pevo Display Model](../213-pevo-display-model/spec.md). The TUI must not
persist viewport-wrapped terminal lines as durable display state.
Raw runtime/debug observations are not ordinary transcript rows. They may be
shown only when a debug/raw surface explicitly requests bounded debug records.

Live fullscreen turns consume in-process Gateway typed transcript events, not
raw runtime stream events. Gateway entry lifecycle events are the live rendering
source while a turn is running. TUI may keep small local presentation state for
folding, elapsed timers, selection, and pending panels, but that state must be
derived from Gateway transcript entries or the shared history projection.

History reload and session switching use the same shared transcript projection
as Gateway/thread snapshots. The TUI must not maintain a second durable display
model or prefer any runtime sidecar for ordinary transcript rows.

The fullscreen composer height is derived from the rendered editable input
rows. Empty input uses one visible row. Non-empty input grows from explicit
logical lines and terminal soft-wrapped rows, and remains capped at six visible
rows before the textarea scrolls internally.

Session observability in the bottom status line is compact and must degrade
without overlap at narrow widths. Context usage is the first observability
segment because it describes the immediate context-window risk. Cache-read
percent, session token total, and estimated cost may follow when enough width is
available. If the full sequence does not fit, the renderer drops later
observability segments before dropping the context usage segment.

The `/usage` panel may render richer current-session usage details above
workdir totals. These details are metric rows derived from persisted
accounting, not transcript entries, and must not display prompt bodies,
message text, tool arguments, or provider request payloads.

## Scope

- alternate-screen lifecycle and fullscreen viewport layout
- transcript ledger blocks for prompts, Thinking, tool evidence, Agent rows,
  assistant answers, turn metadata, and local attachment metadata
- active and completed evidence-row presentation, expansion, folding,
  elapsed labels, and shared activity motion
- typed Gateway transcript entries into live ledger rows and shared transcript
  history entries into history rows
- fixed composer/status-line rendering, including context usage, path/branch
  display, session usage/cache/cost observability, running elapsed projection,
  and child-session status-line behavior
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
- [Evidence Projection](evidence-projection.md) defines transcript/event to
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
