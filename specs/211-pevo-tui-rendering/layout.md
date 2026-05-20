---
name: 211. pevo TUI Rendering Layout
psychevo_self_edit: deny
---

# 211. pevo TUI Rendering Layout

Define the fullscreen layout, transcript/composer/status-line structure, and sidebar rendering rules.

## Layout

Interactive terminals use raw mode and the alternate screen. Fullscreen startup
enters a clean alternate screen, enables alternate-scroll mode, clears the
alternate buffer, and homes the cursor before the first draw so host scrollback
cannot bleed into the fullscreen view. Fullscreen shutdown restores cursor
visibility, raw mode, mouse capture, alternate-scroll mode, and the alternate
screen on normal exit, errors, and unwinds.

The first fullscreen layout is an evidence ledger, not a row-level event log.

The main transcript area is scrollable and renders each turn as a structured
ledger block:

- a dark unlabeled prompt block for the submitted user prompt, with no left
  rail or role label, and with the same full-width `RGB(38,38,38)` surface used
  by the bottom composer. Prompt text is wrapped before rendering so every
  visible physical row, including continuation rows and CJK/wide-character
  rows, carries the same full-width background instead of relying on paragraph
  wrapping to preserve row styling. If the submitted prompt contains image
  placeholders, the prompt block preserves the submitted composer text such as
  `[Image #1] describe this image`.
- local attachment metadata directly after a prompt with successfully submitted
  image attachments. It renders as compact `Meta` evidence headed
  `attachments` with one line per sent image, for example
  `image 1: screenshots/img.png`. Local paths prefer workdir-relative display
  and fall back to absolute paths; URL sources render as text. This metadata is
  local display evidence and is not provider input.
- interleaved folded thinking, tool evidence, and assistant answer material
- folded thinking renders as a flat expandable row, not as a vertical left-rail
  block or derived section. Active Thinking uses the same shared activity
  marker as active tool evidence; completed Thinking uses a stable bullet
  marker with the same evidence marker role as completed tool rows. Thinking
  titles use the ordinary evidence title style rather than a dedicated thinking
  color. Thinking body lines use compact tool-style indentation (`└` then
  continuation spaces), and explicit new paragraphs in reasoning content do not
  receive label-width indentation.
- consecutive tool evidence rows remain flat ledger rows. Individual tool
  evidence stays compact: a bullet/title row followed by indented body output,
  with no vertical left rail or `Tool calls (N)` section header.
- the final assistant answer as unlabeled body text with no left rail or role
  label
- turn metadata directly after a visible answer, or after a terminal
  reasoning-only assistant message when the provider returned no visible text,
  with its compact left rail preserved:
  provider/model with the resolved variant one space to its right only when
  present, elapsed time, failures only when present, debug details only when
  enabled, and non-default mode last. Turn metadata must not show per-turn
  cost; cost summaries belong in `/usage` and its `/stats` alias.

When one submitted prompt produces multiple assistant messages separated by
tool calls, each assistant message with visible text remains in the transcript
as its own answer block. Streaming updates may replace only the currently
active assistant message; `message_end` freezes that block so later model
responses in the same foreground turn append new answer blocks instead of
overwriting earlier visible text.
If a single assistant message streams visible text and then a tool call, the
active tool evidence is placed after that visible text and before turn metadata,
so the current tool-name-first active row remains visible at the bottom of the
ledger.
Assistant messages whose `finish_reason` is `tool_calls` are intermediate
ledger material, even when they contain visible text. They must not render
turn metadata until a final visible answer, terminal reasoning-only message, or
terminal tool failure summary requires it. A `tool_execution_end` event by
itself is not a terminal turn boundary and must not create a turn metadata row,
even when that tool failed and no active tool row remains.
Turn metadata must not render while any active tool row is still live. If an
earlier failure summary meta row exists and a new
active tool row appears, fullscreen TUI removes that meta row and lets the final
answer, terminal reasoning-only message, or terminal failure summary recreate
metadata after active evidence settles.
Turn metadata must also not render while the current foreground run still owns
the bottom status-line running marker, even if a terminal assistant
`message_end` has already arrived. This includes foreground work that has been
detached into visible-live auxiliary agent or shell tracking while inspecting a
parent or child session. Fullscreen TUI defers that metadata until the
foreground run is released at `agent_end`/turn-finish time so the ledger does
not show a completed provider/model summary under an active `Esc` interrupt
hint.
Turn metadata also must not remain below a currently streaming `Thinking` or
visible assistant block. If a prior tool failure created interim turn metadata
and the provider continues with reasoning or answer text, fullscreen TUI removes
that metadata until the assistant message reaches a terminal normal answer,
terminal normal reasoning-only result, or terminal failure summary state.

Assistant messages that contain only folded reasoning and tool calls do not
render turn metadata. Tool-only Thinking sections must remain compact evidence.
If an assistant message ends the turn normally with folded reasoning but no
visible text, fullscreen TUI may restore turn metadata after that final
Thinking block so history resume still exposes provider/model/elapsed context.
Aborted or interrupted reasoning-only message events are not terminal reports
by themselves and must not create a metadata block below `Thinking` before the
turn settles. When a user-confirmed interrupted reasoning-only turn does settle
and metadata is rendered, that metadata must include `interrupted`.

The bottom area contains a compact composer with the same full-width adaptive
input surface used by historical user prompts, a leading dim `›` prompt marker,
the `Ask pevo...` placeholder, and one compact status/hint line. It must not
use a left accent rail or a full bright border around the composer as the
primary visual treatment. Recalled history and restored drafts use the same
composer styling as fresh typed input and must not re-enable the textarea
default cursor-line underline. An empty composer defaults to one visible input
row; non-empty input grows with its wrapped/logical line count up to six visible
rows.

Active text selection in the composer, transcript, and sidebar uses one shared
high-contrast reverse-video plus bold style instead of relying on a color-only
selection background. This keeps selection visible on prompt blocks and other
full-width surfaces that already carry their own adaptive background.

When composer focus is active and no bottom panel owns text input, fullscreen
rendering must set the real terminal cursor to the editable textarea cursor
inside the composer input area on every draw. The position accounts for shell
mode prompt-marker width, empty input, CJK/wide-character display width,
wrapped screen cursor rows, and the textarea viewport. This cursor anchoring is
terminal-local display state for IME candidate windows; it is not persisted and
does not change transcript content. Editable bottom-panel inputs may own and
set the terminal cursor while focused. Non-editable transcript, sidebar,
popup-only, and read-only panel focus leaves the terminal cursor hidden.

The composer must not show the current mode in its border/title. The fixed
status line under the composer shows mode, model, and compact context usage by
priority, without `mode=`, `model=`, or `context=` prefixes. `default` may be
omitted when width is constrained. Shortcut hints, session ids, thinking state,
debug state, and brand text are not part of the stable bottom chrome, but
transient hints may temporarily replace lower-priority status text.

The same status line may append local context after the stable mode/model
segment, in this order when available: compact context usage, workdir, and git
branch. These items are separated by ` · ` and render without keys. Workdir
uses the TUI path display convention: a `$HOME` prefix is shown as `~`, home
itself is `~`, non-home paths remain absolute, and long paths are
center-truncated with `…` using display width. Branch is omitted when no branch
is detected. Context usage is omitted until a latest `ContextSnapshot` or
latest provider input usage exists and its context limit is known; when shown,
it uses the same formatter as the value after `tokens: ` in `/context`, for
example `39.2k/1.0M (3.7%)`. Running turns may refresh this value from streamed
context snapshots, provider input usage metadata, or an explicit `/context`
session estimate. Session startup, resume, and session switching must restore
the latest persisted provider input usage together with the session context
limit before the first draw so the bottom status line does not temporarily omit
context usage for resumed sessions.

On narrow terminals, mode/model/running state takes priority, then context
usage, then path and branch. Lower-priority status text is truncated or hidden
before the line wraps.

While foreground work is running, the same status line appends the shared
activity marker, compact elapsed feedback, and the `Esc` interruption hint after
the stable mode/model segment. The marker must be produced by the same
`activity_spinner_frame(elapsed)` path used by active Thinking/tool ledger rows,
including in tests. The elapsed value is the current visible session's user-turn
elapsed time: it starts when that session's user prompt appears in the
transcript or is restored from that prompt's persisted timestamp. It does not
reset when active Thinking/tool ledger rows start, finish, or change phase.
After the user requests interruption, the appended projection changes to
`interrupting <elapsed>` until the turn settles and does not show a spinner. The
TUI must not add a separate `Working` label, active phase text such as
`Running`, a multi-line status widget, or a transcript row merely because
interruption was requested. Existing shell-mode marking remains in the same
status line and does not move the model or variant text. Active phase names and
phase-level elapsed timers should not be added to the status line. When a running child
agent is opened for inspection and the parent foreground turn is detached into
auxiliary tracking, the inspected child session counts as running only while the
tracked child task matches the visible child session, and its status-line
elapsed uses that child session's own user prompt timestamp. Returning to the
parent/main session uses the parent visible user turn. Auxiliary agent tasks
that remain only to collect post-completion results after `agent_end` do not
count as visible-session live work and must not keep the bottom running marker,
elapsed timer, or `Esc` interrupt hint visible after the foreground turn has
settled.

## Sidebar

The right sidebar is optional local context. It is hidden by default for fresh
state, including on wide terminals, and may be toggled explicitly. Fullscreen
`Ctrl+B` toggles persist `sidebar_visible` so later TUI startups restore the
last explicit open or closed state when terminal width can fit the sidebar. It
must not be required for the main transcript/composer workflow.

The sidebar is local-only. It shows the current session title, short session
id, and changed files. It must not call live provider catalogs or probe
provider APIs. It must not show source, workdir, branch, mode, model, variant,
thinking visibility, message counts, tool-call counts, token/context usage, or
cost.

TUI user-facing `messages` counts are visible-message counts: user prompt
blocks with text, including image-placeholder-only prompts, plus assistant
answer blocks with visible text. They exclude attachment metadata, thinking,
metadata, tool evidence, tool-result records, and assistant
reasoning-only or tool-call-only records. Runtime and SQLite session
`message_count` retain their internal persisted-record semantics.

The sidebar starts with the current session title in bold. When no title is
known, it falls back to the short session id; when no session exists, it shows
`New session`. Sidebar sections use bold headings without colored left rails.
Sidebar content uses restrained default/dim text unless color carries essential
state.
The sidebar render pass must clear/fill its full rectangular area before
drawing current content. Shorter updates, wrapped path changes, or toggling
sections must not leave stale terminal glyphs in blank sidebar rows or in the
first cell of labels such as `tokens`.

The only sidebar section is:

- Modified Files

Context-window usage belongs in the bottom status line and `/context`. Token and
cost summaries belong in `/usage` and its `/stats` alias. Unknown pricing is
omitted from dollar totals and may be summarized as unknown-priced messages in
usage views. Cost display is local estimation only and must not imply provider
billing reconciliation.

Modified Files prefers session-local diff evidence when available. In the first
slice, it may fall back to local git status. It shows at most 10 tail-compacted
paths with compact `+/-` statistics when those statistics are known.

Long local paths in the sidebar should be tail-compacted to preserve the
rightmost useful path segments and avoid multi-line path walls.
