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
- fullscreen transcript, composer, status/footer, and local context sidebar
- persisted TUI-local model, variant, mode, and thinking visibility
- session, model, variant, mode, thinking, status, and help slash commands
- evidence-ledger rendering for prompts, folded reasoning, tool evidence,
  final answers, and turn metadata
- transcript selection, keyboard expansion, and mouse expansion for bounded
  tool evidence
- debug projection for usage and provider metadata summaries
- deterministic visual-regression projections and local diagnostic screenshots
- hard `plan` / `build` runtime mode selection

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

After a prompt has run, later prompts in the same TUI process append to the
current session explicitly.

## TUI State

`$PSYCHEVO_HOME/tui-state.json` is a TUI-local state file. It must not store raw
API keys, provider credentials, full prompts, transcripts, reasoning text, tool
results, or provider payloads.

The state file stores:

- a version number
- global `thinking_visible`
- current model and variant per canonical workdir
- current `mode` per canonical workdir
- a bounded global recent-model list

`thinking_visible` defaults to `true`. Per-workdir `mode` defaults to `build`.

Startup model and variant precedence is:

1. `pevo tui` CLI flags for the current process
2. per-workdir TUI state
3. existing provider config and environment resolution

`/model set <provider/model>` and `/variant set <value>` update TUI state and
affect later prompts in the current process. They do not edit JSONC provider
configuration.

`/thinking` toggles global thinking visibility and persists it. It follows
OpenCode's visibility-only model: it does not enable or disable provider
reasoning, does not change `--variant`, and does not edit provider
configuration.

`/mode set <plan|build>` updates the per-workdir mode and persists it. Mode
changes during a running turn affect the next submitted prompt.

## Layout

Interactive terminals use raw mode and the alternate screen.

The first fullscreen layout is an evidence ledger, not a row-level event log.

The main transcript area is scrollable and renders each turn as a structured
ledger block:

- a left rail that visually connects evidence belonging to the same turn
- a dark prompt block for the submitted user prompt
- interleaved folded thinking, tool evidence, and assistant answer material
- the final assistant answer as unlabeled body text
- turn metadata after the answer: mode, provider/model, elapsed time, token
  metrics only when known, and failures only when present

The bottom area contains an OpenCode-style composer: a left accent rail, a
subtle input surface, status line, and compact shortcut footer. It must not use
a full bright border around the composer as the primary visual treatment.

The right sidebar is fixed at 42 columns when visible on wide terminals. It is
hidden by default on narrow terminals and toggleable on narrow or wide
terminals.

The sidebar is local-only. It may show session id/source, workdir, git branch,
model, variant, current mode, thinking state, message/tool counts, and changed
files. It must not call live provider catalogs or probe provider APIs.

The sidebar sections are:

- Session
- Context
- Modified Files
- Footer

Modified Files prefers session-local diff evidence when available. In the first
slice, it may fall back to local git status. It shows at most 10 tail-compacted
paths with compact `+/-` statistics when those statistics are known.

Long local paths in the sidebar should be tail-compacted to preserve the
rightmost useful path segments and avoid multi-line path walls.

## Evidence Projection

TUI renders runtime events into semantic ledger evidence:

- user prompts become `Prompt` blocks
- folded reasoning becomes `Thinking` evidence
- `read`, `list`, and `search` tool calls become `Explored` evidence
- `bash` tool calls become `Ran <first command line>` evidence
- `write` and `edit` tool calls become `Changed` evidence
- assistant visible output becomes unlabeled answer body text
- turn-level metrics become `Meta` material after the answer

Tool failures remain in their original evidence group and render as failures
instead of being moved into a separate generic error log.

Long tool outputs default to a maximum of 20 visible lines. Expandable evidence
keeps the full stored output available for local inspection in this TUI process
or from persisted message/tool-result material when available.

Usage and provider metadata are not transcript content blocks. They may be
projected into turn metadata or debug views, but they must not appear in
sanitized transcript messages, provider replay across incompatible providers,
or `pevo run --format json` by default.

Default metrics projection shows total tokens and context percentage only when
the model context limit is known. Debug projection shows usage parts and an
allowlisted provider metadata summary.

## Keymap

The first fullscreen keymap is fixed:

- `Enter` submits the composer.
- `Shift+Enter`, `Ctrl+Enter`, `Alt+Enter`, and `Ctrl+J` insert a newline.
- `Tab` cycles `plan -> build -> plan`.
- `Shift+Tab` cycles in reverse.
- `Esc` closes a popup or interrupts a running turn. When idle, it performs no
  destructive action.
- `Ctrl+T` enters transcript selection while leaving composer as the default
  focus.
- `Enter` or `Space` expands or collapses the selected expandable transcript
  block when transcript selection is active.
- `Ctrl+C` and `Ctrl+D` request quit or quit.
- `Ctrl+B` toggles the local context sidebar.
- `Ctrl+R` enters history search.
- `PageUp` and `PageDown` scroll the transcript.

Mouse input is limited to clicking expandable transcript evidence blocks. The
first slice does not use mouse input for focus management, selection ranges,
scrollbars, menus, sidebars, or tool execution.

## Slash Commands

The first TUI supports:

- `/help`
- `/quit`, `/exit`, `/q`
- `/status`
- `/clear`, `/new`
- `/session list`
- `/session show [id]`
- `/session switch <id|prefix|latest>`
- `/model`
- `/models`
- `/model set <provider/model>`
- `/variant`
- `/variant set <none|minimal|low|medium|high|xhigh|max>`
- `/mode`
- `/mode set plan`
- `/mode set build`
- `/thinking`
- `/thinking on`
- `/thinking off`
- future disabled entries in the slash menu: `/undo`, `/compact`, and
  `/export`

Slash command errors are bounded user-visible text. They must not panic, hang,
or start provider network work unless the command explicitly submits a prompt.

The slash menu appears above the composer while the composer contains a slash
command prefix. It shows at most 8 prefix-filtered rows. Disabled future
commands render with an `upcoming` marker and produce bounded feedback instead
of executing.

## Runtime Modes

Runtime mode is explicit and enforceable by the tool surface.

`build` is the default for `pevo run` and for `pevo tui` when TUI state has no
per-workdir mode. Build mode exposes the current full coding-core tools.

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

`pevo run` defaults to `build` and does not expose mode flags in this slice.

## Rendering

The TUI uses a compact Codex-style terminal palette:

- default foreground for primary text
- dim secondary text
- cyan hints and status markers
- green success markers
- red failures
- magenta `pevo` identity

Assistant visible text streams inline inside the current turn. Thinking is
visible by default, rendered as folded/debug material under a `Thinking`
evidence block, not as assistant transcript text. When `/thinking` is off, TUI
shows only a compact thinking indicator without exposing the reasoning content.
Thinking display is local UI material only; it is not promoted into visible
transcript projection, JSON run output, provider replay across providers,
`/session show`, or rendered `agent_end` material.

TUI should create an answer row only after visible assistant text exists. It
must not pre-render an empty answer row that pushes thinking or tool evidence
out of the first visible ledger projection.

Tool starts and ends render as compact evidence blocks. Long tool result bodies
are summarized rather than dumped unless the block is expanded.

When a background turn task completes, TUI must drain all queued runtime stream
events before rendering the turn as complete. Final ledger projection must not
lose late tool or message evidence merely because the task finished between
input polling ticks.

Session display commands use sanitized transcript projection. Folded reasoning
blocks and provider reasoning wire fields must not appear in `/session show` or
rendered `agent_end` material.

For non-terminal stdin/stdout, `pevo tui` keeps deterministic line-by-line
behavior and renders plain, no-ANSI semantic blocks: `Prompt`, `Thinking`,
`Explored`, `Ran`, `Changed`, `Answer`, and `Meta`. `--debug` also affects this
plain projection.

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

Real terminal PNG screenshots are diagnostic artifacts. They may be generated
from a deterministic local mock-provider demo through VHS, but they are not
checked-in goldens and are not compared pixel-by-pixel in default validation.
The diagnostic artifact root is `.local/.psychevo-dev/tui-shots/<timestamp>/`.
The deterministic demo should isolate git state from the parent repository and
pin terminal color inputs, including clearing inherited `NO_COLOR`, so
screenshots are useful as visual diagnostics.

## Related Topics

- [200 pevo CLI](../200-pevo-cli/spec.md) defines the product CLI surface.
- [200 pevo run](../200-pevo-cli/pevo-run.md) defines non-interactive live run.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider message
  translation boundaries.
- [120 Provider Registry](../120-provider-registry/spec.md) defines
  provider/model resolution.
- [040 SQLite Persistence](../040-storage-and-persistence/sqlite-persistence.md)
  defines session and message persistence.
