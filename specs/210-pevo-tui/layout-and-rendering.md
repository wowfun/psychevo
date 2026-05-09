---
name: 210. pevo TUI
psychevo_self_edit: deny
---

# 210. pevo TUI Layout and Rendering

Define the fullscreen ledger layout, evidence projection, rendering rules, and visual diagnostic expectations.

## Layout

Interactive terminals use raw mode and the alternate screen.

The first fullscreen layout is an evidence ledger, not a row-level event log.

The main transcript area is scrollable and renders each turn as a structured
ledger block:

- a dark unlabeled prompt block for the submitted user prompt, with no left
  rail or role label, and with the same full-width `RGB(38,38,38)` surface used
  by the bottom composer. Prompt text is wrapped before rendering so every
  visible physical row, including continuation rows and CJK/wide-character
  rows, carries the same full-width background instead of relying on paragraph
  wrapping to preserve row styling.
- interleaved folded thinking, tool evidence, and assistant answer material
- folded thinking rendered inline as `Thinking: <reasoning>` rather than a
  standalone `Thinking` label row; only the `Thinking:` prefix uses the warm,
  paper-like subdued color role, while reasoning content uses the normal
  thinking body color, and explicit new paragraphs in reasoning content do not
  receive label-width indentation
- tool evidence renders in a compact tool-evidence form: a bullet/title row
  followed by indented body output, with no vertical left rail
- the final assistant answer as unlabeled body text with no left rail or role
  label
- turn metadata directly after a visible answer with its compact left rail preserved:
  provider/model with the resolved variant one space to its right only when
  present, elapsed time, failures only when present, debug details only when
  enabled, and non-default mode last

When one submitted prompt produces multiple assistant messages separated by
tool calls, each assistant message with visible text remains in the transcript
as its own answer block. Streaming updates may replace only the currently
active assistant message; `message_end` freezes that block so later model
responses in the same foreground turn append new answer blocks instead of
overwriting earlier visible text.

Assistant messages that contain only folded reasoning and/or tool calls do not
render turn metadata. Tool-only Thinking blocks must remain compact evidence
and must not be followed by provider/model/elapsed metadata unless a visible
answer or failure summary requires it.

The bottom area contains a compact composer with the same full-width
`RGB(38,38,38)` input surface used by historical user prompts, a leading dim
`›` prompt marker, and one compact state line. It must not use a left accent
rail or a full bright border around the composer as the primary visual
treatment. Recalled history and restored drafts use the same composer styling
as fresh typed input and must not re-enable the textarea default cursor-line
underline. An empty composer defaults to two visible input rows; non-empty input
grows with its wrapped/logical line count up to six visible rows.

The composer must not show the current mode in its border/title. The state line
under the composer shows only the model name, variant value, and non-default
mode, without `mode=`, `model=`, or `variant=` prefixes. Non-default modes are
shown after the model and variant so model/variant positions stay stable.
`default` is omitted. Shortcut hints, session ids, thinking state, debug state,
and brand text are not part of the default bottom chrome.

While foreground work is running, the same state line appends a compact running
projection to the right of the stable model/variant/mode segment. It shows an
animated spinner frame, elapsed seconds, and `Esc`, for example
`xiaomi/mimo-v2.5-pro low  ⠋ 12s · Esc`. After the user requests interruption,
the appended projection changes to `⠋ interrupting 12s` until the turn settles.
This is the only bottom shortcut hint in the first slice. The TUI must not add a
separate `Working` label, a multi-line status widget, or a transcript row merely
because interruption was requested. Existing shell-mode marking remains in the
same state line and does not move the model or variant text.

The right sidebar is optional local context. It is hidden by default for fresh
state, including on wide terminals, and may be toggled explicitly. Fullscreen
`Ctrl+B` toggles persist `sidebar_visible` so later TUI startups restore the
last explicit open or closed state when terminal width can fit the sidebar. It
must not be required for the main transcript/composer workflow.

The sidebar is local-only. It may show the current session title, short
session id, workdir, git branch, message/tool counts, token/context usage, and
changed files. It must not call live provider catalogs or probe provider APIs.
It must not show source, mode, model, variant, or thinking visibility.

TUI user-facing `messages` counts are visible-message counts: user prompt
blocks with text plus assistant answer blocks with visible text. They exclude
thinking, metadata, tool evidence, tool-result records, and assistant
reasoning-only or tool-call-only records. Runtime and SQLite session
`message_count` retain their internal persisted-record semantics.

The sidebar starts with the current session title in bold. When no title is
known, it falls back to the short session id; when no session exists, it shows
`New session`. Sidebar sections use bold headings without colored left rails.
Sidebar content uses restrained default/dim text unless color carries essential
state.

The sidebar sections are:

- Context
- Modified Files

The Context section shows token usage and context percentage when usage and a
known model context limit are available. Token usage and context percentage are
sidebar context, not transcript metadata.

Modified Files prefers session-local diff evidence when available. In the first
slice, it may fall back to local git status. It shows at most 10 tail-compacted
paths with compact `+/-` statistics when those statistics are known.

Long local paths in the sidebar should be tail-compacted to preserve the
rightmost useful path segments and avoid multi-line path walls.

## Evidence Projection

TUI renders runtime events into semantic ledger evidence:

- user prompts become unlabeled dark prompt blocks without a left rail
- folded reasoning becomes inline `Thinking: <reasoning>` evidence; explicit
  new paragraphs in reasoning content start without label-width indentation
- `read`, `list`, and `search` tool calls become `Explored` evidence
- `bash` tool calls become `Ran <first command line>` evidence; the title must
  expose the actual first command line from the tool arguments rather than a
  generic `command` placeholder whenever the runtime supplied it, and completed
  tool updates must preserve the command title captured from the start event
  when the end event only contains the result
- `write` and `edit` tool calls become `Changed` evidence
- before a tool completes, fullscreen TUI may project transient active evidence
  from streaming assistant tool-call blocks and tool-execution start events:
  `read`/`list`/`search` render as `Exploring`, `bash` as `Running`, and
  `write`/`edit` as `Changing`; completion converts the same row back to the
  completed `Explored`/`Ran`/`Changed` title
- assistant visible output becomes unlabeled answer body text without a left
  rail
- turn-level metadata becomes unlabeled material directly after a visible answer
  and keeps the metadata left rail

Tool failures remain in their original evidence group and render as failures
instead of being moved into a separate generic error log.
When the overall turn outcome is `normal`, tool failures are summarized by the
failed tool row and turn metadata, not by an additional red `Error` transcript
row. A red turn-ended error row is reserved for non-normal turn outcomes, so
the TUI must not render contradictory messages such as `turn ended: normal`.

Active tool evidence is local TUI projection only. When a streaming assistant
message exposes a tool call before execution starts, the transcript shows a
short `preparing` body with a generic title if arguments are not yet complete,
for example `Changing files`, `Exploring`, or `Running command`. Once complete
arguments are available, or once the corresponding `tool_execution_start`
arrives, the active title should update to the concrete path/query/command
without inserting a duplicate row. Active tool rows match primarily by
`tool_call_id`; if the id has not arrived yet, fullscreen TUI uses the
assistant tool-call `content_index:call_index` pair scoped to the current
assistant message as a temporary key and migrates it when the id appears. The
same `content_index:call_index` pair may recur in later assistant messages in a
multi-tool turn and must not overwrite prior tool evidence. Pending active rows
that never reach execution because the turn is interrupted or fails stop their
timer and render as failed `interrupted` evidence rather than being persisted as
completed history.

Tool evidence shows elapsed execution duration on the right side of the tool
title row. Running tools refresh that value from the local start instant while
the turn is live; completed tools use the runtime-supplied `elapsed_ms` and must
not continue increasing on later redraws. TUI history reload restores completed
tool duration from the tool-result message metadata when available. Narrow
views preserve the right-side duration first and truncate the title when needed.
Transcript layout caching must not freeze active tool evidence: rows with a
running local start instant must invalidate cached rendering when their
right-side elapsed label or activity marker changes, while completed rows remain
cache-stable.

Long tool outputs default to a maximum of 20 visible lines. Expandable evidence
keeps the full stored output available for local inspection in this TUI process
or from persisted message/tool-result material when available.

Usage and provider metadata are not transcript content blocks. Provider/model
with an optional resolved variant, elapsed time, failures, debug usage parts,
and allowlisted provider metadata may be projected into turn metadata, but
total token usage and context percentage are projected to the sidebar. Usage
and provider metadata must not appear in sanitized transcript messages,
provider replay across incompatible providers, or `pevo run --format json` by
default.

Default metadata projection omits `default` mode and renders elapsed time with
the UI-only compact duration formatter: under 60 seconds as whole seconds
without decimals, for example `12s`, and 60 seconds or more as minutes plus
zero-padded seconds, for example `1m05s` or `2m20s`. The formatter floors
sub-second precision, so `999ms` renders as `0s`. Completed model messages use
the runtime-supplied `elapsed_ms` captured at message completion when
available; fullscreen TUI must not recompute completed elapsed time from later
render or event-drain time, and storage retains millisecond precision.
When runtime resolves an enabled per-turn `reasoning_effort`, assistant message
metadata preserves it as `reasoning_effort`, and TUI renders that value
directly after the model label separated by one space. Missing reasoning effort
and the `none` variant are omitted because they do not produce a provider
request field. Non-default mode is the final metadata item.
Fullscreen TUI history reload restores persisted elapsed time when available
instead of showing only provider/model and response metadata for completed
turns.
Debug projection shows usage parts and an allowlisted provider metadata summary
without `key=value` prefixes and without duplicating `elapsed_ms` or
`reasoning_effort`.

## Rendering

The TUI uses a compact terminal palette:

- default foreground for primary text
- dim secondary text
- cyan hints and status markers
- green success markers
- red failures
- magenta `pevo` identity

Assistant visible text streams inline inside the current turn. Thinking is
visible by default, rendered as folded/debug material under a `Thinking`
evidence block, not as assistant transcript text. When `/show-thinking` is off, TUI
hides the entire Thinking evidence block. Thinking display is local UI material
only; it is not promoted into visible transcript projection, JSON run output,
provider replay across providers, session-list output, or rendered `agent_end`
material.

TUI should create an answer row only after visible assistant text exists. It
must not pre-render an empty answer row that pushes thinking or tool evidence
out of the first visible ledger projection.

While a turn is running, fullscreen TUI auto-follows the transcript when the
viewport is already at the bottom. Assistant streaming deltas, long generated
answers, tool starts, and tool-result updates must be visible on the next draw
without requiring manual scrolling. Manual transcript scrolling opts out of
auto-follow until the user returns to the bottom or a new prompt is submitted.
Transcript scroll math must use the actual content viewport, excluding any
decorative transcript border rows, so the newest content is not hidden behind
the composer or bottom state line. Scroll limits, row hit areas, prompt
background rows, and paragraph rendering must share the same rendered-line
metric, including Ratatui word wrapping for long mixed Markdown/CJK lines,
instead of estimating wrapped height from raw display width. Repeated pure
scrolling must reuse cached rendered row heights when the transcript content,
width, visibility, and row selection have not changed, and rendering should
only materialize the transcript rows intersecting the current viewport rather
than re-wrapping the full transcript from the top on every wheel tick. Mouse
input must not force one full transcript redraw per raw terminal mouse event;
bursty wheel and drag events should be coalesced into bounded redraws, and the
terminal mouse mode should avoid all-motion tracking unless a future feature
requires it. Transcript text selection must remain responsive: dragging should
only update selection state and redraw at the normal frame cadence, and copying
selected text must not synchronously block the input loop on platform clipboard
commands. Requests
to scroll to the bottom during history loading or session switching must be
resolved after the next render has the real transcript viewport dimensions.
Redraws after scrolling or appending shorter rows must not leave stale glyphs
from previous longer rows.

The fullscreen transcript must not include a synthetic startup status row. The
first visible content is existing session history when present, otherwise an
empty transcript above the composer.

After `/new` clears the transcript for a pending new session, the next
fullscreen repaint must show a clean empty transcript area above the composer.
It must not leave stale glyphs, partial title text, or any other remnants from
the previous session or status rows.

Successful turn completion and mode changes must not add synthetic `Ok` or
`Status mode` rows to the transcript. The bottom state line is the source of
truth for the current mode.

Tool starts and ends render as compact evidence blocks. Long tool result bodies
are summarized rather than dumped unless the block is expanded.

User shell escapes render through the same compact `Ran <first command line>`
evidence used for shell tool evidence, with failures remaining in that evidence
group. User shell result evidence is local TUI material and must not become
provider replay context or a persisted user prompt.

When the fullscreen composer contains a user shell escape, the bottom state
line includes a local shell-mode marker while preserving the normal model,
variant, and mode display footprint.

When a background turn task completes, TUI must drain all queued runtime stream
events before rendering the turn as complete. Final ledger projection must not
lose late tool or message evidence merely because the task finished between
input polling ticks.

Fullscreen input submitted while an agent turn or user shell command is active
is queued in submission order. Queued prompts start after active work settles.
Queued user shell escapes run after earlier queued work; bare queued `!` shows
the bounded help text and then drains the next queued item.

Session picker and scripted session-list output must not expose folded
reasoning blocks or provider reasoning wire fields. Folded reasoning blocks
and provider reasoning wire fields must also not appear in rendered
`agent_end` material.

For non-terminal stdin/stdout, `pevo tui` keeps deterministic line-by-line
behavior and renders plain, no-ANSI semantic blocks: `Prompt`, `Thinking`,
`Explored`, `Ran`, `Changed`, `Answer`, and `Meta`. Lines beginning with `!`
after leading whitespace run as user shell escapes and do not require provider
credentials. The plain projection keeps block labels for machine-readable
diagnostics even where fullscreen TUI uses unlabeled prompt and metadata
presentation. `--debug` also affects this plain projection.

## Visual Regression and Diagnostics

The default checked-in visual regression source is an in-process `ratatui`
buffer projection with stable text and style-role markers. It is a regression
golden for layout, emphasis, and color-role discipline, not raw ANSI output or
a screenshot.

Style-role projections normalize terminal styling into the roles used by this
spec: default primary text, dim secondary text, cyan status/hints, green
success, red failures, and magenta `pevo` identity. They must avoid timestamps,
random session ids, real provider text, real git state, real user config, and
other host-volatile material.

Real terminal PNG screenshots are required review artifacts for fullscreen TUI
visual display changes. They are generated from a deterministic local
mock-provider demo through VHS, but they are not checked-in goldens and are not
compared pixel-by-pixel in default validation. A person or visually capable
agent reviews them. The artifact root is
`.local/.psychevo-dev/tui-shots/<timestamp>/`. The deterministic demo should
isolate git state from the parent repository and pin terminal color inputs,
including clearing inherited `NO_COLOR`, so screenshots are useful as visual
diagnostics.

## Related Topics

- [210 pevo TUI](spec.md) is the parent topic.
- [210 pevo TUI Testing](testing.md) defines deterministic acceptance coverage.
