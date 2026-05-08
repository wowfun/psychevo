---
name: 210. pevo TUI
psychevo_self_edit: deny
---

Define the first interactive terminal surface for `pevo`.

This topic builds on [200 pevo CLI](../200-pevo-cli/spec.md) and routes live
coding-agent turns through `psychevo-runtime`. For interactive terminals,
`pevo tui` is a fullscreen terminal UI. For non-terminal stdin/stdout, it keeps
the deterministic line-by-line scripted behavior.

## Scope

- `pevo tui` command spelling and startup behavior
- fullscreen transcript, composer, and minimal bottom state line
- persisted TUI-local model, variant, mode, thinking visibility, and sidebar
  visibility
- session selection, session renaming, model, variant, mode, thinking visibility,
  and status slash commands
- evidence-ledger rendering for prompts, folded reasoning, tool evidence,
  final answers, and turn metadata
- transcript selection and keyboard expansion for bounded tool evidence
- debug projection for usage and provider metadata summaries
- deterministic visual-regression projections and local diagnostic screenshots
- hard `plan` / `default` runtime mode selection

Out of scope:

- panes, plugins, custom keymaps, or heavy markdown rendering
- approvals, auth, provider login, provider catalogs, or model probing
- file pickers, `@file` references, `!bash`, custom slash commands, or
  command-template files
- compaction, rollback, share/fork UI, external editor integration, or TUI
  theme configuration

## Command

`pevo tui [message..]` starts the interactive terminal surface for the selected
working directory.

Accepted first-slice flags are:

- `--dir <path>` selects the working directory.
- `-m, --model <provider/model>` selects the model for this TUI process only.
- `--variant <none|minimal|low|medium|high|xhigh|max>` selects the reasoning
  effort for this TUI process only.
- `-s, --session <id>` starts from an explicit session.
- `--new` starts from a new session on the first submitted prompt.
- `--debug` enables debug-only local projections, including usage parts and
  allowlisted provider metadata summaries.

When positional message text is supplied, TUI submits it immediately and then
continues the prompt loop. In non-terminal stdin, each input line is processed
as one prompt or slash command. Non-terminal stdin is not appended to the
positional prompt, and the fullscreen alternate screen is not used.

`pevo tui` requires initialized `PSYCHEVO_HOME`, because TUI-local state lives
under that home. `PSYCHEVO_CONFIG` and `PSYCHEVO_DB` may still override provider
configuration and SQLite state path, but they do not bypass the home
initialization requirement.

## Session Behavior

Without `--session` or `--new`, TUI resumes the latest `run` or `tui` session
for the canonical working directory. If no matching session exists, the first
submitted prompt creates a new session with `source = "tui"`.

`--session` resumes the requested session. `--new` defers creation until the
first prompt is submitted, then creates a `source = "tui"` session.

When TUI starts with a current session, it loads that session's sanitized
history into the transcript before accepting input. Switching sessions inside
fullscreen TUI replaces the displayed transcript with the selected session's
sanitized history. Folded reasoning remains hidden or folded according to TUI
rendering rules and must not leak provider replay fields. TUI history reload
may restore folded local reasoning into `Thinking: <reasoning>` transcript
evidence, but only from persisted message material that is already marked as
reasoning and never by replaying provider wire fields as visible assistant
text.

Fullscreen composer history is seeded from the current session's persisted user
prompts in session order. Switching sessions replaces that persisted prompt
seed with the selected session's prompts while preserving slash commands
submitted earlier in the current TUI process. History recall still preserves the
in-progress draft and restores it when the user moves past the newest history
entry.

After a prompt has run, later prompts in the same TUI process append to the
current session explicitly.

TUI sessions have an optional display title. When a new TUI session is created
from a user prompt and the session title is still empty, TUI attempts to
generate a concise title with the selected provider/model by using a
non-persisted, no-tool title request. That title request must not append
messages, tool calls, usage rows, or evidence to the session transcript. If the
title request fails, returns empty text, or returns unusable text, TUI falls
back to a deterministic title derived from the first user prompt. Titles are
trimmed, internal whitespace is collapsed, and stored titles are bounded to 100
characters.

Fullscreen TUI must treat the streamed `agent_end` event as the end of the
interactive turn. Auxiliary work that may happen after `agent_end`, including
new-session title generation, must not keep the composer blocked or cause later
prompts to fail with `a turn is already running`.

`/rename <title>` updates the current session title. It is available in
fullscreen and non-terminal scripted TUI. Empty titles and rename attempts
without a current session fail with bounded user-visible errors.

## TUI State

`$PSYCHEVO_HOME/tui-state.json` is a TUI-local state file. It must not store raw
API keys, provider credentials, full prompts, transcripts, reasoning text, tool
results, or provider payloads.

The state file stores:

- a version number
- global `thinking_visible`
- global `sidebar_visible`
- current model and optional variant override per canonical workdir
- current `mode` per canonical workdir
- a bounded global recent-model list

`thinking_visible` defaults to `true`. Per-workdir `mode` defaults to
`default`. `sidebar_visible` defaults to `false`, preserving the hidden sidebar
startup behavior unless the user has explicitly toggled it in fullscreen TUI.

Startup model and variant precedence is:

1. `pevo tui` CLI flags for the current process
2. per-workdir TUI state
3. existing provider config and environment resolution

Fullscreen `/model` opens an interactive local model picker. Selecting a model
then opens a variant picker. Selecting `Config default` clears the per-workdir
variant override so runtime uses the selected model's configured
`reasoning_effort`; selecting an explicit variant persists that override.
`/variant <none|minimal|low|medium|high|xhigh|max>` continues to update only
the per-workdir variant override. Bare `/variant` is not a display command and
returns a bounded usage error. The removed `/variant set <value>` form returns
bounded guidance to use `/variant <value>`. These TUI state changes affect
later prompts in the current process and do not edit JSONC provider
configuration.

`/show-thinking` toggles global thinking visibility and persists it. It is a
visibility-only control: it does not enable or disable provider reasoning, does
not change `--variant`, and does not edit provider configuration. Fullscreen
TUI must refresh the current transcript projection immediately and must not
append a status row for thinking visibility changes. `/show-thinking on` and
`/show-thinking off` set the value explicitly. `/thinking` is removed and must
return a bounded error that points users to `/show-thinking`.

`/mode <plan|default>` updates the per-workdir mode and persists it. Bare
`/mode` is not a display command and returns a bounded usage error. The removed
`/mode set <value>` form returns bounded guidance to use `/mode <value>`. Mode
changes during a running turn affect the next submitted prompt.

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
- assistant visible output becomes unlabeled answer body text without a left
  rail
- turn-level metadata becomes unlabeled material directly after a visible answer
  and keeps the metadata left rail

Tool failures remain in their original evidence group and render as failures
instead of being moved into a separate generic error log.

Tool evidence shows elapsed execution duration on the right side of the tool
title row. Running tools refresh that value from the local start instant while
the turn is live; completed tools use the runtime-supplied `elapsed_ms` and must
not continue increasing on later redraws. TUI history reload restores completed
tool duration from the tool-result message metadata when available. Narrow
views preserve the right-side duration first and truncate the title when needed.

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

Default metadata projection omits `default` mode and renders elapsed time in
seconds, for example `2.5s`. Completed model messages use the runtime-supplied
`elapsed_ms` captured at message completion when available; fullscreen TUI must
not recompute completed elapsed time from later render or event-drain time.
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

## Keymap

The first fullscreen keymap is fixed:

- `Enter` submits the composer. When slash completion suggestions are visible,
  the first suggestion is selected by default and `Enter` executes that
  suggestion directly.
- `Shift+Enter`, `Ctrl+Enter`, `Alt+Enter`, and `Ctrl+J` insert a newline.
- `Up` and `Down` recall submitted composer history when the current composer
  position is at the first or last logical line respectively. History recall
  preserves the in-progress draft and restores it when the user moves past the
  newest history entry. Within multi-line input away from those boundaries,
  `Up` and `Down` keep their normal textarea cursor movement.
- `Tab` completes slash commands in the composer when the current input starts
  with `/`.
- `Shift+Tab` cycles `default -> plan -> default`.
- `Esc` closes a popup or interrupts a running turn. When idle, it performs no
  destructive action.
- `Ctrl+T` enters transcript selection while leaving composer as the default
  focus.
- `Enter` or `Space` expands or collapses the selected expandable transcript
  block when transcript selection is active.
- When a TUI text selection is active, `Ctrl+C` copies and clears it. Otherwise
  `Ctrl+C` requests quit. `Ctrl+D` quits.
- `Ctrl+B` toggles the local context sidebar.
- `Ctrl+R` enters history search.
- `PageUp`/`PageDown` and mouse wheel scroll the transcript or the active
  bottom selection pane.

Fullscreen TUI enables terminal mouse capture while the alternate screen is
active so mouse wheel events remain inside the application instead of scrolling
host terminal scrollback. Leaving fullscreen disables mouse capture. Left-click
selection is supported for slash menu rows and bottom selection pane rows, and
those interactive row hits take precedence over starting text selection.
Mouse drag selection over rendered transcript and sidebar text is also
supported. The active selection is highlighted while dragging, uses text from
the final rendered buffer rather than pre-wrapped logical lines, locks to the
rendered region where the drag started, and trims only right-side terminal
padding when copying. A drag that starts in the transcript must not copy same-row
sidebar text, and a drag that starts in the sidebar must not copy same-row
transcript text. On mouse release, selected text is copied through the
application clipboard backend and then the selection is cleared. On WSL,
detection must work even when
`WSL_INTEROP` and `WSL_DISTRO_NAME` are absent by inspecting Linux kernel
release/version text for WSL markers. WSL copy prefers `powershell.exe`
`Set-Clipboard` with UTF-8 stdin, then `clip.exe`, then terminal-mediated
OSC52/local Linux fallbacks. Copy failures are bounded visible errors and must
not exit fullscreen TUI. `Esc` clears an active selection before applying normal
idle behavior.

## Slash Commands

The first TUI supports:

- `/quit`, `/exit`, `/q`
- `/status`
- `/clear`, `/new`
- `/sessions`, `/resume`, `/continue`
- `/model`
- `/variant <none|minimal|low|medium|high|xhigh|max>`
- `/mode <plan|default>`
- `/show-thinking`
- `/show-thinking on`
- `/show-thinking off`
- `/rename <title>`
- `/undo`
- `/redo`
- future disabled entries in the slash menu: `/compact` and `/export`

`/help` is not a TUI slash command. It returns the bounded unknown-command
error used for unsupported slash commands.

`/status` shows workdir, home, db, session, model, variant, mode, thinking,
and debug state as one multi-line status block. Fullscreen TUI appends one
status transcript block, and non-terminal scripted TUI writes the same
multi-line status text as one output block.

Fullscreen `/sessions`, `/resume`, `/continue`, and `/model` use the shared
bottom selection pane. The pane includes title/subtitle text, search,
current/default markers, selected-row highlighting, footer hints, `Enter`
selection, `Esc` close or back, arrow/Page/Home/End navigation, and scrolling.

`/sessions`, `/resume`, and `/continue` show date-grouped session rows sorted by
most recently updated with right-aligned updated time and visible-message
counts. Right alignment and row truncation must use terminal display width so
CJK/wide-character titles do not wrap the updated time onto a second line.
Selecting a session replaces the transcript with that session's sanitized
history and does not add a status row. In non-terminal scripted mode,
`/sessions`, `/resume`, and `/continue` print a deterministic session list
instead of opening a panel.

Fullscreen `/model` shows configured provider/model rows from local
configuration only. It must not call live provider catalogs or require provider
credentials. Selecting a model opens a second bottom pane for variant selection.
For a newly selected model, `Config default` is selected by default; for the
current model, the current explicit variant override is selected when one
exists. In non-terminal scripted mode, `/model` prints deterministic local model
information instead of opening a pane.

`/models`, `/model set <provider/model>`, `/session list`, `/session show`, and
`/session switch` are not TUI commands in this slice.

`/undo` reverts the most recent visible user message in the current session,
all later messages, and associated file changes. `/redo` restores a previously
undone message range. Undo and redo use runtime-managed Git snapshots captured
before user prompts; if the target snapshot is unavailable or cannot be
restored, the command reports a bounded error and must not change session
metadata. The command does not require provider credentials and must not start
provider network work.

After `/undo`, the fullscreen composer is populated with the undone user prompt
so the user can edit and resubmit it. Reverted messages are hidden from TUI
history and later model context while the soft revert marker is active. Running
`/undo` repeatedly moves the revert boundary to earlier user messages. Running
`/redo` moves the boundary forward; when no later hidden user message remains,
`/redo` restores the full pre-undo snapshot and clears the revert marker.

Before the next non-command prompt is appended to a session with an active
revert marker, runtime removes the reverted message range and clears the marker.
This cleanup is part of prompt submission and must happen before context
assembly for the new prompt.

If a fullscreen turn is running, `/undo` and `/redo` request interruption first.
If the turn does not settle promptly, the command reports a bounded error and
does not apply the undo or redo operation.

Slash command errors are bounded user-visible text. They must not panic, hang,
or start provider network work unless the command explicitly submits a prompt.

The slash menu appears above the composer while the composer contains a slash
command prefix. It shows at most 8 prefix-filtered rows. Disabled future
commands render with an `upcoming` marker and produce bounded feedback instead
of executing.

Slash menu command labels stay canonical and do not include parameter
placeholders. Parameter hints appear only in description text, such as
`<title> rename current session` for `/rename`, `set <value>` for `/variant`,
`set <plan|default>` for `/mode`, and `toggle; set <on|off>` for
`/show-thinking`. Tab completion inserts only the command token, never a
placeholder template.

The first slash menu row is selected by default. Pressing `Enter` while
suggestions are visible executes that selected command instead of submitting the
partial composer text as an unknown command.

The slash menu supports Up/Down/Home/End selection and left-click row
selection. Up and Down wrap between the first and last visible slash menu rows;
Home and End jump directly to the first and last row. The highlighted slash
command, not always the first row, executes on `Enter`. The slash menu is
hidden while a bottom selection pane is open, and keyboard input is routed to
the pane search and navigation controls until it closes.

## Runtime Modes

Runtime mode is explicit and enforceable by the tool surface.

`default` is the default for `pevo run` and for `pevo tui` when TUI state has no
per-workdir mode. Default mode exposes the current full coding-core tools.

`plan` is hard read-only. It exposes only:

- `read`: read a file under the selected workdir
- `list`: list files or directories under the selected workdir with limits and
  truncation metadata
- `search`: literal text search under the selected workdir with limits and
  truncation metadata

Plan mode must not expose `bash`, `write`, or `edit`. Its read-only semantics
must not depend only on provider instructions.

The runtime sends an ephemeral mode instruction to the provider for the current
turn. The instruction is not persisted as a transcript message.

`pevo run` defaults to `default` and does not expose mode flags in this slice.

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

When a background turn task completes, TUI must drain all queued runtime stream
events before rendering the turn as complete. Final ledger projection must not
lose late tool or message evidence merely because the task finished between
input polling ticks.

Session picker and scripted session-list output must not expose folded
reasoning blocks or provider reasoning wire fields. Folded reasoning blocks
and provider reasoning wire fields must also not appear in rendered
`agent_end` material.

For non-terminal stdin/stdout, `pevo tui` keeps deterministic line-by-line
behavior and renders plain, no-ANSI semantic blocks: `Prompt`, `Thinking`,
`Explored`, `Ran`, `Changed`, `Answer`, and `Meta`. The plain projection keeps
block labels for machine-readable diagnostics even where fullscreen TUI uses
unlabeled prompt and metadata presentation. `--debug` also affects this plain
projection.

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

- [200 pevo CLI](../200-pevo-cli/spec.md) defines the product CLI surface.
- [200 pevo run](../200-pevo-cli/pevo-run.md) defines non-interactive live run.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider message
  translation boundaries.
- [120 Provider Registry](../120-provider-registry/spec.md) defines
  provider/model resolution.
- [040 SQLite Persistence](../040-storage-and-persistence/sqlite-persistence.md)
  defines session and message persistence.
