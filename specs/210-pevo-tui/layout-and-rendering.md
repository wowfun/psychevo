---
name: 210. pevo TUI
psychevo_self_edit: deny
---

# 210. pevo TUI Layout and Rendering

Define the fullscreen ledger layout, evidence projection, rendering rules, and visual diagnostic expectations.

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
  marker. Thinking body lines use compact tool-style indentation (`└` then
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
so the current `Exploring`/`Running`/`Changing` state remains visible at the
bottom of the ledger.
Assistant messages whose `finish_reason` is `tool_calls` are intermediate
ledger material, even when they contain visible text. They must not render
turn metadata until a final visible answer, terminal reasoning-only message, or
tool failure summary requires it.
Turn metadata must not render while any active `Exploring`/`Running`/`Changing`
tool row is still live. If an earlier failure summary meta row exists and a new
active tool row appears, fullscreen TUI removes that meta row and lets the final
answer, terminal reasoning-only message, or later failure summary recreate
metadata after active evidence settles.
Turn metadata also must not remain below a currently streaming `Thinking` or
visible assistant block. If a prior tool failure created interim turn metadata
and the provider continues with reasoning or answer text, fullscreen TUI removes
that metadata until the assistant message reaches a terminal normal answer,
terminal normal reasoning-only result, or another failure summary state.

Assistant messages that contain only folded reasoning and tool calls do not
render turn metadata. Tool-only Thinking sections must remain compact evidence.
If an assistant message ends the turn normally with folded reasoning but no
visible text, fullscreen TUI may restore turn metadata after that final
Thinking block so history resume still exposes provider/model/elapsed context.
Aborted or interrupted reasoning-only messages are not terminal reports and
must not create a metadata block below `Thinking`.

The bottom area contains a compact composer with the same full-width adaptive
input surface used by historical user prompts, a leading dim `›` prompt marker,
and one compact state line. It must not use a left accent rail or a full bright
border around the composer as the primary visual treatment. Recalled history and
restored drafts use the same composer styling as fresh typed input and must not
re-enable the textarea default cursor-line underline. An empty composer defaults
to two visible input rows; non-empty input grows with its wrapped/logical line
count up to six visible rows.

The composer must not show the current mode in its border/title. The state line
under the composer shows the model name, variant value, and non-default mode,
without `mode=`, `model=`, or `variant=` prefixes. Non-default modes are shown
after the model and variant so model/variant positions stay stable. `default`
is omitted. Shortcut hints, session ids, thinking state, debug state, and brand
text are not part of the default bottom chrome.

The same state line appends local context after the stable model/variant
segment, in this order when available: workdir, git branch, and compact context
usage. These items are separated by ` · ` and render without keys. Workdir uses
the TUI path display convention: a `$HOME` prefix is shown as `~`, home itself
is `~`, non-home paths remain absolute, and long paths are center-truncated
with `…` using display width. Branch is omitted when no branch is detected.
Context usage is omitted until a latest `ContextSnapshot` or latest provider
input usage exists and its context limit is known; when shown, it uses the same
formatter as the value after `tokens: ` in `/context`, for example
`39.2k/1.0M (3.7%)`. Running turns may refresh this value from streamed
context snapshots, provider input usage metadata, or an explicit `/context`
session estimate.

On narrow terminals, the stable model/variant/mode/running segment takes
priority. The optional local context segment hides branch first, then truncates
workdir, and hides context usage only after those reductions cannot fit.

While foreground work is running, the same state line appends a compact running
projection to the right of the stable model/variant/mode segment. It shows an
animated spinner frame, elapsed seconds, and `Esc`, for example
`xiaomi/mimo-v2.5-pro low  ⠋ 12s · Esc`. After the user requests interruption,
the appended projection changes to `⠋ interrupting 12s` until the turn settles.
This is the only bottom shortcut hint in the first slice. The TUI must not add a
separate `Working` label, active phase text such as `Running`, a multi-line
status widget, or a transcript row merely because interruption was requested.
Existing shell-mode marking remains in the same state line and does not move
the model or variant text. Active phase names belong in ledger tool rows only.

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

Context-window usage belongs in the bottom state line and `/context`. Token and
cost summaries belong in `/usage` and its `/stats` alias. Unknown pricing is
omitted from dollar totals and may be summarized as unknown-priced messages in
usage views. Cost display is local estimation only and must not imply provider
billing reconciliation.

Modified Files prefers session-local diff evidence when available. In the first
slice, it may fall back to local git status. It shows at most 10 tail-compacted
paths with compact `+/-` statistics when those statistics are known.

Long local paths in the sidebar should be tail-compacted to preserve the
rightmost useful path segments and avoid multi-line path walls.

## Evidence Projection

TUI renders runtime events into semantic ledger evidence:

- user prompts become unlabeled dark prompt blocks without a left rail
- folded reasoning becomes flat `Thinking` evidence; explicit new paragraphs in
  reasoning content start without label-width indentation
- `read`, `list`, and `search` tool calls become `Explored` evidence
- `bash` tool calls become `Ran <first command line>` evidence; the title must
  expose the first actual shell command from the tool arguments rather than a
  generic `command` placeholder whenever the runtime supplied it. Leading blank
  lines and full-line shell comments, including model-written planning comments
  such as `# Try webcache`, are skipped for title selection so the ledger shows
  the executable command line. Completed tool updates must preserve the command
  title captured from the start event when the end event only contains the
  result.
- `write` and `edit` tool calls become `Changed` evidence
- before a tool completes, fullscreen TUI may project transient active evidence
  from streaming assistant tool-call blocks, runtime pending tool-call input
  events, and tool-execution start events:
  `read`/`list`/`search` render as `Exploring`, `bash` as `Running`, and
  `write`/`edit` as `Changing`; completion converts the same row back to the
  completed `Explored`/`Ran`/`Changed` title. Active tool rows must keep the
  present-tense display while their local timer is live, even if an intermediate
  title value was restored as completed-tense text. They should not show a
  redundant body line that says only `running` or `preparing`; the active title,
  spinner/activity marker, and right-side elapsed duration carry that state.
- assistant visible output becomes unlabeled answer body text without a left
  rail
- turn-level metadata becomes unlabeled material directly after a visible
  answer, or after a terminal reasoning-only assistant message when no visible
  answer exists, and keeps the metadata left rail

Tool failures remain in their original evidence group and render as failures
instead of being moved into a separate generic error log. Interrupted tool
evidence is distinct from ordinary failure evidence: a tool result with
`outcome: "aborted"` or `error: "aborted"` renders a muted `interrupted` marker
in the existing `Explored`/`Ran`/`Changed` row rather than a red failure body
or `(no output)`. `bash` timeout failures must render an explicit timeout
line in the failed `Ran` row even when the command produced partial output.
When the overall turn outcome is `normal`, tool failures are summarized by the
failed tool row and turn metadata, not by an additional red `Error` transcript
row. A red turn-ended error row is reserved for non-normal turn outcomes, so
the TUI must not render contradictory messages such as `turn ended: normal`.
User-confirmed interrupted turns show `interrupted` in turn metadata instead of
counting the interrupted tool as `1 failure`.

Active tool evidence is local TUI projection only. Runtime must surface a
named pending tool-call input event as soon as a provider streams the tool name,
before waiting for complete JSON arguments or local execution. While the model
is still producing tool input, the transcript shows a short `preparing` body
with a generic title if arguments are not yet complete, for example
`Changing files`, `Exploring`, or `Running command`. Once complete arguments
are available, or once the corresponding `tool_execution_start` arrives, the
active title should update to the concrete path/query/command without inserting
a duplicate row. Active tool
rows match primarily by `tool_call_id`; if the id has not arrived yet,
fullscreen TUI uses the assistant tool-call `content_index:call_index` pair
scoped to the current assistant message as a temporary key and migrates it when
the id appears. The same `content_index:call_index` pair may recur in later
assistant messages in a multi-tool turn and must not overwrite prior tool
evidence. Pending active rows that never reach execution because the turn is
interrupted or fails stop their timer and render as static `interrupted`
evidence rather than being persisted as completed history.

When Thinking is visible, fullscreen TUI may show a provisional active tool row
from visible Thinking text that explicitly announces imminent tool use, such as
`Let me write...` or `Let me run...`, because some providers stream long
tool-input generation as reasoning before emitting the structured tool-call
block. This provisional row uses a generic title such as `Changing files` or
`Running command`, never a guessed path or command. Hidden thinking must not
create a provisional row from reasoning text. A concrete assistant tool-call
block, runtime pending tool-call input event, or runtime `tool_execution_start`
must replace the provisional row with the real active row when it arrives; if an
assistant message finishes without a matching tool call, the provisional row is
removed. Once concrete signal arrives, the active `Exploring`, `Running`, or
`Changing` row must be rendered for at least one frame before later same-turn
events can convert it to completed `Explored`, `Ran`, or `Changed` evidence.
This applies even when local tool execution completes in the same event-drain
tick, such as a 0ms `write` after a long provider-side tool-argument generation
phase.

Transcript folding is row-level only. The renderer must not synthesize
`Thinking` or `Tool calls (N)` section headers. `Thinking` rows and tool
evidence rows (`Explored`/`Ran`/`Changed`, including active
`Exploring`/`Running`/`Changing` display) are rendered through the same ledger
evidence row component and are individually foldable when they have rendered
detail text. Short rows default open; selecting an open foldable row shows
`▾ collapse`, and collapsed short rows show `▸ details`. Long `Ran`/`Running`
command titles stay single-line with ellipsis in the title row so elapsed time
remains visible, but rows with long command titles can expand to show the
complete wrapped command below the title. Long Thinking bodies and long tool
outputs use the same default collapse threshold: eight logical lines or roughly
1200 display characters. Line-count collapses show `▸ N more lines`; width-only
collapses whose omitted line count is not meaningful show `▸ more output`.

Mouse clicks on expandable rows toggle that row's details. Dragging to select
transcript text must not toggle rows. Transcript-focus `Enter` and `Space`
apply the same toggle to the selected row. Transcript focus selection is a
single-line focus affordance: only the selected row's first visible line uses
the `›` marker, while body and wrapped continuation lines retain their normal
ledger indentation. Mouse text selection remains a separate copy-selection
state and continues to use the semantic selection background. Collapsed rows
contribute their actual rendered height to the transcript scroll model, and
selection movement must walk visible rows.
When fullscreen TUI reconstructs transcript history from persisted messages,
assistant messages whose `finish_reason` is `tool_calls` and whose outcome is
still `normal` must also rehydrate their unmatched tool-call blocks as active
ledger evidence until the matching `tool_result` record is encountered. The
later `tool_result` updates that same row in place rather than appending a
duplicate, so reconnecting or reloading a running session still shows
`Changing <path>` during provider-side or local write gaps. If a persisted
assistant message is already terminally interrupted (`finish_reason=aborted` or
an `aborted`/`failed`/`stopped` outcome), unmatched tool calls from that message
render as static muted `interrupted` evidence with no live timer. History
reload must never turn those aborted tool calls back into active
`Exploring`/`Running`/`Changing` rows.
For providers that buffer tool-call input until the end of a long write
argument generation, fullscreen TUI may show a provisional `Changing files`
row from visible assistant preamble text only when that visible text explicitly
announces an imminent write/change action. This fallback is not allowed for
folded reasoning text, must be replaced by the real tool row when a concrete
tool signal arrives, and must be removed if the assistant message finishes
without a write/edit tool call. Repeated message updates for the same visible
preamble must not create additional provisional `Changing files` rows once a
concrete active write/edit row exists. Completion must leave exactly one
completed `Changed` row for the tool call and no orphan active fallback rows.

Tool evidence shows elapsed execution duration on the right side of the tool
title row. Active Thinking rows also show a right-side elapsed value while
reasoning is streaming, but completed Thinking rows do not synthesize a
duration from turn metadata. Running tools refresh elapsed from the local start
instant while the turn is live; completed live rows freeze the larger of the
runtime-supplied `elapsed_ms` and the active ledger duration since the first
concrete `Exploring`/`Running`/`Changing` signal, so a provider-side pending
period does not collapse to `0s` when local execution is instant. Completed
rows must not continue increasing on later redraws. TUI history reload restores
completed tool duration from the tool-result message metadata when available
and does not recompute old completed rows from the current wall clock. Narrow
views preserve the right-side duration first and truncate the title when
needed.
Transcript layout caching must not freeze active tool evidence: rows with a
running local start instant must invalidate cached rendering when their
right-side elapsed label or activity marker changes, while completed rows remain
cache-stable.

Expandable evidence keeps the full stored output available for local inspection
in this TUI process or from persisted message/tool-result material when
available. Expandable title rows use a right-side text affordance instead of
bracket tokens: collapsed rows show `▸ N more lines` when the omitted line
count is known, width-only collapses show `▸ more output`, and expanded rows
show `▾ collapse`. Narrow terminals may shorten those hints, but must not
reintroduce bare `[+]` or `[-]`.

Usage and provider metadata are not transcript content blocks. Provider/model
with an optional resolved variant, elapsed time, failures, debug usage parts,
and allowlisted provider metadata may be projected into turn metadata, but cost
belongs in `/usage` and its `/stats` alias, while context percentage belongs in
`/context` and the bottom state line. Usage and provider metadata must not
appear in sanitized transcript messages, provider replay across incompatible
providers, or `pevo run --format json` by default.

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

The TUI uses a compact terminal-adaptive palette:

- default foreground for primary text
- dim secondary text
- cyan hints and status markers
- green success markers
- red failures
- magenta `pevo` identity

Fullscreen rendering may query the terminal default foreground/background under
a bounded startup deadline. When the terminal reports a usable background, TUI
surfaces derive subtle prompt/composer/menu/selection backgrounds from that
default so dark and light terminal themes remain readable. When the query is
unsupported, times out, or runs outside an interactive terminal, the renderer
falls back to the existing dark surface palette, including `RGB(38,38,38)` for
prompt and composer rows. Terminal probing is best-effort and must never block
startup beyond the bounded deadline or fail the TUI.

Prompt blocks and the composer must still share the same full-width surface
within a render profile. Popup, bottom-panel, and selected-row surfaces use the
same semantic theme roles rather than hardcoded local colors, while preserving
the compact no-left-rail ledger treatment.

Assistant visible answer text uses lightweight Markdown rendering for local
display only. Supported styling includes headings, lists, emphasis, inline
code, fenced code blocks, tables, links, and local file links. Markdown tables
render as box tables when the available width can fit them; narrow terminals
fall back to readable pipe-table text. Fenced `md` or `markdown` code blocks
that contain only table-like Markdown may be smart-unpacked and rendered as
tables instead of as code blocks. Other fenced code blocks keep clear top and
bottom boundaries, use the existing long-content folding thresholds, and apply
lightweight semantic syntax highlighting. Link display exposes destinations:
normal links show their URL, while local file links show a path relative to the
session working directory where possible. This rendering is a TUI projection
only and must not change persisted message content, provider replay, `/copy`,
or non-terminal JSON output.

When global raw transcript visibility is enabled, fullscreen TUI keeps the same
ledger outer structure but renders assistant answer bodies, and visible
Thinking bodies, as raw Markdown source. Raw mode is display-only and must not
change persisted message content, provider replay, `/copy`, non-terminal JSON
output, row identity, or tool evidence rendering.

Activity indicators and shimmer-like running text must flow through a shared
motion primitive with a deterministic static fallback for tests and reduced
motion contexts. Time-varying motion must be included in transcript layout cache
keys only when it changes rendered width or visible content.

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
requires it. Wheel scrolling uses the last rendered hover region instead of
keyboard focus: transcript hover scrolls the transcript, active bottom-pane
hover scrolls the pane, and composer/status hover is ignored rather than being
interpreted as composer `Up`/`Down` history navigation. If an outer terminal
synthesizes wheel input as plain cursor keys, the event is handled exactly like
real keyboard input because the app no longer has pointer coordinates or event
origin metadata. Transcript text selection must remain responsive: dragging
should only update selection state and redraw at the normal frame cadence, and
copying selected text must not synchronously block the input loop on platform
clipboard commands. Requests
to scroll to the bottom during history loading or session switching must be
resolved after the next render has the real transcript viewport dimensions.
Scroll boundaries must use the same cached rendered-row total that the viewport
renderer uses, rather than falling back to a separately estimated transcript
line count once a layout has been computed. The cache is valid only when row
content, visibility, selection, expansion state, active elapsed labels, and
active motion markers still match the transcript. In transcript focus, moving
the selected ledger row with `Up`/`Down` must scroll the selected row into view.
When focus is in the composer and no popup or pane is active, `Up`/`Down` remain
input/history boundary keys; transcript scrolling uses PageUp/PageDown,
transcript focus movement, or real mouse-wheel hover events.
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
diagnostics. The VHS demo must include a long Markdown/table answer with
turn metadata, manually scroll away from and back to the bottom, and capture a
bottom-of-transcript screenshot where the final answer marker and metadata are
visible.

## Related Topics

- [210 pevo TUI](spec.md) is the parent topic.
- [210 pevo TUI Testing](testing.md) defines deterministic acceptance coverage.
