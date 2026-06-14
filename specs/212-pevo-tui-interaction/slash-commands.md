---
name: 212. pevo TUI Interaction Slash Commands
psychevo_self_edit: deny
---

# 212. pevo TUI Interaction Slash Commands

Define slash command inventory, parsing, command feedback, bottom panes, model/session commands, file completion, and local command behavior.

## Slash Commands

The first TUI supports:

- `/help`
- `/quit`, `/exit`, `/q`
- `/status`
- `/usage`, `/stats`
- `/context`
- `/diff`
- `/refresh`
- `/btw [prompt]`
- `/steer <message>`
- `/queue <message>`
- `/pending cancel`
- `/clear`, `/new`
- `/sessions`, `/resume`, `/continue`
- `/model`
- `/tools`
- `/variant <none|minimal|low|medium|high|xhigh|max>`
- `/mode <plan|default|acceptEdits|dontAsk|bypassPermissions>`
- `/permissions`
- `/sandbox`
- `/show-thinking`
- `/show-thinking on`
- `/show-thinking off`
- `/show-raw`
- `/show-raw on`
- `/show-raw off`
- `/copy`
- `/export [path] [-f|--format markdown|json] [-i|--include list]`
- `/share [path] [-i|--include list]`
- `/image <source> [prompt]`
- `/rename <title>`
- `/undo`
- `/redo`
- `/agents`
- `/fork`
- `/compact [instructions]`
- `/skills`
- `/bundles`
- `/curator`
- `/<skill-or-bundle> [args]`

Slash command discovery is backed by the shared runtime command catalog,
parser, and UI-independent execution effects defined by
[026 Commands](../026-commands/spec.md). The TUI supplies terminal capabilities
such as picker, clipboard, renderer toggles, process exit, side conversation,
and image attachment, then projects shared command effects into panes,
transcript command rows, composer state, queues, and approvals. The slash menu
stays a flat list with at most 8 rows and does not show group headers. Built-in
compatibility aliases may match the canonical command row but do not appear as
independent menu rows. User-configured aliases appear as alias rows when
matched: the row label is the alias token, the description identifies the
configured target slash line and canonical summary, Tab completes to the alias
token, and Enter or mouse selection submits the alias token so normal alias
expansion performs execution.
Peer-agent ACP slash commands are rendered as `/agent:command` entries when a
peer thread or peer-capable agent context exposes them. Selecting or submitting
one sends the peer's un-namespaced slash command through Gateway command
execution. Psychevo core commands keep priority over any peer command with the
same name.
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
`/status`, `/context`, `/refresh`, `/btw`, `/model`, `/sessions`, `/new`, `/copy`, `/undo`,
`/redo`, and `/quit`. `Commands` lists all built-in slash commands in canonical registry
order. `Custom commands` summarizes dynamic skill and bundle invocation as
`/<skill-or-bundle> [args]` with the discovered count and lists each concrete
slash line configured with user aliases or shortcuts. It reports
`No custom commands available` only when no skills and no configured slash
targets are available.
Help rows use `<usage> - <summary>` and may append compact alias text on the
canonical command row. If a command has configured shortcuts, the same row may
also append compact shortcut text. Rows with expanded detail render the detail
immediately after the command row as a short indented continuation line. Fullscreen `/help`
opens a bottom help pane with
`Help`, `General`, `Commands`, and `Custom commands` header tabs; `Esc` closes
the pane, and tab/arrow navigation may switch help sections. Scripted `/help`
prints the same deterministic help text without opening a pane.

`/diff` is backed by the shared command catalog and the semantics in
[214 pevo Diff Command](../214-pevo-diff-command/spec.md). Fullscreen TUI opens
a read-only `D I F F` overlay pager instead of appending a command transcript
row. The overlay is a static snapshot, may be used while a turn is active,
closes with Esc, and supports normal transcript scrolling keys.

`/status` shows workdir, home, db, session, model, variant, mode, permission
mode, and debug state as one multi-line status block. It does not include thinking or raw
visibility; `/show-thinking` and `/show-raw` remain the dedicated commands for
changing and reporting those settings. Fullscreen TUI appends one command
transcript row without an extra `Status` title, and non-terminal scripted TUI
writes the same multi-line status text as one output block.

Composer submission classifies input before slash parsing. Leading shell
escapes keep taking precedence. Known slash commands parse as commands, and
known command argument errors remain bounded local errors. Unknown
slash-looking input passes through as ordinary prompt text with the original
submitted line, so inputs such as `/home/me/docs/README.md`,
`/made-up explain this`, and an uninstalled `/<skill-or-bundle> [args]` reach
the model. Ordinary prompt text is not scanned for image paths or image URLs:
`描述这张图片的内容：img1.avif`, `/home/me/out.avif`, `@img.avif`,
`https://example.com/image.png`, and `data:image/*;base64,...` are all prompt
text unless they came from an existing pending image placeholder. This prevents
output-path prompts from being misclassified as missing input images. The only
fullscreen attachment entrypoints are `/image <source> [prompt]` and a
standalone paste that resolves to a readable image source.

During a foreground agent turn, ordinary non-slash prompt submission is a
steer request for the current turn. `/steer <message>` is the explicit form and
is valid only while the foreground running item is an agent turn; when idle,
inside non-agent work, or inside an unavailable target, it reports bounded
feedback and does not queue implicitly. A steered input remains pending UI
until the stream confirms the committed user message. While pending, its text is
visible in the fixed pending preview above the composer rather than only as a
status count. Once the stream confirms that the input was committed, the
preview entry is removed and the message appears in history as ordinary user
transcript content.

`/queue <message>` appends a prompt to the caller-owned next-turn FIFO queue.
When the TUI is idle, it drains immediately by starting the next turn through
normal runtime invocation. When work is running, it waits behind earlier queued
items. `/pending cancel` cancels all not-yet-sent steer inputs and clears all
not-yet-started queued inputs for the current foreground surface. The status
line does not show separate steer/queue counters; pending text visibility
belongs to the fixed pending preview. Queued prompt text is visible in the same
fixed pending preview as steer text. The preview's `undo` action removes queued
prompts before they start, and `edit` updates the queued prompt in place while
its sequence is still waiting.

Dynamic skill and bundle slash commands submit as prompt input when accepted
from the slash menu, mouse selection, or a typed complete command. Dynamic
commands are discovered after core commands and may be capped by non-TUI
surfaces; the TUI menu continues to show matched dynamic commands within its
normal row limit. The composer must not be rewritten to a `$skill` or `$bundle`
marker as the visible result of Enter. Internally, the submitted slash line
resolves to the same explicit marker text used by runtime skill expansion,
while fullscreen transcript/history display keeps the slash line the user
submitted.

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
`data:image/*;base64,...` URLs when they are supplied through `/image`.
Standalone paste creates an attachment only for local sources that resolve to a
readable image file; pasted `http(s)://` and `data:image/*` URLs remain ordinary
composer text unless the user supplies them through `/image`. Local paths must
resolve to readable files with supported image extensions and must not exceed
the configured local source size limit before an attachment is created. Remote
URLs are not downloaded or preflighted locally. If selected model metadata
explicitly says image input is unsupported, the TUI does not send structured
image blocks; it degrades the submission to text containing the attachment
source list plus the prompt with image
placeholders removed, with bounded feedback telling the user the image was
degraded to text.

`/copy` copies the latest visible assistant answer as raw Markdown source. It
does not copy Thinking, tool evidence, metadata, selected transcript rows, or
rendered rich text. It is unaffected by `/show-raw`, so rich and raw transcript
display modes copy the same source text. Fullscreen TUI reports copy success or
failure through short status feedback and must not append a command transcript
row. If no assistant answer is visible, it reports a bounded failure status.

`/export [path] [-f|--format markdown|json] [-i|--include list]` writes selected
sections from the current persisted session as a local artifact. When `path` is
omitted, Markdown writes
`psychevo-session-<short-session-id>.md` and JSON writes
`psychevo-session-<short-session-id>.json` in the selected workdir. The short
session id is long enough to distinguish sibling parent and child sessions
created in the same time window. When `path` is relative, it resolves against
the selected workdir. The command uses the same include semantics and section
projection as `pevo session export`. The
export include vocabulary is `header` (`h`), `messages` (`m`), `reasoning`
(`r`), `provider-input-evidence` (`pie`), `last-provider-request` (`lpr`),
and `last-provider-response`.
If `--include` is omitted, the effective include set is `messages`. The include
set is exact, and `reasoning` expands to include `messages`.
`last-provider-response` exports the latest persisted assistant response
projection from stored message, usage, and metadata rows; it is not an exact raw
provider payload because provider SSE chunks are not persisted. The command does
not contact a provider, open an editor, or upload content. Fullscreen TUI
reports success or failure in one command transcript row, and non-terminal
scripted TUI prints the same bounded text.

`/share [path] [-i|--include list]` writes selected local shareable Markdown
sections for the current persisted session and reports its path. When `path` is
omitted, it writes `psychevo-share-<short-session-id>.md` in the selected
workdir, using the same collision-resistant short session id as export
filenames. It is intentionally a local packaging step only: it does not create a
public link, call a remote share API, create a gist, or persist durable sharing
state. The share include vocabulary is restricted to `header` (`h`), `messages`
(`m`), `reasoning` (`r`), and `provider-input-evidence` (`pie`);
`last-provider-request`, `last-provider-response`, `-f`/`--format`, and legacy
raw provider request flags are unsupported.

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

`/refresh` is the TUI-visible refresh entrypoint. It rebuilds the current
session prompt prefix using the existing reload-context behavior and schedules a
background cleanup of orphaned hidden `/btw` side sessions for the current
workdir. Fullscreen TUI rejects `/refresh` as a whole while the active thread is
running, writes one command transcript row for the context reload and scheduled
cleanup, and reports cleanup completion through transient status text with the
deleted side-session count. `/refresh` is disabled inside `/btw` side
conversations. `/reload-context` is no longer exposed through TUI help, menu,
or common command discovery; direct TUI input of `/reload-context` must produce
bounded feedback telling users to use `/refresh`. Non-TUI session
reload-context APIs remain available.

`/compact [instructions]` manually compacts the current main-session context as
defined by [130 Context Compaction](../130-context-compaction/spec.md). Optional
trailing instructions guide summary focus. If a turn is running, fullscreen TUI
queues manual compaction behind that turn and ahead of later queued prompts; it
does not interrupt the turn. Completion reports before/after token estimates
and a folded display-only summary row that is not a durable session message.
Scripted TUI prints bounded compaction feedback.

`/btw [prompt]` opens a temporary side conversation for local side work. `/btw`
opens an empty side conversation; `/btw <prompt>` opens it and immediately
submits `<prompt>` as the first side prompt. `/side` is a hidden compatibility
alias with identical behavior and does not appear as a separate help or menu
row. A side conversation is implemented as a hidden temporary child session that
inherits a startup snapshot of the parent conversation and inserts hidden
boundary instructions marking inherited history as reference-only. Later parent
output is not merged into the side context. The side session inherits the
current model, reasoning, mode, permissions, selected agent, skills, and tool
surface at creation time. Tools remain available under the current permission
policy, and explicit workspace mutations requested inside the side conversation
are real workspace changes; deleting the side session does not revert them.
Side-local model, reasoning, and permission changes do not affect the parent
after return.

While the user is inside a side conversation, the status area identifies it as a
side conversation and shows parent status such as running, needs input, needs
approval, failed, interrupted, closed, or finished. If the parent turn is
running when `/btw` starts, fullscreen TUI detaches it into the existing
auxiliary-running path so it continues while the side conversation is active.
`Ctrl+C` inside an idle side conversation returns to the parent, deletes the
hidden side session and messages, reloads the parent transcript, replays any
buffered parent live events, and shows only transient feedback. `Ctrl+C` during
a running side turn interrupts that side turn first; a later `Ctrl+C` returns
to the parent. Side conversations are not workspace sandboxes: only the
temporary session transcript is deleted.

Side conversations allow only a limited slash-command set:
`/help`, `/status`, `/context`, `/model`, `/variant`, `/mode`, `/permissions`,
`/sandbox`, `/show-thinking`, `/show-raw`, `/copy`, `/export`, `/share`,
`/quit`, `/exit`, and `/q`. Nested `/btw` or `/side`, `/refresh`, session
navigation, `/new`, `/clear`, `/undo`, `/redo`, `/agents`, `/fork`, `/compact`,
`/skills`, and skill invocation are rejected with bounded feedback. `/quit`,
`/exit`, and `/q` keep their normal meaning of exiting the program; returning
to the parent is only `Ctrl+C`.

Fullscreen `/sessions`, `/resume`, `/continue`, `/model`, and `/tools` use bottom panes
with title text, selected-row highlighting, footer hints, `Enter` selection,
`Esc` close or back, arrow/Page/Home/End navigation, and scrolling. Shared
bottom selection panes do not render subtitles.

`/permissions` shows the effective approval mode, permission mode, configured
local allow/ask/deny rules, and the project-local config path. Text mutation
forms accepted by shared non-picker surfaces are
`/permissions allow|ask|deny <rule>` and
`/permissions remove <allow|ask|deny> <rule>`; TUI may continue to offer richer
picker affordances for the same policy edits. Permission policy semantics are
defined by [041 Permissions](../041-permissions/spec.md); this topic owns only
the interactive projection.

`/sandbox` shows the effective sandbox mode, backend, confinement status, and
configured roots. Sandbox enforcement semantics are defined by
[045 Sandbox](../045-sandbox/spec.md); this topic owns only the interactive
projection.

Fullscreen `/tools` opens a searchable bottom pane of built-in and configured
toolsets. Rows show enabled, disabled, or available status for the current
runtime mode plus the expanded tool count. `Enter` toggles the selected toolset
for the current mode by editing project-local `.psychevo/config.toml`; shared
text forms `/tools enable <toolset>` and `/tools disable <toolset>` perform the
same current-mode mutation for non-picker surfaces. Changes apply to future
turns. The pane does not start provider calls or modify a running turn.

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

Fullscreen `/model` opens a tabbed bottom pane with `Models` and `Info` tabs
in local-config mode. `/model -g` and `/model --global` open the same pane in
global-config mode. It uses the same tab header behavior as `/help` and opens
on `Models`. `Tab` and `Right` switch to `Info`; `BackTab` and `Left` switch
back to `Models`. The current query, selected row, and scroll position are
preserved when switching tabs. `Esc` closes the model pane from either tab and
cancels unfinished model catalog fetches.

At TUI startup, if `$PSYCHEVO_HOME/models_dev_cache.json` is absent, TUI starts
one non-blocking, best-effort `models.dev` metadata cache warmup. Startup,
rendering, and command handling must not wait for this request. Warmup success
silently refreshes local model metadata for future panes. The cache file stores
only user-relevant models: the current intended model selection, recent TUI
models, and locally configured model entries. Warmup failure is silent by
default and may only surface as a warning when debug output is enabled.

The `Models` tab shows search directly below the tab header, an `Add provider`
action row, an `All providers` status row, and a selectable provider status
row before each provider's models. These action rows replace non-selectable
provider group headers. Selecting `Add provider` opens a bottom-panel wizard for
creating a global user-defined OpenAI Chat-compatible provider. Selecting `All
providers` concurrently fetches every fetchable provider catalog; selecting a
provider row fetches or retries only that provider. Fetch rows use `Enter
fetch` in the footer. Model rows use `Enter select`, and `Enter` continues to
open variant selection before writing the selected local or global default
model.

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
`$PSYCHEVO_HOME/config.toml`, writes raw API keys only to
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
selection writes the selected scope's TOML default model, clears TUI-local
model and variant overrides for the current workdir, and updates recent models.
When the user chooses an explicit variant, the TOML default model is written in
object form with `reasoning_effort`. When the user chooses `Config default`,
TUI writes the selected model's known configured `reasoning_effort` when one is
available; otherwise it writes only the provider/model id. It does not write
TOML provider metadata or credentials. If the picker was opened with
`/model --global` and the current workdir has a local model setting that still
wins effective resolution, the completion status reports the global write and
local override.

All bottom selection panes keep `Home` and `End` as direct first/last jumps, and
their `Up` and `Down` navigation wraps between the first and last visible rows.

Obsolete slash commands are not kept as compatibility redirects. Inputs such
as `/models`, `/model set <provider/model>`, `/model fetch`,
`/variant set <value>`, `/mode set
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

User-configured slash aliases are loaded from effective `config.toml`
`tui.slash_aliases`. Keys are concrete built-in slash input lines, including
arguments or flags, validated by the same slash parser used for user input.
Values must be aliases beginning with `/` and containing no whitespace. An
alias expands to the configured concrete slash line before parsing; if the
submitted alias has trailing text, that text is appended to the configured
target line before parsing. Configured aliases are accepted anywhere built-in
aliases are accepted, including scripted TUI input, but they are never emitted
as separate command registry rows. In the fullscreen slash menu, matched
configured aliases are displayed as alias rows so they have the same completion
affordance as ordinary slash commands.

Configured alias startup validation rejects aliases that conflict with any
built-in canonical command, built-in alias, dynamic skill or bundle slash name,
obsolete command compatibility boundary, or another configured alias. Dynamic
skill and bundle command names are not user-aliasable in v1.

`/skills` is a skill hub dispatcher. With no arguments it shows a bounded hub
dashboard/help block; read subcommands include `list`, `browse`, `search`,
`inspect`, `check`, `audit`, and `reload`. In fullscreen mode it appends a
bounded status transcript block; in scripted mode it prints the same
information deterministically. Mutating `/skills` subcommands that write
scoped state default to the current workdir `.psychevo`, accept `--local` as an
explicit local scope, accept `-g`/`--global` for global scope, and reject legacy
`--scope` and `--project` forms.

`/bundles` shows local bundle status and help, and `/curator` shows curator
status/help. Mutating subcommands under `/skills`, `/bundles`, and `/curator`
go through Psychevo permissions.

`/<skill-or-bundle> [args]` inserts a `$skill` or `$bundle` marker into the
composer using the expansion contract from [055 Skills](../055-skills/spec.md).
It does not auto-submit in fullscreen mode. Unknown names report bounded error
and do not submit a provider prompt.

The slash menu appears above the composer while the composer contains a slash
command token. It shows at most 8 matched rows. Matching uses the canonical
command label and orders exact matches first, prefix matches next, and
subsequence fuzzy matches last while preserving menu order within each class.
When the typed token matches a built-in alias, the canonical row is shown and
selected using the same ordering class as the alias match. When it matches a
user-configured alias, the alias row is shown and selected using the same
ordering class as the alias match.
When skill commands are enabled, discovered skills and bundles appear as dynamic
`/<name>` rows after built-in slash commands and participate in the same
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
