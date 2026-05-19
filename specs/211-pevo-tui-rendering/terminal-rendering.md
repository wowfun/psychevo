---
name: 211. pevo TUI Terminal Rendering
psychevo_self_edit: deny
---

# 211. pevo TUI Terminal Rendering

Define terminal-adaptive palette, Markdown/raw/plain rendering, scrolling, and runtime-drain rendering behavior.

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
the composer or bottom status line. Scroll limits, row hit areas, prompt
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
content, visibility, expansion state, active elapsed labels, and active motion
markers still match the transcript. When focus is in the composer and no popup
or pane is active, `Up`/`Down` remain input/history boundary keys; transcript
scrolling uses PageUp/PageDown or real mouse-wheel hover events.
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
`Status mode` rows to the transcript. The bottom status line is the source of
truth for the current mode.

Tool starts and ends render as compact evidence blocks whose titles start with
the actual tool invocation name. Long tool result bodies are summarized rather
than dumped unless the block is expanded.

User shell escapes render through the same compact shell evidence shape, but
with the local `! <first command line>` invocation label instead of a `Ran`
category verb. Failures remain in that evidence group. The provider-visible
persisted record is a user-role text fragment:
`<user_shell_command><command>...</command><result>Exit code: ... Duration: ...
seconds Truncated: ... Output: ...</result></user_shell_command>`. TUI display
metadata and message content text must keep session reloads and history recall
human-facing: reload renders the persisted shell record as `! <first command
line>` evidence rather than a raw XML user prompt, and composer history recalls
it as `!<command>`.

When the fullscreen composer contains a user shell escape, the bottom state
line includes a local shell-mode marker while preserving the normal model,
variant, and mode display footprint.

When a background turn task completes, TUI must drain all queued runtime stream
events before rendering the turn as complete. Final ledger projection must not
lose late tool or message evidence merely because the task finished between
input polling ticks.

Fullscreen prompts submitted while an agent turn or user shell command is active
are queued in submission order. User shell escapes submitted while a foreground
agent turn is active start immediately as auxiliary shell tasks; their bounded
results are injected as external user shell context at provider request
boundaries, after tool batches, and before terminal checks. If the foreground
turn has already ended by the time the auxiliary shell completes, the shell
context still persists on the same session for the next turn. User shell escapes
submitted while another standalone shell command is active keep the existing
shell abort/queue behavior. Bare queued `!` shows the bounded help text and then
drains the next queued item.

Session picker and scripted session-list output must not expose folded
reasoning blocks or provider reasoning wire fields. Folded reasoning blocks
and provider reasoning wire fields must also not appear in rendered
`agent_end` material.

For non-terminal stdin/stdout, `pevo tui` keeps deterministic line-by-line
behavior and renders plain, no-ANSI semantic blocks: `Prompt`, `Thinking`,
tool-name-first evidence, `Answer`, and `Meta`. Lines beginning with `!` after
leading whitespace run as user shell escapes and do not require provider
credentials. The plain projection keeps block labels for machine-readable
diagnostics where they still exist, even where fullscreen TUI uses unlabeled
prompt and metadata presentation. `--debug` also affects this plain projection.
