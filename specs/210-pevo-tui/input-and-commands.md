---
name: 210. pevo TUI
psychevo_self_edit: deny
---

# 210. pevo TUI Input and Commands

Define fullscreen input handling, keymaps, slash commands, file completion,
shell escapes, and local selection behavior. Shared interaction principles and
visual treatment come from [080 Design System](../080-design-system/spec.md).

## Keymap

The first fullscreen keymap is fixed and composer-first. V1 keeps the key
handling organized for future user keymap configuration, but it does not expose
custom keymaps.

- `Enter` submits the composer. When slash completion suggestions are visible,
  the first suggestion is selected by default and `Enter` executes that
  suggestion directly.
- `Shift+Enter`, `Ctrl+Enter`, `Alt+Enter`, and `Ctrl+J` insert a newline.
- Shell mode is an explicit composer state, not literal `!` text in the
  textarea. Pressing `Shift+1` from an empty composer enters shell mode and
  leaves the textarea empty. While shell mode is active, the composer prompt
  marker is `! ` instead of the normal prompt marker. Empty shell mode exits on
  either `Esc` or `Backspace`. Pasted text, history recall, scripted input, or
  raw submit text that begins with `!` after leading whitespace imports as
  shell mode with the leading `!` stripped. Submitting shell mode records
  `!<command>` in composer history, but the shell execution layer receives only
  `<command>`. Bare shell mode submission shows bounded shell-help text and
  does not execute.
- `!<command>` runs the command locally in the selected workdir through the
  bounded runtime shell executor. The command is not a provider-callable `bash`
  tool, but its bounded result is persisted as model-visible user shell context
  according to the runtime shell-context contract. Live and reloaded user shell
  transcript rows render like user prompt rows: the command line uses the same
  full-width prompt background, starts with `! ` followed by the user's command,
  uses the same marker color as the shell-mode composer `!`, and omits the
  normal tool-evidence bullet and `Ran` label. The command output
  remains below that prompt-styled command line as bounded evidence detail. This
  distinguishes user-submitted shell commands from model-requested bash tool
  calls.
- `Up` and `Down` recall submitted composer history when the current composer
  position is at the first or last logical line respectively. History recall
  preserves the in-progress draft and restores it when the user moves past the
  newest history entry. Within multi-line input away from those boundaries,
  `Up` and `Down` keep their normal textarea cursor movement.
- `Tab` completes slash commands in the composer when the current input starts
  with `/` and shell mode is not active.
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
- Shell mode reuses the same `@` file path completion popup, so
  `cat @src<Tab>` inserts a workdir-relative path such as `src/main.rs ` using
  the existing whitespace quoting rules. Image paths selected this way remain
  plain text paths and do not create attachments. Naked shell words such as
  `cat src<Tab>` do not trigger shell-native completion.
- `Shift+Tab` cycles `default -> plan -> default`.
- `Esc` clears active UI state before it can interrupt work: text selection,
  file and skill popups, slash menu, bottom selection panes, history search,
  and an empty shell-mode composer all take priority. If none of those states
  is active and foreground work is running, `Esc` requests
  interruption through runtime control. Runtime-controlled provider generation
  and foreground shell waits must wake on that signal instead of waiting for the
  next provider chunk, shell polling interval, or title-generation follow-up.
  When idle, it performs no destructive action.
- `Ctrl+T` focuses the transcript. In transcript focus, `Up`/`Down` move the
  focused transcript row, `PageUp`/`PageDown` scroll, `Enter`/`Space` toggles
  folded evidence rows or opens clickable `Agent` rows, and `Esc` returns to
  composer focus.
- `?` opens contextual shortcut help when the current surface supports it.
- When a TUI text selection is active, `Ctrl+C` copies and clears it. Otherwise
  `Ctrl+C` requests quit. `Ctrl+D` quits.
- `Ctrl+O` copies the latest visible assistant answer as raw Markdown source,
  equivalent to `/copy`.
- `Ctrl+B` toggles the local context sidebar.
- `Ctrl+R` enters history search.
- `PageUp`/`PageDown` scroll the transcript or active bottom selection pane.
  Mouse wheel events route by the current pointer row: transcript hover scrolls
  the transcript, bottom-pane hover scrolls the pane, and composer/status/other
  non-scrollable hover does not trigger composer history recall.
- In composer focus, plain `Up` and `Down` are input/history boundary keys, not
  transcript scrolling keys. `Up` recalls the previous submitted prompt only
  when the composer cursor is on the first logical line; an empty composer at
  that boundary recalls the latest prompt. `Down` recalls the next submitted
  prompt only while a history entry is already active and the cursor is on the
  last logical line, restoring the saved draft after the newest history entry.
  Otherwise `Up`/`Down` remain textarea navigation or no-op behavior.

Transcript rows are a lightweight keyboard focus target, not a modal editor.
Bounded Thinking, command, and tool evidence details expand inline by mouse
clicking the folded row or by focusing the transcript with `Ctrl+T` and pressing
`Enter`/`Space` on the selected row. Clickable `Agent` rows use the same row
focus path to enter the child session. Composer focus remains the default after
turn completion and after `Esc`.

Fullscreen TUI enables terminal mouse capture while the alternate screen is
active and enables xterm alternate-scroll mode so terminal wheel input stays
inside the fullscreen application instead of scrolling host terminal scrollback.
Wheel input reported as mouse events with coordinates uses hover-based routing;
terminals that synthesize plain `Up`/`Down` cursor keys are indistinguishable
from real keyboard input once delivered to the app and are handled by the
composer boundary rules above. Leaving fullscreen disables mouse capture and
leaves alternate-scroll disabled. Fullscreen TUI also enables bracketed paste
while active and disables it during terminal restoration.
Bracketed paste events are inserted into the composer as a single paste
operation after normalizing `\r\n` and bare `\r` to `\n`; pasted text must not be
reinterpreted as key-by-key input or lose bytes from local filesystem paths.
Pasting a standalone image source adds it to the pending image attachments
only when the pasted text resolves to a readable image source, and inserts a
plain-text attachment placeholder into the composer using the fixed
`[Image #N]` format. Pasting a standalone image-looking path that is missing,
unreadable, or unsupported inserts the normalized text unchanged and does not
show an image error. Pasting ordinary text, including prompts with embedded
local paths, relative paths, `http(s)://` URLs, or `data:image/*` URLs, inserts
the normalized text unchanged and must not auto-extract attachments. Pasting
updates file, skill, and slash completion popup state the same way as ordinary
composer edits. Left-click selection is
supported for slash menu rows and bottom selection pane rows, and those
interactive row hits take precedence over starting text selection.
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

- `/help`
- `/quit`, `/exit`, `/q`
- `/status`
- `/usage`, `/stats`
- `/context`
- `/clear`, `/new`
- `/sessions`, `/resume`, `/continue`
- `/model`
- `/variant <none|minimal|low|medium|high|xhigh|max>`
- `/mode <plan|default>`
- `/show-thinking`
- `/show-thinking on`
- `/show-thinking off`
- `/show-raw`
- `/show-raw on`
- `/show-raw off`
- `/copy`
- `/export [path] [--format markdown|json] [-i|--include list]`
- `/share [path] [-i|--include list]`
- `/image <source> [prompt]`
- `/rename <title>`
- `/undo`
- `/redo`
- `/skills`
- `/skill:<name> [args]`
- future disabled entries in the slash menu: `/compact`

Slash command discovery is registry-backed and remains a lightweight menu above
the composer. The slash menu shows canonical command labels only, stays a flat
list with at most 8 rows, and does not show hidden aliases or group headers.
Slash menu summaries stay compact enough for one-line discovery. Expanded
`/help` may add short detail lines for commands whose consequences are easy to
miss, such as local artifact writes, provider calls, session mutation, display
state, clipboard behavior, shell/image submission, or sensitive export
includes.
Fullscreen TUI projects slash-command feedback
that is written to the transcript as one command transcript row: the first line
echoes the submitted command as `> <command>`, and the result begins on the
next line with `└`. This command row is display-only and must not count as a
user prompt, visible message, durable session message, or provider-context
input. Slash commands that open bottom panes, including `/help`, do not append a
command transcript row. Non-terminal scripted TUI keeps deterministic plain text
output without command-row wrapping. Command transcript rows are foldable by
mouse in fullscreen; they default open, collapse to the echoed command line, and
expand back to the full local result. `/help` does not accept arguments.

`/help` output uses the three groups defined by [026 Commands](../026-commands/spec.md).
`General` lists ordinary keyboard shortcuts plus common built-in commands:
`/status`, `/context`, `/model`, `/sessions`, `/new`, `/copy`, `/undo`,
`/redo`, and `/quit`. `Commands` lists all built-in slash commands in canonical registry
order. `Custom commands` summarizes dynamic skill invocation as
`/skill:<name> [args]` with the discovered skill count, or reports
`No custom commands available` when skills are disabled or none are discovered.
Help rows use `<usage> - <summary>` and may append compact alias text on the
canonical command row. Rows with expanded detail render the detail immediately
after the command row as a short indented continuation line. Fullscreen `/help`
opens a bottom help pane with
`Help`, `General`, `Commands`, and `Custom commands` header tabs; `Esc` closes
the pane, and tab/arrow navigation may switch help sections. Scripted `/help`
prints the same deterministic help text without opening a pane.

`/status` shows workdir, home, db, session, model, variant, mode, and debug
state as one multi-line status block. It does not include thinking or raw
visibility; `/show-thinking` and `/show-raw` remain the dedicated commands for
changing and reporting those settings. Fullscreen TUI appends one command
transcript row without an extra `Status` title, and non-terminal scripted TUI
writes the same multi-line status text as one output block.

Composer submission classifies input before slash parsing. Leading shell
escapes keep taking precedence. Known slash commands and unknown slash-looking
commands remain slash command input, including bounded errors such as
`/unknown`. Ordinary prompt text is not scanned for image paths or image URLs:
`描述这张图片的内容：img1.avif`, `/home/me/out.avif`, `@img.avif`,
`https://example.com/image.png`, and `data:image/*;base64,...` are all prompt
text unless they came from an existing pending image placeholder. This prevents
output-path prompts from being misclassified as missing input images. The only
fullscreen attachment entrypoints are `/image <source> [prompt]` and a
standalone paste that resolves to a readable image source.

Image inputs are tracked as pending composer attachments bound to plain-text
placeholders. `/image <source> [prompt]` adds one image source to the pending
set and rewrites the composer to include `[Image #N]` followed by `[prompt]`
when present. Multiple images are added by repeating `/image` or by multiple
standalone readable image-source pastes, not by parsing several sources from
one command. Pending images are shown once in the bottom status line as
`images N`; successful attachment adds must not also show a second transient
`attached image N` status. Editing or deleting a placeholder before submission
unbinds that attachment. On submit, the TUI sends only attachments whose
complete placeholder text remains in the composer, ordered by first
placeholder appearance, and compresses the final attachment metadata numbering
to `image 1..N`. Pending images, attachment placeholders, and ephemeral
status/error text are cleared after successful submission or `/new`, and move
with queued prompts when a turn is already running. Image-only submission is
allowed when at least one pending placeholder remains.

Image sources may be absolute local paths, workdir-relative local paths, quoted
paths, paths with escaped spaces, `file://` URLs, `http(s)://` URLs, or
`data:image/*;base64,...` URLs when they are supplied through `/image` or as a
standalone paste. Local paths must resolve to readable files with supported
image extensions and must not exceed the configured local source size limit
before an attachment is created. Remote URLs are not downloaded or preflighted
locally. If selected model metadata explicitly says image input is unsupported,
the TUI does not send structured image blocks; it degrades the submission to
text containing the attachment source list plus the prompt with image
placeholders removed, with bounded feedback telling the user the image was
degraded to text.

`/copy` copies the latest visible assistant answer as raw Markdown source. It
does not copy Thinking, tool evidence, metadata, selected transcript rows, or
rendered rich text. It is unaffected by `/show-raw`, so rich and raw transcript
display modes copy the same source text. Fullscreen TUI reports copy success or
failure through short status feedback and must not append a command transcript
row. If no assistant answer is visible, it reports a bounded failure status.

`/export [path] [--format markdown|json] [-i|--include list]` writes selected
sections from the current persisted session as a local artifact. When `path` is
omitted, Markdown writes
`psychevo-session-<short-session-id>.md` and JSON writes
`psychevo-session-<short-session-id>.json` in the selected workdir. When `path`
is relative, it resolves against the selected workdir. The command uses the
same include semantics and section projection as `pevo session export`. The
export include vocabulary is `header` (`h`), `messages` (`m`), `reasoning`
(`r`), `provider-input-evidence` (`pie`), and `last-provider-request` (`lpr`).
If `--include` is omitted, the effective include set is `messages`. The include
set is exact, and `reasoning` expands to include `messages`. The command does
not contact a provider, open an editor, or upload content. Fullscreen TUI
reports success or failure in one command transcript row, and non-terminal
scripted TUI prints the same bounded text.

`/share [path] [-i|--include list]` writes selected local shareable Markdown
sections for the current persisted session and reports its path. When `path` is
omitted, it writes `psychevo-share-<short-session-id>.md` in the selected
workdir. It is intentionally a local packaging step only: it does not create a
public link, call a remote share API, create a gist, or persist durable sharing
state. The share include vocabulary is restricted to `header` (`h`), `messages`
(`m`), `reasoning` (`r`), and `provider-input-evidence` (`pie`);
`last-provider-request` and legacy raw provider request flags are unsupported.

`/show-raw` toggles raw transcript visibility. `/show-raw on` and
`/show-raw off` set it explicitly. It is a display-only mode and does not
rewrite stored transcript content, provider payloads, non-terminal renderer
output, or `/copy` results. Fullscreen TUI refreshes existing transcript rows
immediately and must not append a command transcript row. `/raw` is obsolete
and unsupported.

`/usage` shows local usage and estimated-cost statistics for the current
workdir from persisted SQLite accounting. Fullscreen TUI opens the shared
bottom selection pane in a read-only usage mode; non-terminal scripted TUI
prints the same deterministic summary. `/stats` is an alias for `/usage`.
Neither command may call providers or refresh model catalogs.

`/context` shows context-window usage as one compact block. Fullscreen TUI
appends one command transcript row titled `Context Usage` for the latest
provider request snapshot when available, otherwise a current-session estimate.
Fullscreen rendering may include an adaptive colored context bar when a context
limit is known. The bar uses the available transcript width, rounded down to a
multiple of five cells, with a minimum of 50 cells and a maximum of 100 cells;
its legend renders on the following line. In fullscreen rich rendering, legend
markers `S`, `T`, `K`, `M`, and `.` use the same category colors as the bar
cells while the label text remains normal body text. Human text renders the
model-facing `messages` category as `input_messages`, including the legend
label and role count rows, while structured snapshots keep the `messages`
category key. Non-terminal scripted TUI prints the same compact text without a
bar or command-row wrapper. `/context` does not accept arguments and must not
call providers.

Fullscreen `/sessions`, `/resume`, `/continue`, and `/model` use bottom panes
with title text, selected-row highlighting, footer hints, `Enter` selection,
`Esc` close or back, arrow/Page/Home/End navigation, and scrolling. Shared
bottom selection panes do not render subtitles.

`/sessions`, `/resume`, and `/continue` show date-grouped session rows sorted by
latest persisted activity with right-aligned activity time and visible-message
counts. Selecting, viewing, or resuming a session does not update that activity
time; persisting new transcript material does. The pane title identifies whether
it is showing active or archived sessions, and the footer exposes `Tab` view
switching plus the action-mode entrypoint. Right alignment and row truncation
must use terminal display width so CJK/wide-character titles do not wrap the
activity time onto a second line. Selecting an active session replaces the
transcript with that session's sanitized history and does not add a status row.
Selecting an archived session restores it, switches to it, replaces the
transcript with its sanitized history, and does not add a status row or make the
session latest by itself. In non-terminal scripted mode,
`/sessions`, `/resume`, and `/continue` print a deterministic active-session
list instead of opening a panel.

Fullscreen `/model` opens a tabbed bottom pane with `Models` and `Info` tabs,
using the same tab header behavior as `/help`. It opens on `Models`. `Tab` and
`Right` switch to `Info`; `BackTab` and `Left` switch back to `Models`. The
current query, selected row, and scroll position are preserved when switching
tabs. `Esc` closes the model pane from either tab and cancels unfinished model
catalog fetches.

At TUI startup, if `$PSYCHEVO_HOME/models_dev_cache.json` is absent, TUI starts
one non-blocking, best-effort `models.dev` metadata cache warmup. Startup,
rendering, and command handling must not wait for this request. Warmup success
silently refreshes local model metadata for future panes. The cache file stores
only user-relevant models: the current intended model selection, recent TUI
models, and locally configured model entries. Warmup failure is silent by
default and may only surface as a warning when debug output is enabled.

The `Models` tab shows search directly below the tab header, an `Add provider`
action row, a status-style `All providers` row, and a selectable provider status
row before each provider's models. These action rows replace non-selectable
provider group headers. Selecting `Add provider` opens a bottom-panel wizard for
creating a global user-defined OpenAI Chat-compatible provider. Selecting `All
providers` concurrently fetches every fetchable provider catalog; selecting a
provider row fetches or retries only that provider. Fetch rows use `Enter
fetch` in the footer. Model rows use `Enter select`, and `Enter` continues to
open variant selection.

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

Within fullscreen `/model`, `Ctrl+R` explicitly refreshes the `models.dev`
metadata cache. This action is separate from provider `/models` catalog fetches:
it does not call provider APIs, does not use API keys, and does not validate live
providers. It writes only user-relevant model entries to the cache, using the
same target set as startup warmup. While the refresh is pending the panel shows
`refreshing metadata`; completion shows `metadata refreshed`; failure shows
`metadata refresh failed: <short error>`. Refresh completion rebuilds the model
pane while preserving tab, query, selected row, and info scroll.

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

Model rows show known model metadata compactly in the `Models` tab: context and
output limits, capability tags, and input/output/cache pricing when available.
Metadata may come from config, existing `models.dev` cache, explicit metadata
refresh, or explicit provider catalog fetches. Unknown metadata is omitted
rather than shown as zero. The `Info` tab is a
read-only detail view for the currently selected model row. Non-model action or
provider rows show a bounded empty state instead of details. The `Info` tab
shows known values only and expands the selected model metadata into identity
and source, limits, capabilities, modalities, pricing, pricing source, row
source, current/default markers, and configured default variant. Capabilities
with known `false` values render as explicit negatives such as `no reasoning`
or `no tools`; unknown capabilities and unknown modalities are omitted. `Info`
supports `Up`/`Down`/`PageUp`/`PageDown`/`Home`/`End` scrolling and treats
`Enter` as a no-op.

The model picker keeps local rows authoritative. When a local configured model
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

Obsolete slash commands are not kept as compatibility redirects. Inputs such
as `/models`, `/model set <provider/model>`, `/variant set <value>`, `/mode set
<value>`, `/thinking`, `/session list`, `/session show`, and `/session switch`
are unsupported command forms and must not appear in the slash menu.

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
The settled transcript renders the aborted foreground work with an explicit
`interrupted` marker rather than ordinary failure styling. While the interrupt
is still in progress, the bottom status line continues to show `interrupting`.
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
