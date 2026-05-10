---
name: 210. pevo TUI
psychevo_self_edit: deny
---

# 210. pevo TUI Input and Commands

Define fullscreen input handling, keymaps, slash commands, file completion, shell escapes, and local selection behavior.

## Keymap

The first fullscreen keymap is fixed:

- `Enter` submits the composer. When slash completion suggestions are visible,
  the first suggestion is selected by default and `Enter` executes that
  suggestion directly.
- `Shift+Enter`, `Ctrl+Enter`, `Alt+Enter`, and `Ctrl+J` insert a newline.
- Composer input that begins with `!` after leading whitespace is a user shell
  escape. `!<command>` runs the command locally in the selected workdir through
  the bounded runtime shell executor and is not sent to the provider. Bare `!`
  shows bounded shell-help text and does not execute.
- `Up` and `Down` recall submitted composer history when the current composer
  position is at the first or last logical line respectively. History recall
  preserves the in-progress draft and restores it when the user moves past the
  newest history entry. Within multi-line input away from those boundaries,
  `Up` and `Down` keep their normal textarea cursor movement.
- `Tab` completes slash commands in the composer when the current input starts
  with `/`.
- Typing an `@` token in the fullscreen composer opens a file path completion
  popup for the selected workdir. Valid tokens start at the beginning of the
  current line or after whitespace, so `@`, `@src`, and `hello @src` trigger
  completion while `foo@bar` does not. The popup is hidden while a bottom
  selection pane is open.
- While the `@` file popup is visible, `Up`/`Down` wrap selection,
  `Home`/`End` jump to the first/last result, `Tab` and `Enter` insert the
  selected relative path, and `Esc` closes the popup without editing text.
  Inserting a path replaces only the active `@` token, appends one trailing
  space, and quotes paths containing whitespace when they do not already
  contain a double quote.
- `Shift+Tab` cycles `default -> plan -> default`.
- `Esc` clears active UI state before it can interrupt work: text selection,
  file and skill popups, slash menu, bottom selection panes, history search,
  transcript focus, and an empty shell-mode composer all take priority. If none
  of those states is active and foreground work is running, `Esc` requests
  interruption through runtime control. Runtime-controlled provider generation
  and foreground shell waits must wake on that signal instead of waiting for the
  next provider chunk, shell polling interval, or title-generation follow-up.
  When idle, it performs no destructive action.
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
- `/stats`
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
- `/skills`
- `/skill:<name> [args]`
- future disabled entries in the slash menu: `/compact` and `/export`

`/help` is not a TUI slash command. It returns the bounded unknown-command
error used for unsupported slash commands.

`/status` shows workdir, home, db, session, model, variant, mode, and debug
state as one multi-line status block. It does not include thinking visibility;
`/show-thinking` remains the dedicated command for changing and reporting that
setting. Fullscreen TUI appends one status transcript block, and non-terminal
scripted TUI writes the same multi-line status text as one output block.

`/stats` shows local usage and estimated-cost statistics for the current
workdir from persisted SQLite accounting. Fullscreen TUI opens the shared
bottom selection pane in a read-only stats mode; non-terminal scripted TUI
prints the same deterministic summary. `/stats` must not call providers or
refresh model catalogs.

Fullscreen `/sessions`, `/resume`, `/continue`, and `/model` use the shared
bottom selection pane. The pane includes title text, search directly below the
title, current/default markers, selected-row highlighting, footer hints,
`Enter` selection, `Esc` close or back, arrow/Page/Home/End navigation, and
scrolling. Shared bottom selection panes do not render subtitles.

`/sessions`, `/resume`, and `/continue` show date-grouped session rows sorted by
most recently updated with right-aligned updated time and visible-message
counts. The pane title identifies whether it is showing active or archived
sessions, and the footer exposes `Tab` view switching plus the action-mode
entrypoint. Right alignment and row truncation must use terminal display width
so CJK/wide-character titles do not wrap the updated time onto a second line.
Selecting an active session replaces the transcript with that session's
sanitized history and does not add a status row. Selecting an archived session
restores it, switches to it, replaces the transcript with its sanitized
history, and does not add a status row. In non-terminal scripted mode,
`/sessions`, `/resume`, and `/continue` print a deterministic active-session
list instead of opening a panel.

Fullscreen `/model` shows an `Add provider` action row, a status-style
`All providers` row, and a selectable provider status row before each
provider's models. These action rows replace non-selectable provider group
headers. Selecting `Add provider` opens a bottom-panel wizard for creating a
global user-defined OpenAI Chat-compatible provider. Selecting `All providers`
concurrently fetches every fetchable provider catalog; selecting a provider row
fetches or retries only that provider. Fetch rows use `Enter fetch` in the
footer. Model rows use `Enter select`.

The `/model` add-provider wizard writes only global Psychevo provider
configuration and global `.env` credentials. It prompts for display label,
editable provider id, base URL, and API key when the generated key variable is
not already present. The provider id is generated from the label as a slug, and
the key variable is `<PROVIDER_ID_UPPER>_API_KEY` with non-alphanumeric
characters converted to `_`. Existing key variables in global `.env` are
reused and never overwritten. The wizard rejects duplicate provider ids,
built-in ids, built-in aliases, invalid ids, missing labels, missing base URLs,
and base URLs that do not start with `http://` or `https://`.

Saving a provider appends or updates only the new provider entry in global
`$PSYCHEVO_HOME/config.jsonc`, writes raw API keys only to
`$PSYCHEVO_HOME/.env`, refreshes the model pane, fetches the new provider
catalog, and focuses that provider row while the fetch is pending. It does not
edit the global default model. If TUI was started with `PSYCHEVO_CONFIG`, the
add-provider wizard reports a bounded error because the global config is not
the active provider configuration source.

`/model` fetch is explicit and fullscreen-only. There is no `/model fetch`
slash command, opening `/model` does not call remote catalogs, and
non-terminal scripted `/model` prints deterministic local model information
only.

Model fetch rows use status words instead of command text. `All providers` and
provider rows may show `not fetched`, `fetching`, `fetched N models`,
`no models`, `partial failed`, `failed: <short error>`, or
`missing <ENV>`. Missing credentials reuse runtime credential resolution, so
loopback/no-auth providers can fetch without an Authorization header while
non-local providers with no key show the missing environment variable. A
provider fetch times out after five seconds and shows `failed: timeout`.

Fetchable providers come from the current configured provider map and the
provider currently implied by CLI, environment, top-level config, or TUI state
model selection. Providers are not added only because a credential environment
variable is present. Catalog requests reuse runtime provider base URL and
credential resolution. The OpenAI-compatible models endpoint is derived by
replacing a trailing `/chat/completions` path with `/models`, otherwise by
appending `/models` to the resolved base URL. The first slice does not add a
catalog URL config field and does not filter non-chat model ids from remote
catalog results.

Fetch results are cached only for the current TUI process. Closing and
reopening `/model` preserves provider fetch state and fetched models but starts
with an empty search query. Fetch failure does not clear the previous fetched
models for that provider. `Esc` cancels unfinished provider requests and keeps
completed results. Selecting an existing model while a fetch is in progress is
allowed and cancels unfinished catalog requests when the pane closes or moves to
variant selection.

Model rows show known model metadata compactly: context and output limits,
capability tags, and input/output/cache pricing when available. Unknown
metadata is omitted rather than shown as zero. The model picker keeps local rows authoritative. When a local configured model
and fetched model have the same provider/model id, the local row is shown and
the fetched source is not displayed. Pure fetched rows show only `fetched` plus
known remote metadata. Fetched model ids are displayed unchanged and sorted by
model id within their provider. Refresh removes stale fetched-only rows unless
the stale model is the current TUI selection, in which case the current row
remains visible. If TUI state references a current model that is no longer in
local config, `/model` still shows that current model row; runtime execution
continues to use existing provider/model resolution errors if the provider can
no longer be resolved.

When `/model` opens, focus starts on the current model when present, on the
first local model when no current model is present but local models exist, and
on `All providers` only when there are no model rows. `All providers` is always
visible during search. A provider query shows the provider row and that
provider's models; a model match also keeps its provider row visible. If no
model matches a query, `All providers` remains visible and a fetch preserves the
current query.

Selecting a fetched-only model opens the existing variant pane. For such rows,
the `Config default` variant row describes `use provider default`. Final model
selection writes only TUI state for the current workdir and updates recent
models. It does not edit JSONC provider configuration.

All bottom selection panes keep `Home` and `End` as direct first/last jumps, and
their `Up` and `Down` navigation wraps between the first and last visible rows.

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

When the user interrupts a foreground turn, queued composer submissions are not
automatically started after the aborted turn settles. Queued prompt inputs and
queued shell escapes are restored to the composer in FIFO order; shell commands
are restored as `!<command>` lines. If the composer already contains a draft,
the restored queue text is inserted before that draft, separated by newlines.
Normal turn completion and ordinary failures retain the existing FIFO
auto-start behavior.

Slash command errors are bounded user-visible text. They must not panic, hang,
or start provider network work unless the command explicitly submits a prompt.

`/skills` lists discovered skills in deterministic precedence order. In
fullscreen mode it appends a bounded status-style transcript block; in scripted
mode it prints the same list deterministically.

`/skill:<name> [args]` expands the named skill into the next prompt using the
skill expansion contract from [055 Skills](../055-skills/spec.md). Unknown
skills report a bounded error and do not submit a provider prompt.

The slash menu appears above the composer while the composer contains a slash
command token. It shows at most 8 matched rows. Matching uses the canonical
command label and orders exact matches first, prefix matches next, and
subsequence fuzzy matches last while preserving menu order within each class.
When skill commands are enabled, discovered skills appear as dynamic
`/skill:<name>` rows after built-in slash commands and participate in the same
matching and Tab completion behavior.
Whitespace after the command token hides the menu so argument text does not
produce slash suggestions. Disabled future commands render with an `upcoming`
marker and produce bounded feedback instead of executing.

Slash menu command labels stay canonical and do not include parameter
placeholders. Parameter hints appear only in description text, such as
`<title> rename current session` for `/rename`, `set <value>` for `/variant`,
`set <plan|default>` for `/mode`, and `toggle; set <on|off>` for
`/show-thinking`. `/model` is described as `select/fetch model`. Tab
completion remains prefix-only and inserts only the command token, never a
placeholder template or a fuzzy-only match.

The first slash menu row is selected by default. Pressing `Enter` while
suggestions are visible executes that selected command instead of submitting the
partial composer text as an unknown command.

The slash menu supports Up/Down/Home/End selection and left-click row
selection. Up and Down wrap between the first and last visible slash menu rows;
Home and End jump directly to the first and last row. The highlighted slash
command, not always the first row, executes on `Enter`. The slash menu is
hidden while a bottom selection pane is open, and keyboard input is routed to
the pane search and navigation controls until it closes.

The fullscreen `@` file popup searches paths under the canonical TUI workdir.
Results are shown as workdir-relative paths with directory rows visually marked
and are limited to 8 visible rows. Search respects gitignore rules, allows
hidden files, skips obvious VCS internals, and discards stale asynchronous
results when the composer token changes before a search completes. Selecting a
result inserts plain prompt text only; it does not create a structured mention,
attach file contents, or change runtime context assembly.

## Related Topics

- [210 pevo TUI](spec.md) is the parent topic.
- [210 pevo TUI Testing](testing.md) defines deterministic acceptance coverage.
